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
