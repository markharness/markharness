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
    options.open(path)
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

/// Removes the single file at `target`, refusing to follow a symlink/junction
/// at `target` itself or any of its ancestors. A missing `target` is treated
/// as success (removal is idempotent), mirroring `remove_dir_all_no_follow`.
pub fn remove_file_no_follow(root: &Path, target: &Path) -> io::Result<()> {
    ensure_no_symlink_ancestor(root, target)?;
    match fs::remove_file(target) {
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
    match fs::remove_dir_all(target) {
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
        std::os::unix::fs::symlink(target, link).unwrap();
    }

    #[cfg(windows)]
    fn link_dir(link: &Path, target: &Path) {
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
        let target = root.join("generated").join("testcases").join("ground.yml");

        assert!(ensure_no_symlink_ancestor(root, &target).is_ok());
    }

    #[test]
    fn rejects_a_target_behind_a_symlinked_ancestor() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let outside = tempfile::tempdir().unwrap();
        let link = root.join("generated");
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
        let target = root.join("generated");
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
    fn replace_file_creates_the_file_and_its_parent_dir_when_none_exist() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let target = root.join("executions").join("m1").join("results.yml");

        replace_file(root, &target, b"id: m1\n").unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "id: m1\n");
    }

    #[test]
    fn replace_file_overwrites_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let target = root.join("changes").join("m2.yaml");
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
        link_dir(&root.join("changes"), outside.path());
        let target = root.join("changes").join("m2.yaml");

        let result = replace_file(root, &target, b"payload");

        assert!(result.is_err(), "expected an error, got: {result:?}");
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    }

    #[test]
    fn replace_file_rejects_and_leaves_outside_untouched_when_tmp_path_is_a_symlinked_directory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let outside = tempfile::tempdir().unwrap();
        let target = root.join("changes").join("m2.yaml");
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
        let target = root.join("changes").join("m2.yaml");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target.with_file_name("m2.yaml.tmp"), "stale leftover").unwrap();

        replace_file(root, &target, b"fresh content").unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "fresh content");
    }

    #[test]
    fn remove_file_no_follow_removes_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let target = root.join("axes").join("unused.yml");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, "id: unused\nlabel: unused\n").unwrap();

        remove_file_no_follow(root, &target).unwrap();

        assert!(!target.exists());
    }

    #[test]
    fn remove_file_no_follow_treats_a_missing_file_as_success() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let target = root.join("axes").join("missing.yml");

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
