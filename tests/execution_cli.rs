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

fn write_generated_testcase(
    root: &Path,
    condition_id: &str,
    case_id: &str,
    case_uid: Option<&str>,
) {
    let dir = root.join(".markharness/generated/testcases");
    std::fs::create_dir_all(&dir).unwrap();
    let uid_line = case_uid
        .map(|uid| format!("case_uid: {uid}\n"))
        .unwrap_or_default();
    std::fs::write(
        dir.join(format!("{condition_id}.yml")),
        format!("case_id: {case_id}\n{uid_line}case_revision: rev-1\n"),
    )
    .unwrap();
}

#[test]
fn execution_record_exits_two_when_case_id_does_not_exist() {
    let dir = init_project();

    let output = run(&[
        "execution",
        "record",
        "tc-does-not-exist-001",
        "--target-revision",
        "abc123",
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
    assert!(!dir.path().join(".markharness/executions/records").exists());
}

/// ADR 0017 §3: a Scenario with no `case_uid` (not migrated) has no
/// identity to record execution evidence against.
#[test]
fn execution_record_exits_two_when_the_case_has_no_case_uid() {
    let dir = init_project();
    write_generated_testcase(dir.path(), "ground", "tc-ground-001", None);

    let output = run(&[
        "execution",
        "record",
        "tc-ground-001",
        "--target-revision",
        "abc123",
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
        stderr.contains("no case_uid yet"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn execution_record_exits_two_when_target_revision_is_blank() {
    let dir = init_project();
    write_generated_testcase(dir.path(), "ground", "tc-ground-001", Some("case-uid-1"));

    let output = run(&[
        "execution",
        "record",
        "tc-ground-001",
        "--target-revision",
        "   ",
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
        stderr.contains("--target-revision must not be empty"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn execution_record_writes_one_file_per_execution() {
    let dir = init_project();
    write_generated_testcase(dir.path(), "ground", "tc-ground-001", Some("case-uid-1"));

    let output = run(&[
        "execution",
        "record",
        "tc-ground-001",
        "--target-revision",
        "abc123",
        "--environment",
        "staging",
        "--result",
        "pass",
        "--executor",
        "yamada",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let records_dir = dir.path().join(".markharness/executions/records");
    let entries: Vec<_> = std::fs::read_dir(&records_dir).unwrap().collect();
    assert_eq!(entries.len(), 1);
    let content = std::fs::read_to_string(entries[0].as_ref().unwrap().path()).unwrap();
    assert!(content.contains("case_uid: case-uid-1"));
    assert!(content.contains("target_revision: abc123"));
    assert!(content.contains("environment: staging"));
}
