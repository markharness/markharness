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

fn init_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let output = run(&["init", "--dir", dir.path().to_str().unwrap()]);
    assert!(output.status.success());
    dir
}

fn write_generated_testcase(root: &Path, condition_id: &str, case_id: &str) {
    let dir = root.join(".markharness/generated/testcases");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(format!("{condition_id}.yml")),
        format!("case_id: {case_id}\n"),
    )
    .unwrap();
}

fn write_milestone(root: &Path, milestone: &str) {
    let dir = root
        .join(markharness::project_root::MARKHARNESS_DIR)
        .join("executions")
        .join(milestone);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("milestone.yml"), format!("id: {milestone}\n")).unwrap();
}

#[test]
fn execution_record_exits_two_when_milestone_does_not_exist() {
    let dir = init_project();
    write_generated_testcase(dir.path(), "ground", "tc-ground-001");

    let output = run(&[
        "execution",
        "record",
        "tc-ground-001",
        "--milestone",
        "m1",
        "--result",
        "pass",
        "--executor",
        "yamada",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("milestone 'm1' not found"),
        "unexpected stderr: {stderr}"
    );
    assert!(
        !dir.path()
            .join(".markharness/executions/m1/results.yml")
            .exists()
    );
}

#[test]
fn execution_record_exits_two_when_case_id_does_not_exist() {
    let dir = init_project();
    write_milestone(dir.path(), "m1");

    let output = run(&[
        "execution",
        "record",
        "tc-does-not-exist-001",
        "--milestone",
        "m1",
        "--result",
        "pass",
        "--executor",
        "yamada",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("case_id 'tc-does-not-exist-001' not found"),
        "unexpected stderr: {stderr}"
    );
    assert!(
        !dir.path()
            .join(".markharness/executions/m1/results.yml")
            .exists()
    );
}
