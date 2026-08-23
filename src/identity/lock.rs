use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

fn lock_path(root: &Path) -> PathBuf {
    root.join(crate::project_root::MARKHARNESS_DIR)
        .join(".identity.lock")
}

/// An exclusively held identity-operation lock (design doc §6): backed by
/// the OS's own advisory file lock (`std::fs::File::try_lock`, stable
/// since Rust 1.89 — `flock` on Unix, `LockFileEx` on Windows), not by the
/// mere presence or absence of a file.
///
/// An earlier design used a plain file's existence as the lock — created
/// via an atomic create-if-absent, cleared by a startup scan that read the
/// dead owner's PID out of the file and decided it was safe to remove.
/// That "is this lock file merely a leftover from a crashed process"
/// question turned out to have no fully race-free answer reachable with
/// portable, path-based filesystem primitives alone: deciding a lock is
/// stale and then acting on that decision are two separate steps, and no
/// matter how tightly the gap between them is narrowed, a *different*
/// process can in principle replace the exact stale lock being cleared
/// with a fresh, genuinely live one in between — after which removing it
/// would clear a live lock out from under a running operation.
///
/// An OS advisory lock sidesteps the question entirely: the OS itself
/// releases it automatically when the holding process exits for *any*
/// reason, including a crash or `kill -9`, as part of tearing down that
/// process's open file descriptions. So there is no "is this a leftover
/// from a dead process" heuristic to get right — a fresh `acquire` right
/// after a crash simply succeeds immediately, because the OS has already
/// released the lock by the time anything else could ask. No PID-liveness
/// check, no staleness window, no TOCTOU race between deciding and acting.
///
/// The lock *file* itself, once created, is never deleted by this
/// module — only ever locked and unlocked — specifically so that every
/// `acquire` call this codebase's own code makes resolves to the same
/// underlying file (and therefore contends for the same OS lock):
/// deleting and recreating it here would let a concurrent opener resolve
/// to a different file and never truly contend. That is a guarantee this
/// module upholds about *its own* behavior, not a guarantee that holds
/// against every possible concurrent writer to the filesystem: a
/// sufficiently privileged adversarial process could still delete and
/// recreate `.identity.lock` (or an ancestor, most concretely
/// `.markharness/` itself) as an ordinary, non-symlink file/directory
/// between two different processes' `acquire` calls, and each call would
/// resolve internally consistently while still ending up locking two
/// different underlying files that happen to share a path — a
/// split-brain. `fs_safety::open_lock_file_no_follow`'s own doc comment
/// spells out exactly which variant of that ancestor race is closed
/// (symlink/junction substitution, atomically on Unix via `openat`) and
/// which is accepted as a residual, deliberately out-of-scope risk
/// (delete-and-recreate as an ordinary directory) — see it for the
/// precise boundary rather than assuming this module closes every case.
#[derive(Debug)]
pub struct IdentityLock {
    file: File,
}

impl Drop for IdentityLock {
    /// Best-effort safety net, not the primary release path: ordinary
    /// code should still call [`IdentityLock::release`] explicitly so
    /// unlock failures are visible. This exists for the error paths that
    /// can't reach that call — e.g. `run_startup_recovery` propagating a
    /// genuine failure from `recover_incomplete_operations` via `?` before
    /// ever handing the lock to its caller — so a held lock is never
    /// silently leaked for the rest of the process's lifetime. Errors here
    /// can't propagate from `Drop` and are ignored; the OS releases the
    /// underlying advisory lock when this `File`'s own handle closes
    /// regardless of whether `unlock` itself reports success, so even a
    /// failed call here still converges.
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

impl IdentityLock {
    /// Acquires the lock, failing immediately if another operation
    /// currently holds it (design doc: no queuing/blocking — callers
    /// surface this as "an identity operation is already in progress").
    pub fn acquire(root: &Path) -> io::Result<IdentityLock> {
        let path = lock_path(root);
        // `open_lock_file_no_follow` (not a plain `OpenOptions::open`)
        // refuses to follow a symlink/junction placed at this exact path,
        // or anywhere along the ancestor directories leading to it —
        // including creating its parent directory (`.markharness/`)
        // itself only *after* checking for a symlinked ancestor, not
        // before: an ordinary open (or an unconditional
        // `create_dir_all` run ahead of that check) would transparently
        // lock and write to whatever a link points at instead of
        // `.identity.lock` itself, letting something outside `root`
        // silently participate in — or hijack — exclusivity meant to be
        // scoped to this project.
        let file = crate::fs_safety::open_lock_file_no_follow(root, &path)?;
        match file.try_lock() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    format!(
                        "another identity operation is already in progress (lock held at {})",
                        path.display()
                    ),
                ));
            }
            Err(fs::TryLockError::Error(e)) => return Err(e),
        }
        // Diagnostic only — never read back by this module for
        // correctness — so a human inspecting the file while it's locked
        // can see which process currently holds it. Done only now, after
        // the lock is actually held, and its failure is propagated rather
        // than swallowed: a write failing here on a file we supposedly
        // just locked successfully would itself be a signal something is
        // wrong with the lock path, not something safe to paper over.
        file.set_len(0)?;
        write!(&file, "{}", std::process::id())?;
        Ok(IdentityLock { file })
    }

    /// Releases the lock (unlocks it; the underlying file itself stays in
    /// place — see the type's own doc comment for why it must never be
    /// deleted).
    pub fn release(self) -> io::Result<()> {
        self.file.unlock()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquiring_twice_fails_the_second_time() {
        let dir = tempfile::tempdir().unwrap();
        let first = IdentityLock::acquire(dir.path()).unwrap();
        let second = IdentityLock::acquire(dir.path());
        assert!(second.is_err());
        first.release().unwrap();
    }

    #[test]
    fn releasing_allows_reacquiring() {
        let dir = tempfile::tempdir().unwrap();
        let lock = IdentityLock::acquire(dir.path()).unwrap();
        lock.release().unwrap();
        let lock = IdentityLock::acquire(dir.path()).unwrap();
        lock.release().unwrap();
    }

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
        // Directory junctions, unlike file symlinks, need no elevation or
        // Developer Mode on Windows.
        let status = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/j"])
            .arg(link)
            .arg(target)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "mklink /j failed");
    }

    /// Regression test: `acquire` used to call `fs::create_dir_all` on the
    /// lock's parent directory (`.markharness/`) *before* any symlink
    /// check ran — so if `.markharness` itself had already been replaced
    /// with a symlink/junction pointing elsewhere, that unconditional
    /// `create_dir_all` would silently walk through it and create
    /// directories at the link's target, entirely bypassing the later
    /// no-follow check on the lock file itself (which, by then, would
    /// correctly refuse to proceed — but only after the damage of
    /// creating directories through the link was already done).
    /// `open_lock_file_no_follow` now creates that parent directory
    /// itself, strictly after its own ancestor check, so nothing should
    /// ever be created through a pre-existing symlinked ancestor.
    #[test]
    fn acquire_never_creates_anything_through_a_symlinked_markharness_ancestor() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let outside = tempfile::tempdir().unwrap();
        link_dir(
            &root.join(crate::project_root::MARKHARNESS_DIR),
            outside.path(),
        );

        let result = IdentityLock::acquire(root);

        assert!(
            result.is_err(),
            "expected acquire to refuse a symlinked .markharness ancestor"
        );
        assert_eq!(
            fs::read_dir(outside.path()).unwrap().count(),
            0,
            "must never have created anything through the symlinked ancestor"
        );
    }

    #[cfg(unix)]
    fn link_file(link: &Path, target: &Path) -> bool {
        if let Some(parent) = link.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::os::unix::fs::symlink(target, link).is_ok()
    }

    #[cfg(windows)]
    fn link_file(link: &Path, target: &Path) -> bool {
        if let Some(parent) = link.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        // Creating a file symlink (unlike the directory junctions this
        // codebase's other symlink-safety tests use) requires either an
        // elevated process or Developer Mode on Windows. Report failure
        // rather than panicking so this test can skip cleanly on a
        // restricted machine instead of failing for an unrelated reason.
        std::os::windows::fs::symlink_file(target, link).is_ok()
    }

    /// Regression test: `.identity.lock` replaced with a symlink/junction
    /// pointing elsewhere must never be followed — `acquire` must refuse
    /// it outright, and the link's target must never be touched (created,
    /// locked, or written to).
    #[test]
    fn acquire_refuses_a_lock_path_that_is_a_symlink_and_never_touches_its_target() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("elsewhere.lock");
        let link = root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join(".identity.lock");

        if !link_file(&link, &target) {
            eprintln!(
                "skipping acquire_refuses_a_lock_path_that_is_a_symlink_and_never_touches_its_target: \
                 this platform/user refused to create a file symlink (needs elevation or Developer Mode on Windows)"
            );
            return;
        }

        let result = IdentityLock::acquire(root);
        assert!(
            result.is_err(),
            "expected a symlinked lock path to be refused"
        );
        assert!(
            !target.exists(),
            "must never have created the symlink's target"
        );
    }

    /// Same rejection, exercised through a path this test can create on
    /// every platform without needing any special privilege: a plain
    /// directory occupying the lock path (which a real symlink/junction
    /// pointing at a directory would also present as, from `acquire`'s
    /// point of view, since it never gets far enough to distinguish the
    /// two once opened with the reparse-point/`O_NOFOLLOW` flag).
    #[test]
    fn acquire_refuses_a_lock_path_that_is_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let lock_path = root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join(".identity.lock");
        std::fs::create_dir_all(&lock_path).unwrap();

        let result = IdentityLock::acquire(root);

        assert!(
            result.is_err(),
            "expected a directory at the lock path to be refused"
        );
    }

    /// A leftover `.identity.lock` *file* from a previous, now-dead
    /// process (simulated here by writing one directly, without ever
    /// locking it — exactly what remains on disk after a crash, since the
    /// OS lock itself dies with the process but the file it was created in
    /// does not) must never block a fresh acquire: nothing at the OS level
    /// still holds a lock on it, regardless of what content it contains or
    /// how implausible that content is.
    #[test]
    fn a_leftover_unlocked_file_from_a_crashed_process_never_blocks_a_fresh_acquire() {
        let dir = tempfile::tempdir().unwrap();
        let lock_dir = dir.path().join(".markharness");
        std::fs::create_dir_all(&lock_dir).unwrap();
        std::fs::write(lock_dir.join(".identity.lock"), "999999999").unwrap();

        let lock = IdentityLock::acquire(dir.path()).unwrap();
        lock.release().unwrap();
    }

    /// Regression test for the TOCTOU race the earlier PID-heuristic
    /// design had (see this module's own doc comment): true mutual
    /// exclusion, verified the standard way — a shared counter that must
    /// never be seen above 1, incremented right after acquiring and
    /// decremented right before releasing, contended by real OS threads
    /// over a bounded wall-clock window. (All these threads share one OS
    /// process, so `std::process::id()` can't distinguish which of them
    /// currently holds the lock — hence the counter, rather than checking
    /// the diagnostic PID this module writes into the file.)
    #[test]
    fn only_one_thread_ever_holds_the_lock_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Wall-clock-bounded rather than iteration-count-bounded, so this
        // stays fast and predictable across machines regardless of how
        // many attempts that budget buys.
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
        const THREADS: usize = 4;
        let holders = std::sync::atomic::AtomicUsize::new(0);

        std::thread::scope(|scope| {
            for _ in 0..THREADS {
                let holders = &holders;
                scope.spawn(move || {
                    while std::time::Instant::now() < deadline {
                        let lock = loop {
                            match IdentityLock::acquire(root) {
                                Ok(lock) => break lock,
                                Err(_) => continue,
                            }
                        };
                        let now_holding =
                            holders.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                        assert_eq!(
                            now_holding, 1,
                            "more than one thread held the lock at the same time"
                        );
                        holders.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                        lock.release().unwrap();
                    }
                });
            }
        });
    }
}
