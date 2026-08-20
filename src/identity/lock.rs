use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::fs_safety::{create_new_no_follow, remove_file_no_follow};

fn lock_path(root: &Path) -> PathBuf {
    root.join(crate::project_root::MARKHARNESS_DIR)
        .join(".identity.lock")
}

/// An exclusively held identity-operation lock (design doc §6, Q6):
/// application-level, not an OS advisory lock. Acquired via
/// `create_new_no_follow` (atomic create-if-absent); a crashed process
/// leaves the file behind, which the startup recovery scan clears after
/// confirming the owning PID is no longer running (`clear_if_stale`)
/// rather than on every command's mere presence check — otherwise a
/// concurrently *running* operation's lock would be wrongly cleared by
/// another command starting at the same time.
pub struct IdentityLock {
    root: PathBuf,
}

impl IdentityLock {
    /// Acquires the lock, failing immediately if another operation
    /// currently holds it (design doc: no queuing/blocking — callers
    /// surface this as "an identity operation is already in progress").
    pub fn acquire(root: &Path) -> io::Result<IdentityLock> {
        let path = lock_path(root);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = create_new_no_follow(&path).map_err(|e| {
            io::Error::new(
                e.kind(),
                format!(
                    "another identity operation is already in progress (lock held at {}): {e}",
                    path.display()
                ),
            )
        })?;
        write!(file, "{}", std::process::id())?;
        Ok(IdentityLock {
            root: root.to_path_buf(),
        })
    }

    /// Releases the lock. Deliberately not done via `Drop`: a crash
    /// (process kill, not a normal Rust unwind) must leave the lock file
    /// behind for recovery to find, and relying on `Drop` would make that
    /// untestable and easy to accidentally "fix" away.
    pub fn release(self) -> io::Result<()> {
        remove_file_no_follow(&self.root, &lock_path(&self.root))
    }
}

/// If a lock file exists at `root` and the PID recorded in it is no
/// longer running, removes it and returns `true`. Returns `false` when no
/// lock file exists, or when it exists but its owner still appears to be
/// running (including when liveness cannot be determined — the safe
/// default is to leave a possibly-live lock alone rather than risk
/// clearing one out from under a running operation).
pub fn clear_if_stale(root: &Path) -> io::Result<bool> {
    let path = lock_path(root);
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };

    let is_stale = match contents.trim().parse::<u32>() {
        Ok(pid) => !pid_is_alive(pid),
        // An unparseable lock file cannot belong to a live, well-formed
        // operation; treat it as stale rather than getting stuck forever.
        Err(_) => true,
    };

    if is_stale {
        remove_file_no_follow(root, &path)?;
    }
    Ok(is_stale)
}

#[cfg(unix)]
fn pid_is_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .map(|status| status.success())
        // Inconclusive (e.g. `kill` missing): assume alive, so a possibly
        // live lock is never wrongly cleared.
        .unwrap_or(true)
}

#[cfg(windows)]
fn pid_is_alive(pid: u32) -> bool {
    std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).contains(&pid.to_string()))
        .unwrap_or(true)
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

    #[test]
    fn clear_if_stale_is_a_no_op_when_no_lock_exists() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!clear_if_stale(dir.path()).unwrap());
    }

    /// A lock file naming the current test process's own PID must never be
    /// cleared — this process is, definitionally, still running.
    #[test]
    fn clear_if_stale_leaves_a_lock_owned_by_a_live_process_alone() {
        let dir = tempfile::tempdir().unwrap();
        let lock = IdentityLock::acquire(dir.path()).unwrap();
        assert!(!clear_if_stale(dir.path()).unwrap());
        lock.release().unwrap();
    }

    /// A PID far beyond any real process table's range (0 is reserved but
    /// still a *real*, always-present PID on both Unix's process-group-0
    /// signaling semantics and Windows's System Idle Process — using it
    /// here would make this test flaky) simulates a crashed owner without
    /// needing to actually kill a process in a unit test.
    #[test]
    fn clear_if_stale_removes_a_lock_owned_by_a_dead_pid() {
        let dir = tempfile::tempdir().unwrap();
        let lock_dir = dir.path().join(".markharness");
        std::fs::create_dir_all(&lock_dir).unwrap();
        std::fs::write(lock_dir.join(".identity.lock"), "999999999").unwrap();

        assert!(clear_if_stale(dir.path()).unwrap());
        assert!(!lock_dir.join(".identity.lock").exists());

        // The lock is now free to acquire.
        let lock = IdentityLock::acquire(dir.path()).unwrap();
        lock.release().unwrap();
    }

    #[test]
    fn clear_if_stale_treats_an_unparseable_lock_file_as_stale() {
        let dir = tempfile::tempdir().unwrap();
        let lock_dir = dir.path().join(".markharness");
        std::fs::create_dir_all(&lock_dir).unwrap();
        std::fs::write(lock_dir.join(".identity.lock"), "not-a-pid").unwrap();

        assert!(clear_if_stale(dir.path()).unwrap());
    }
}
