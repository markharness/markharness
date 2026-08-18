// Integration test fixtures write directly to a scratch repo before
// invoking the CLI binary; that's outside fs_safety's managed-root scope
// (see clippy.toml / src/lib.rs).
#![allow(clippy::disallowed_methods)]

use std::path::Path;
use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_markharness")
}

fn run(args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .output()
        .expect("failed to run markharness binary")
}

fn run_git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

fn write_feature(root: &Path, label: &str) {
    let dir = root.join(".markharness/knowledge/controls/player-jump");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        root.join(".markharness/knowledge/controls/requirement.yml"),
        "id: controls\nlabel: controls\naxis: []\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("feature.yml"),
        format!("id: player-jump\nrequirement: controls\nlabel: {label}\naxis: []\n"),
    )
    .unwrap();
}

fn commit_all(root: &Path, message: &str) {
    run_git(root, &["add", "-A"]);
    run_git(root, &["commit", "-q", "-m", message]);
}

#[test]
fn changes_lineage_reports_linear_when_only_one_branch_changed_the_feature() {
    let dir = tempfile::tempdir().unwrap();
    let init_output = run(&["init", "--dir", dir.path().to_str().unwrap()]);
    assert!(init_output.status.success());
    run_git(dir.path(), &["init", "-q", "-b", "main"]);
    run_git(dir.path(), &["config", "user.email", "test@example.com"]);
    run_git(dir.path(), &["config", "user.name", "Test"]);

    write_feature(dir.path(), "base");
    commit_all(dir.path(), "base");
    run_git(dir.path(), &["branch", "feature"]);

    write_feature(dir.path(), "changed-on-main");
    commit_all(dir.path(), "on main");

    run_git(dir.path(), &["checkout", "-q", "feature"]);
    std::fs::write(dir.path().join("unrelated.txt"), "x\n").unwrap();
    commit_all(dir.path(), "unrelated on feature");

    run_git(dir.path(), &["checkout", "-q", "main"]);
    run_git(
        dir.path(),
        &["merge", "--no-ff", "-q", "-m", "merge feature", "feature"],
    );

    let output = run(&[
        "changes",
        "lineage",
        "--commit",
        "HEAD",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("player-jump: linear"),
        "unexpected stdout: {stdout}"
    );
}

#[test]
fn changes_lineage_exits_two_when_commit_is_not_a_merge_commit() {
    let dir = tempfile::tempdir().unwrap();
    let init_output = run(&["init", "--dir", dir.path().to_str().unwrap()]);
    assert!(init_output.status.success());
    run_git(dir.path(), &["init", "-q", "-b", "main"]);
    run_git(dir.path(), &["config", "user.email", "test@example.com"]);
    run_git(dir.path(), &["config", "user.name", "Test"]);
    write_feature(dir.path(), "v1");
    commit_all(dir.path(), "v1");

    let output = run(&[
        "changes",
        "lineage",
        "--commit",
        "HEAD",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not a merge commit"),
        "unexpected stderr: {stderr}"
    );
}
