// This module is the authorized place to call std::fs's follow-symlink
// write/remove primitives directly; everything else in the crate should
// route through the safe wrappers defined here instead (see clippy.toml).
#![allow(clippy::disallowed_methods)]

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Component, Path, PathBuf};

/// Creates a brand-new file at `path`, atomically refusing to follow (and
/// erroring on) any symlink or junction already occupying that exact path.
///
/// - Unix: relies on `O_CREAT | O_EXCL`, which POSIX guarantees fails with
///   `EEXIST` when `path` already exists, even as a symlink, without
///   dereferencing it.
/// - Windows: `CREATE_NEW` alone is not enough, since by default Windows
///   transparently reparses (follows) an existing reparse point before
///   checking for existence at the target, potentially creating the file
///   outside `root`. Passing `FILE_FLAG_OPEN_REPARSE_POINT` makes the open
///   operate on the reparse point itself, so an existing symlink/junction at
///   `path` correctly fails with "already exists" instead of being followed.
pub fn create_new_no_follow(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    retry_on_transient_permission_denial(|| options.open(path))
}

/// Opens `target` for reading and writing, creating it (and its parent
/// directory, if missing) if it doesn't yet exist, while never resolving
/// *any* path component — ancestor or final — through a symlink/junction.
/// Unlike [`create_new_no_follow`], this must succeed when `target`
/// already legitimately exists as a regular file (an existing lock file
/// being reopened to lock it again), so it can't rely on `O_CREAT |
/// O_EXCL`'s atomicity alone.
///
/// A stat-then-open sequence — check every ancestor with
/// [`ensure_no_symlink_ancestor`], then open the final component with
/// `O_NOFOLLOW`/`FILE_FLAG_OPEN_REPARSE_POINT` — only protects that final
/// component atomically. Every *ancestor* directory in the path is still
/// resolved by name from the top on the subsequent open, which leaves a
/// real window: a different process could replace an ancestor (e.g. this
/// project's own `.markharness/`) with a symlink/junction after the check
/// passed but before the open runs, and the open would silently follow it.
///
/// - **Unix**: substantially narrows that window by resolving the path one
///   component at a time via raw `openat`/`mkdirat` (the `libc` crate,
///   added specifically for this), each one opened *relative to the
///   already-open file descriptor for its parent* — `O_DIRECTORY |
///   O_NOFOLLOW` for every intermediate directory, `O_NOFOLLOW` for the
///   final file — rather than by re-resolving a path from the top each
///   time. This closes the *symlink-substitution* variant of the ancestor
///   race completely: `O_NOFOLLOW` makes the kernel refuse, atomically, to
///   traverse through a symlink/junction at any step, so an ancestor
///   replaced with one is never silently followed, no matter when the
///   replacement happens relative to this call.
///
///   This does **not** close a related but distinct variant: an ancestor
///   (most concretely `.markharness/` itself) being *deleted and recreated
///   as an ordinary, non-symlink directory* by a concurrent writer.
///   `O_NOFOLLOW` has nothing to say about that case — the freshly
///   recreated directory is a perfectly ordinary directory, not a symlink,
///   so `openat` legitimately opens it. If that swap happens *between* two
///   different processes' calls to this function (rather than mid-way
///   through one call), each process's own resolution is internally
///   self-consistent, but the two processes can end up holding locks on
///   two different underlying files that happen to share the same path —
///   a split-brain, not prevented by anything this function does. Closing
///   that variant would require an OS-level guarantee that no name-based
///   locking scheme can provide on its own (POSIX `flock` on a
///   conventional path has the identical exposure): either a persistent,
///   externally-verified identity for `.markharness/` established once
///   and checked on every acquire (itself subject to the same swap
///   problem for *its own* storage), or filesystem-level enforcement that
///   `.markharness/` cannot be deleted while the project is in use (e.g.
///   `chattr +i` on Linux), which is a deployment/environment concern
///   outside what an application can enforce through path-based APIs.
///   Given the capability this requires — concurrent write access to the
///   project directory, timed to race a lock acquisition, in order to
///   *delete `.markharness/` and everything durably recorded under it* —
///   an attacker in that position already has direct, simpler means to
///   cause equivalent or worse damage (e.g. overwriting
///   `.markharness/identity-events/*.yml` outright), so this residual
///   exposure is accepted rather than chased further here.
/// - **Windows**: falls back to the stat-then-open sequence above, which
///   additionally leaves the symlink-substitution variant only narrowed
///   (not closed): Win32 has no directly accessible equivalent to
///   `openat` — the NT native API's `NtCreateFile` supports relative
///   opens via `OBJECT_ATTRIBUTES.RootDirectory`, but reaching it requires
///   FFI into `ntdll.dll`, a meaningfully larger and riskier dependency
///   than this narrow, local threat was judged to justify. The window
///   this leaves on Windows is real but narrow (a handful of filesystem
///   calls wide); it is a deliberately accepted residual risk, recorded
///   in full (condition, required capability, mitigations, rejected
///   alternative, and reconsideration triggers) in design doc §6.4's
///   Windows-specific accepted-risk addendum, not silently assumed closed.
///
/// After opening (on both platforms), verifies the result names a genuine
/// regular file — Unix rejects anything that isn't `is_file()` (catching
/// directories, symlinks, and non-regular types like FIFOs/sockets/devices
/// in one check); Windows rejects a directory, a symlink, any other kind of
/// reparse point (checked via the raw `FILE_ATTRIBUTE_REPARSE_POINT` bit,
/// since `is_symlink()` only recognizes specific reparse tags), and —
/// because Windows' legacy DOS device names (`NUL`, `CON`, `COM1`, ...) are
/// intercepted by name before ever reaching the filesystem, leaving no NTFS
/// attributes to inspect at all — anything `GetFileType` doesn't classify as
/// `FILE_TYPE_DISK`. This is what a caller never locks or writes to the
/// wrong file even if the platform-specific mechanism above and this check
/// somehow disagreed with each other.
#[cfg(unix)]
pub fn open_lock_file_no_follow(root: &Path, target: &Path) -> io::Result<File> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::io::FromRawFd;

    let relative = target.strip_prefix(root).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is not inside {}", target.display(), root.display()),
        )
    })?;

    let mut normal_components: Vec<&std::ffi::OsStr> = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(name) => normal_components.push(name),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "{} escapes {} via a non-normal path component",
                        target.display(),
                        root.display()
                    ),
                ));
            }
        }
    }
    let Some(file_name) = normal_components.pop() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "{} names no file relative to {}",
                target.display(),
                root.display()
            ),
        ));
    };

    let root_cstr = CString::new(root.as_os_str().as_bytes())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let mut dir_fd = unsafe {
        libc::open(
            root_cstr.as_ptr(),
            libc::O_DIRECTORY | libc::O_RDONLY | libc::O_CLOEXEC,
        )
    };
    if dir_fd < 0 {
        return Err(io::Error::last_os_error());
    }

    for component in normal_components {
        let name = CString::new(component.as_bytes())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        // Best-effort create; `AlreadyExists` (from an earlier run or a
        // racing process) is fine — the `openat` below, not this call, is
        // what actually verifies and uses whatever is there.
        if unsafe { libc::mkdirat(dir_fd, name.as_ptr(), 0o777) } != 0 {
            let err = io::Error::last_os_error();
            if err.kind() != io::ErrorKind::AlreadyExists {
                unsafe { libc::close(dir_fd) };
                return Err(err);
            }
        }
        let next_fd = unsafe {
            libc::openat(
                dir_fd,
                name.as_ptr(),
                libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_RDONLY | libc::O_CLOEXEC,
            )
        };
        unsafe { libc::close(dir_fd) };
        if next_fd < 0 {
            return Err(io::Error::last_os_error());
        }
        dir_fd = next_fd;
    }

    let file_name_cstr = CString::new(file_name.as_bytes())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let file_fd = unsafe {
        libc::openat(
            dir_fd,
            file_name_cstr.as_ptr(),
            libc::O_RDWR | libc::O_CREAT | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o666,
        )
    };
    unsafe { libc::close(dir_fd) };
    if file_fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let file = unsafe { File::from_raw_fd(file_fd) };
    let file_type = file.metadata()?.file_type();
    if !file_type.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "refusing to use a non-regular-file lock path: {}",
                target.display()
            ),
        ));
    }
    Ok(file)
}

#[cfg(windows)]
pub fn open_lock_file_no_follow(root: &Path, target: &Path) -> io::Result<File> {
    ensure_no_symlink_ancestor(root, target)?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    let file = retry_on_transient_permission_denial(|| options.open(target))?;
    let metadata = file.metadata()?;
    let file_type = metadata.file_type();
    // `FileType::is_symlink()` only recognizes specific reparse tags
    // (symlink, mount point); it does not catch every other kind of
    // reparse point (e.g. cloud-file placeholders, WSL/Bind links) that
    // `FILE_FLAG_OPEN_REPARSE_POINT` opens without following. Checking the
    // raw attribute bit directly catches all of them, regardless of tag.
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    let is_reparse_point = metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0;
    // Attribute checks alone still don't guarantee "a genuine regular
    // file": Windows' legacy DOS device names (`CON`, `NUL`, `PRN`, `AUX`,
    // `COM1`-`COM9`, `LPT1`-`LPT9`) are intercepted by name before ever
    // reaching the filesystem, so a path component that happens to collide
    // with one opens a handle to that device instead — a handle with no
    // NTFS attributes to inspect at all (`FILE_ATTRIBUTE_REPARSE_POINT`
    // included), since it was never backed by a filesystem entry. The one
    // way to positively confirm the handle is backed by an on-disk file
    // rather than a device or pipe is `GetFileType`, which classifies the
    // underlying kernel object directly.
    if !is_disk_file(&file)? || file_type.is_dir() || file_type.is_symlink() || is_reparse_point {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "refusing to use a non-regular-file lock path: {}",
                target.display()
            ),
        ));
    }
    Ok(file)
}

/// Classifies `file`'s underlying kernel handle via the Win32 `GetFileType`
/// API, returning whether it is `FILE_TYPE_DISK` — i.e. a genuine,
/// filesystem-backed file. Declared as a minimal, local `extern` binding
/// rather than pulling in a crate (e.g. `windows-sys`) for one function:
/// `kernel32.dll` is already always linked into every Windows Rust binary,
/// so this adds no new dependency or linker surface, unlike the `ntdll.dll`
/// FFI considered and rejected elsewhere in this module for the ancestor
/// `openat`-equivalent problem.
#[cfg(windows)]
fn is_disk_file(file: &File) -> io::Result<bool> {
    use std::os::windows::io::{AsRawHandle, RawHandle};

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetFileType(hfile: RawHandle) -> u32;
        fn SetLastError(dwerrcode: u32);
    }

    const FILE_TYPE_DISK: u32 = 0x0001;
    const FILE_TYPE_UNKNOWN: u32 = 0x0000;

    // MSDN: `GetFileType` returning `FILE_TYPE_UNKNOWN` is ambiguous — it
    // can mean either a genuinely unrecognized type or a real failure
    // inside the call itself. Clearing the last-error before calling and
    // checking it afterward is the documented way to tell them apart.
    unsafe { SetLastError(0) };
    let file_type = unsafe { GetFileType(file.as_raw_handle()) };
    if file_type == FILE_TYPE_UNKNOWN {
        let error = io::Error::last_os_error();
        if let Some(0) = error.raw_os_error() {
            // Genuinely unknown, not a failure; treat as non-disk.
            return Ok(false);
        }
        return Err(error);
    }
    Ok(file_type == FILE_TYPE_DISK)
}

/// Bounded retry-with-backoff for a filesystem operation that can spuriously
/// fail with `PermissionDenied` right after a *different* operation just
/// deleted or is deleting the same path — a well-known NTFS quirk: a file
/// briefly stays in a "pending delete" state (not fully gone until every
/// open handle closes) during which both a competing delete and a
/// create/recreate at that same path can observe `ERROR_ACCESS_DENIED`
/// instead of the clean "already exists" / "not found" POSIX unlink and
/// open give atomically. This never retries any other error kind —
/// `AlreadyExists`, `NotFound`, and every other genuine failure fail
/// immediately, exactly as before — so it does not turn any of this
/// crate's fail-fast, no-queuing contention checks (`create_new_no_follow`
/// callers like `identity::lock::IdentityLock::acquire`) into blocking
/// ones; it only bridges a transient OS-level race that has nothing to do
/// with whether the path is legitimately, durably occupied.
fn retry_on_transient_permission_denial<T>(mut op: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    const MAX_ATTEMPTS: u32 = 50;
    for attempt in 1..=MAX_ATTEMPTS {
        match op() {
            Ok(value) => return Ok(value),
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied && attempt < MAX_ATTEMPTS => {
                // A short, mildly increasing backoff: cheap for the common
                // case (cleared within the first attempt or two), while
                // still giving a generous total budget (a few hundred ms)
                // for a heavily loaded machine (e.g. many other tests
                // running in parallel) where the OS takes longer to finish
                // the pending delete this is waiting out.
                std::thread::sleep(std::time::Duration::from_millis((attempt as u64).min(10)));
            }
            Err(e) => return Err(e),
        }
    }
    unreachable!("loop always returns on its final attempt")
}

fn tmp_path_for(target: &Path) -> PathBuf {
    let mut name: OsString = target
        .file_name()
        .expect("replace_file target must have a file name")
        .to_os_string();
    name.push(".tmp");
    target.with_file_name(name)
}

/// Writes `content` to `target`, creating or replacing it, while refusing to
/// follow a symlink/junction anywhere along the way: in an ancestor
/// directory, at `target` itself, or at the temporary file used for the
/// atomic replace. This is the single write primitive managed writes under
/// `root` should go through instead of calling `std::fs::write`/`rename`
/// directly (see `clippy.toml`'s `disallowed-methods`).
///
/// A stale `<target>.tmp` left behind by a previously interrupted run is
/// removed before writing the new one. A `<target>.tmp` that is itself a
/// symlink/junction is rejected outright rather than unlinked: on Unix,
/// `remove_file` would happily unlink it (it never dereferences a symlink
/// into its target) and let the write proceed, but that would silently
/// treat an attacker-planted link at the tmp path as ordinary leftover
/// state instead of the tampering it is.
pub fn replace_file(root: &Path, target: &Path, content: &[u8]) -> io::Result<()> {
    ensure_no_symlink_ancestor(root, target)?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }

    let tmp_path = tmp_path_for(target);
    match fs::symlink_metadata(&tmp_path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "refusing to remove symlinked tmp path: {}",
                    tmp_path.display()
                ),
            ));
        }
        Ok(_) => fs::remove_file(&tmp_path)?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }

    let mut file = create_new_no_follow(&tmp_path)?;
    if let Err(e) = io::Write::write_all(&mut file, content) {
        drop(file);
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }
    drop(file);

    ensure_no_symlink_ancestor(root, target)?;
    let rename_result = fs::rename(&tmp_path, target);
    if rename_result.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }
    rename_result
}

/// Replaces a managed directory with a fully prepared staging directory.
/// If installing the staging directory fails after the old directory was
/// moved aside, the old directory is restored before returning the error.
pub fn replace_dir_from_staging(root: &Path, staging: &Path, target: &Path) -> io::Result<()> {
    ensure_no_symlink_ancestor(root, staging)?;
    ensure_no_symlink_ancestor(root, target)?;

    let backup = target.with_file_name(format!(
        ".{}.backup-{}",
        target
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("managed-dir"),
        std::process::id()
    ));
    if backup.exists() {
        remove_dir_all_no_follow(root, &backup)?;
    }

    // `fs::rename`'s destination requires an existing parent directory (unlike
    // `replace_file`, which callers may invoke before `target`'s ancestor
    // directories — e.g. `.markharness/` itself — have ever been created).
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }

    let had_target = target.exists();
    if had_target {
        fs::rename(target, &backup)?;
    }
    if let Err(error) = fs::rename(staging, target) {
        if had_target {
            let _ = fs::rename(&backup, target);
        }
        return Err(error);
    }
    if had_target {
        remove_dir_all_no_follow(root, &backup)?;
    }
    Ok(())
}

/// Removes the single file at `target`, refusing to follow a symlink/junction
/// at `target` itself or any of its ancestors. A missing `target` is treated
/// as success (removal is idempotent), mirroring `remove_dir_all_no_follow`.
pub fn remove_file_no_follow(root: &Path, target: &Path) -> io::Result<()> {
    ensure_no_symlink_ancestor(root, target)?;
    match retry_on_transient_permission_denial(|| fs::remove_file(target)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Recursively removes `target` and its contents, refusing to follow a
/// symlink/junction at `target` itself or any of its ancestors. A missing
/// `target` is treated as success (removal is idempotent).
///
/// This only guards `target` itself: `std::fs::remove_dir_all` already
/// avoids descending into symlinked subdirectories nested inside `target`
/// (a nested symlink is unlinked, not followed), so no per-entry check is
/// needed beyond the top-level one performed here.
pub fn remove_dir_all_no_follow(root: &Path, target: &Path) -> io::Result<()> {
    ensure_no_symlink_ancestor(root, target)?;
    match retry_on_transient_permission_denial(|| fs::remove_dir_all(target)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Rejects `target` if it, or any directory between `root` (exclusive) and
/// `target`, is a symlink. On Windows this also catches directory
/// junctions, since `FileType::is_symlink()` reports true for both.
/// Components that don't exist yet are not an error, since managed
/// directories and files (`knowledge/`, `generated/`, `executions/`,
/// `changes/`, `.markharness-cache/`, ...) are frequently created on demand.
///
/// Also rejects any `ParentDir` (`..`), `RootDir`, or `Prefix` component in
/// the part of `target` relative to `root`: `strip_prefix` only compares
/// path components lexically, so `root/a/../../outside` still strips down
/// to a relative path that walks back out of `root` once resolved by the
/// OS, even though no individual step is a symlink.
///
/// Guards against a malicious repository placing a link where a managed
/// directory or file is expected, so that a later
/// `remove_dir_all`/`write`/`rename` doesn't silently follow the link
/// outside `root`. This is a check-then-use guard, not an atomic one: for
/// the final write/replace step itself, prefer [`create_new_no_follow`]
/// (or [`replace_file`]) to close the remaining race between this check and
/// the operation it precedes.
pub fn ensure_no_symlink_ancestor(root: &Path, target: &Path) -> io::Result<()> {
    let relative = target.strip_prefix(root).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is not inside {}", target.display(), root.display()),
        )
    })?;

    let mut current = root.to_path_buf();
    for component in relative.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir => continue,
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "{} escapes {} via a non-normal path component",
                        target.display(),
                        root.display()
                    ),
                ));
            }
        }
        current.push(component);
        if let Ok(metadata) = fs::symlink_metadata(&current)
            && metadata.file_type().is_symlink()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("refusing to follow symlink ancestor: {}", current.display()),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn link_dir(link: &Path, target: &Path) {
        if let Some(parent) = link.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::os::unix::fs::symlink(target, link).unwrap();
    }

    #[cfg(windows)]
    fn link_dir(link: &Path, target: &Path) {
        if let Some(parent) = link.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let status = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/j"])
            .arg(link)
            .arg(target)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "mklink /j failed");
    }

    #[test]
    fn allows_a_target_with_no_symlink_ancestors() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let target = root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("generated")
            .join("testcases")
            .join("ground.yml");

        assert!(ensure_no_symlink_ancestor(root, &target).is_ok());
    }

    #[test]
    fn rejects_a_target_behind_a_symlinked_ancestor() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let outside = tempfile::tempdir().unwrap();
        let link = root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("generated");
        link_dir(&link, outside.path());
        let target = link.join("testcases").join("ground.yml");

        let result = ensure_no_symlink_ancestor(root, &target);

        assert!(
            result.is_err(),
            "expected an error for a target behind a symlinked ancestor, got: {result:?}"
        );
    }

    #[test]
    fn rejects_a_target_that_is_itself_a_symlinked_directory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let outside = tempfile::tempdir().unwrap();
        let target = root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("generated");
        link_dir(&target, outside.path());

        let result = ensure_no_symlink_ancestor(root, &target);

        assert!(
            result.is_err(),
            "expected an error for a target that is itself a symlink, got: {result:?}"
        );
    }

    #[test]
    fn rejects_a_target_that_escapes_root_via_parent_dir_components() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        fs::create_dir_all(&root).unwrap();
        let outside = dir.path().join("outside");
        let target = root.join("a").join("..").join("..").join("outside");
        assert_eq!(target.strip_prefix(&root).unwrap().components().count(), 4);

        let result = ensure_no_symlink_ancestor(&root, &target);

        assert!(
            result.is_err(),
            "expected an error for a target escaping root via '..', got: {result:?}"
        );
        assert!(!outside.exists());
    }

    #[test]
    fn replace_file_rejects_a_target_that_escapes_root_via_parent_dir_components() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        fs::create_dir_all(&root).unwrap();
        let outside = dir.path().join("outside.yaml");
        let target = root.join("a").join("..").join("..").join("outside.yaml");

        let result = replace_file(&root, &target, b"payload");

        assert!(result.is_err(), "expected an error, got: {result:?}");
        assert!(!outside.exists());
    }

    #[test]
    fn remove_dir_all_no_follow_rejects_a_target_that_escapes_root_via_parent_dir_components() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        fs::create_dir_all(&root).unwrap();
        let outside = dir.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("keep.txt"), "keep me").unwrap();
        let target = root.join("a").join("..").join("..").join("outside");

        let result = remove_dir_all_no_follow(&root, &target);

        assert!(result.is_err(), "expected an error, got: {result:?}");
        assert!(outside.join("keep.txt").exists());
    }

    #[test]
    fn create_new_no_follow_creates_a_writable_file_when_none_exists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("results.yml.tmp");
        use std::io::Write;

        let mut file = create_new_no_follow(&path).unwrap();
        file.write_all(b"hello").unwrap();
        drop(file);

        assert_eq!(fs::read_to_string(&path).unwrap(), "hello");
    }

    #[test]
    fn create_new_no_follow_rejects_a_path_occupied_by_a_symlinked_directory() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let path = dir.path().join("results.yml.tmp");
        link_dir(&path, outside.path());

        let result = create_new_no_follow(&path);

        assert!(
            result.is_err(),
            "expected an error for a path occupied by a symlinked directory, got: {result:?}"
        );
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    }

    #[test]
    fn open_lock_file_no_follow_creates_and_reopens_a_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let target = root.join(".identity.lock");

        let mut file = open_lock_file_no_follow(root, &target).unwrap();
        use std::io::Write;
        file.write_all(b"123").unwrap();
        drop(file);

        // Reopening an already-existing regular file must succeed too
        // (unlike `create_new_no_follow`, which is one-shot) and must not
        // truncate content the caller hasn't explicitly cleared itself.
        let file = open_lock_file_no_follow(root, &target).unwrap();
        drop(file);
        assert_eq!(fs::read_to_string(&target).unwrap(), "123");
    }

    /// Unix-only regression test for the *ancestor* TOCTOU window a plain
    /// stat-then-open sequence leaves open (only the final component gets
    /// atomic no-follow protection; every ancestor is still resolved by
    /// name on the actual open call, so a concurrent swap of an ancestor
    /// between the check and the open would silently be followed). The
    /// `openat`/`mkdirat`-based implementation resolves every component
    /// relative to the previous one's already-open file descriptor
    /// instead, so this exact race must never let anything be created
    /// through the swapped-in ancestor: a thread repeatedly calling
    /// `open_lock_file_no_follow` on a nested path races a second thread
    /// that repeatedly deletes and replaces the intermediate ancestor
    /// directory with a symlink to an unrelated `outside` directory. No
    /// matter how the two interleave, `outside` must stay empty — either
    /// a given attempt resolves entirely against the real ancestor
    /// directory it observed (succeeding there) or it fails outright; it
    /// must never partially resolve through the symlink.
    #[cfg(unix)]
    #[test]
    fn open_lock_file_no_follow_never_creates_anything_through_a_concurrently_swapped_ancestor() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let outside = tempfile::tempdir().unwrap();
        let ancestor = root.join(".markharness");
        let target = ancestor.join(".identity.lock");
        fs::create_dir_all(&ancestor).unwrap();

        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
        std::thread::scope(|scope| {
            let swapper = scope.spawn(|| {
                let mut swapped_in = true;
                while std::time::Instant::now() < deadline {
                    if swapped_in {
                        let _ = fs::remove_file(&ancestor);
                        let _ = fs::create_dir_all(&ancestor);
                    } else {
                        let _ = fs::remove_dir_all(&ancestor);
                        let _ = std::os::unix::fs::symlink(outside.path(), &ancestor);
                    }
                    swapped_in = !swapped_in;
                }
            });

            while std::time::Instant::now() < deadline {
                let _ = open_lock_file_no_follow(root, &target);
            }
            swapper.join().unwrap();
        });

        assert_eq!(
            fs::read_dir(outside.path()).unwrap().count(),
            0,
            "must never have created anything through the concurrently swapped-in symlink ancestor"
        );
    }

    /// Regression test: a directory junction placed at the lock path
    /// (using the same unprivileged mechanism this file's other
    /// symlink-safety tests already rely on, since real file symlinks need
    /// elevation/Developer Mode on Windows) must be refused outright, and
    /// its target must never be touched.
    #[test]
    fn open_lock_file_no_follow_rejects_a_path_occupied_by_a_symlinked_directory() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = dir.path();
        let path = root.join(".identity.lock");
        link_dir(&path, outside.path());

        let result = open_lock_file_no_follow(root, &path);

        assert!(
            result.is_err(),
            "expected an error for a path occupied by a symlinked directory, got: {result:?}"
        );
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    }

    #[test]
    fn open_lock_file_no_follow_rejects_a_path_that_is_a_plain_directory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let path = root.join(".identity.lock");
        fs::create_dir_all(&path).unwrap();

        let result = open_lock_file_no_follow(root, &path);

        assert!(
            result.is_err(),
            "expected an error for a plain directory at the lock path, got: {result:?}"
        );
    }

    /// A FIFO is neither a directory nor a symlink, so the earlier
    /// `is_dir() || is_symlink()` check let it through; only rejecting
    /// non-regular files outright (`!is_file()`) catches it. Locking or
    /// writing a PID into a FIFO would block or misbehave in ways this
    /// function's callers never expect.
    #[cfg(unix)]
    #[test]
    fn open_lock_file_no_follow_rejects_a_path_that_is_a_fifo() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let path = root.join(".identity.lock");
        let path_cstr = CString::new(path.as_os_str().as_bytes()).unwrap();
        let result = unsafe { libc::mkfifo(path_cstr.as_ptr(), 0o600) };
        assert_eq!(result, 0, "mkfifo failed: {}", io::Error::last_os_error());

        let result = open_lock_file_no_follow(root, &path);

        assert!(
            result.is_err(),
            "expected an error for a FIFO at the lock path, got: {result:?}"
        );
    }

    /// Creating a file symlink (unlike the directory junctions this file's
    /// other symlink-safety tests use) requires either an elevated process
    /// or Developer Mode on Windows. Report failure rather than panicking so
    /// this test can skip cleanly on a restricted machine instead of failing
    /// for an unrelated reason.
    #[cfg(windows)]
    fn link_file(link: &Path, target: &Path) -> bool {
        if let Some(parent) = link.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::os::windows::fs::symlink_file(target, link).is_ok()
    }

    /// A file-backed reparse point at the lock path must be rejected even
    /// though `FILE_FLAG_OPEN_REPARSE_POINT` opens it without following it
    /// (so it wouldn't fail with `AlreadyExists`/be silently dereferenced),
    /// and even for a reparse tag `FileType::is_symlink()` alone would not
    /// necessarily recognize — this exercises the raw
    /// `FILE_ATTRIBUTE_REPARSE_POINT` check added alongside `is_symlink()`.
    #[cfg(windows)]
    #[test]
    fn open_lock_file_no_follow_rejects_a_path_occupied_by_a_file_reparse_point() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = dir.path();
        let outside_file = outside.path().join("target.txt");
        fs::write(&outside_file, b"outside").unwrap();
        let path = root.join(".identity.lock");
        if !link_file(&path, &outside_file) {
            eprintln!(
                "skipping open_lock_file_no_follow_rejects_a_path_occupied_by_a_file_reparse_point: this platform/user refused to create a file symlink"
            );
            return;
        }

        let result = open_lock_file_no_follow(root, &path);

        assert!(
            result.is_err(),
            "expected an error for a path occupied by a file reparse point, got: {result:?}"
        );
    }

    /// Windows' legacy DOS device names (`NUL`, `CON`, `COM1`, ...) are
    /// intercepted by `CreateFile` before ever reaching the filesystem,
    /// regardless of the directory prefix in front of them, as long as the
    /// path isn't in verbatim (`\\?\`) form — which is exactly the form a
    /// short path like this test's tempdir-based one takes. Opening one
    /// yields a real, usable handle with no filesystem attributes at all
    /// (not a directory, not a reparse point), so only `GetFileType`
    /// classifying it as something other than `FILE_TYPE_DISK` catches it.
    #[cfg(windows)]
    #[test]
    fn open_lock_file_no_follow_rejects_a_path_that_is_a_reserved_device_name() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let path = root.join("NUL");

        let result = open_lock_file_no_follow(root, &path);

        assert!(
            result.is_err(),
            "expected an error for a path that resolves to the NUL device, got: {result:?}"
        );
    }

    #[test]
    fn replace_file_creates_the_file_and_its_parent_dir_when_none_exist() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let target = root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("executions")
            .join("m1")
            .join("results.yml");

        replace_file(root, &target, b"id: m1\n").unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "id: m1\n");
    }

    #[test]
    fn replace_file_overwrites_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let target = root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("changes")
            .join("m2.yaml");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, "old content").unwrap();

        replace_file(root, &target, b"new content").unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "new content");
    }

    #[test]
    fn replace_file_rejects_a_target_behind_a_symlinked_ancestor() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let outside = tempfile::tempdir().unwrap();
        link_dir(
            &root
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("changes"),
            outside.path(),
        );
        let target = root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("changes")
            .join("m2.yaml");

        let result = replace_file(root, &target, b"payload");

        assert!(result.is_err(), "expected an error, got: {result:?}");
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    }

    #[test]
    fn replace_file_rejects_and_leaves_outside_untouched_when_tmp_path_is_a_symlinked_directory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let outside = tempfile::tempdir().unwrap();
        let target = root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("changes")
            .join("m2.yaml");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        link_dir(&target.with_file_name("m2.yaml.tmp"), outside.path());

        let result = replace_file(root, &target, b"payload");

        assert!(result.is_err(), "expected an error, got: {result:?}");
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
        assert!(!target.exists());
    }

    #[test]
    fn replace_file_succeeds_despite_a_stale_tmp_file_left_by_a_previous_failed_run() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let target = root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("changes")
            .join("m2.yaml");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target.with_file_name("m2.yaml.tmp"), "stale leftover").unwrap();

        replace_file(root, &target, b"fresh content").unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "fresh content");
    }

    #[test]
    fn remove_file_no_follow_removes_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let target = root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("axes")
            .join("unused.yml");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, "id: unused\nlabel: unused\n").unwrap();

        remove_file_no_follow(root, &target).unwrap();

        assert!(!target.exists());
    }

    #[test]
    fn remove_file_no_follow_treats_a_missing_file_as_success() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let target = root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("axes")
            .join("missing.yml");

        assert!(remove_file_no_follow(root, &target).is_ok());
    }

    #[test]
    fn remove_dir_all_no_follow_removes_an_existing_directory_and_its_contents() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let target = root.join(".markharness-cache");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("main.json"), "{}").unwrap();

        remove_dir_all_no_follow(root, &target).unwrap();

        assert!(!target.exists());
    }

    #[test]
    fn remove_dir_all_no_follow_treats_a_missing_directory_as_success() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let target = root.join(".markharness-cache");

        assert!(remove_dir_all_no_follow(root, &target).is_ok());
    }

    #[test]
    fn remove_dir_all_no_follow_rejects_a_target_that_is_itself_a_symlinked_directory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("keep.txt"), "keep me").unwrap();
        let target = root.join(".markharness-cache");
        link_dir(&target, outside.path());

        let result = remove_dir_all_no_follow(root, &target);

        assert!(result.is_err(), "expected an error, got: {result:?}");
        assert!(outside.path().join("keep.txt").exists());
    }
}
