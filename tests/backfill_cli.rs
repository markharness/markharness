// Integration test fixtures write directly to a scratch repo before
// invoking the CLI binary; that's outside fs_safety's managed-root scope
// (see clippy.toml / src/lib.rs).
#![allow(clippy::disallowed_methods)]

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

#[test]
fn backfill_run_reports_zero_processed_when_no_milestones_exist() {
    let dir = tempfile::tempdir().unwrap();
    let init_output = run(&["init", "--dir", dir.path().to_str().unwrap()]);
    assert!(init_output.status.success());

    let output = run(&["backfill", "run", "--dir", dir.path().to_str().unwrap()]);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("backfill: 0 processed, 0 already up to date"),
        "unexpected stdout: {stdout}"
    );
}

fn run_git(root: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

/// Standards/Spec review of issue #29: `backfill run` must not silently
/// succeed (exit 0) while a pair was skipped for being schema-incompatible
/// — the skip has to be visible in output and in the exit code, or a
/// caller (human or CI) has no way to notice.
#[test]
fn backfill_run_exits_non_zero_and_reports_an_incompatible_pair() {
    let dir = tempfile::tempdir().unwrap();
    let init_output = run(&["init", "--dir", dir.path().to_str().unwrap()]);
    assert!(init_output.status.success());
    run_git(dir.path(), &["init", "-q"]);
    run_git(dir.path(), &["config", "user.email", "test@example.com"]);
    run_git(dir.path(), &["config", "user.name", "Test"]);

    std::fs::write(
        dir.path().join(".markharness/config.toml"),
        "schema_version = 1\n\n[knowledge]\nschema_version = 2\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join(".markharness/executions/m1")).unwrap();
    std::fs::write(
        dir.path().join(".markharness/executions/m1/milestone.yml"),
        "id: m1\n",
    )
    .unwrap();
    run_git(dir.path(), &["add", "-A"]);
    run_git(dir.path(), &["commit", "-q", "-m", "m1"]);
    run_git(dir.path(), &["tag", "m1"]);

    std::fs::write(
        dir.path().join(".markharness/config.toml"),
        "schema_version = 1\n\n[knowledge]\nschema_version = 1\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join(".markharness/executions/m2")).unwrap();
    std::fs::write(
        dir.path().join(".markharness/executions/m2/milestone.yml"),
        "id: m2\n",
    )
    .unwrap();
    run_git(dir.path(), &["add", "-A"]);
    run_git(dir.path(), &["commit", "-q", "-m", "m2"]);
    run_git(dir.path(), &["tag", "m2"]);

    let output = run(&["backfill", "run", "--dir", dir.path().to_str().unwrap()]);

    assert!(
        !output.status.success(),
        "expected a non-zero exit code, stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("m2"),
        "expected the incompatible pair to be reported, stdout: {stdout}"
    );
}

#[test]
fn backfill_run_accepts_no_cache_flag() {
    let dir = tempfile::tempdir().unwrap();
    let init_output = run(&["init", "--dir", dir.path().to_str().unwrap()]);
    assert!(init_output.status.success());

    let output = run(&[
        "backfill",
        "run",
        "--dir",
        dir.path().to_str().unwrap(),
        "--no-cache",
    ]);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
