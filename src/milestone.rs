use std::fs;
use std::io;
use std::path::Path;

use crate::fs_safety::ensure_no_symlink_ancestor;
use crate::git;

#[derive(Debug, PartialEq, Eq)]
pub enum MilestoneInitOutcome {
    Created,
    AlreadyInitialized,
}

#[derive(Debug)]
pub enum MilestoneInitError {
    TagNotFound,
    Io(io::Error),
}

impl From<io::Error> for MilestoneInitError {
    fn from(e: io::Error) -> Self {
        MilestoneInitError::Io(e)
    }
}

pub fn milestone_init(root: &Path, tag: &str) -> Result<MilestoneInitOutcome, MilestoneInitError> {
    if !git::tag_exists(root, tag)? {
        return Err(MilestoneInitError::TagNotFound);
    }

    let milestone_dir = root.join("executions").join(tag);
    let milestone_path = milestone_dir.join("milestone.yml");
    if milestone_path.is_file() {
        return Ok(MilestoneInitOutcome::AlreadyInitialized);
    }

    ensure_no_symlink_ancestor(root, &milestone_path)?;
    fs::create_dir_all(&milestone_dir)?;
    fs::write(&milestone_path, format!("id: {tag}\n"))?;
    Ok(MilestoneInitOutcome::Created)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn run_git(root: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    fn init_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        run_git(dir.path(), &["init", "-q"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test"]);
        dir
    }

    fn commit_all(root: &Path, message: &str) {
        run_git(root, &["add", "-A"]);
        run_git(root, &["commit", "-q", "-m", message]);
    }

    #[cfg(unix)]
    fn link_dir(link: &Path, target: &Path) {
        std::os::unix::fs::symlink(target, link).unwrap();
    }

    #[cfg(windows)]
    fn link_dir(link: &Path, target: &Path) {
        let status = Command::new("cmd")
            .args(["/c", "mklink", "/j"])
            .arg(link)
            .arg(target)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "mklink /j failed");
    }

    #[test]
    fn milestone_init_refuses_to_follow_a_symlinked_executions_dir() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "init");
        run_git(dir.path(), &["tag", "m1"]);
        let outside = tempfile::tempdir().unwrap();
        link_dir(&dir.path().join("executions"), outside.path());

        let result = milestone_init(dir.path(), "m1");

        assert!(matches!(result, Err(MilestoneInitError::Io(_))));
        assert!(!outside.path().join("m1").exists());
    }

    #[test]
    fn milestone_init_errors_when_tag_does_not_exist() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "init");

        let result = milestone_init(dir.path(), "m1");

        assert!(matches!(result, Err(MilestoneInitError::TagNotFound)));
    }

    #[test]
    fn milestone_init_writes_milestone_yml_when_tag_exists() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "init");
        run_git(dir.path(), &["tag", "m1"]);

        let result = milestone_init(dir.path(), "m1");

        assert_eq!(result.unwrap(), MilestoneInitOutcome::Created);
        let written = fs::read_to_string(dir.path().join("executions/m1/milestone.yml")).unwrap();
        assert_eq!(written, "id: m1\n");
    }

    #[test]
    fn milestone_init_is_idempotent_and_leaves_existing_file_untouched() {
        let dir = init_repo();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        commit_all(dir.path(), "init");
        run_git(dir.path(), &["tag", "m1"]);
        let milestone_path = dir.path().join("executions/m1/milestone.yml");
        fs::create_dir_all(milestone_path.parent().unwrap()).unwrap();
        fs::write(&milestone_path, "id: m1\nlabel: hand-edited\n").unwrap();

        let result = milestone_init(dir.path(), "m1");

        assert_eq!(result.unwrap(), MilestoneInitOutcome::AlreadyInitialized);
        let content = fs::read_to_string(&milestone_path).unwrap();
        assert_eq!(content, "id: m1\nlabel: hand-edited\n");
    }
}
