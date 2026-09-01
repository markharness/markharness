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

fn write_valid_tree(root: &Path) {
    let base = root.join(".markharness/knowledge/controls/player-jump/jump/ground");
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(
        root.join(".markharness/axes/gameplay.yml"),
        "id: gameplay\nlabel: Gameplay\n",
    )
    .unwrap();
    std::fs::write(
        root.join(".markharness/knowledge/controls/requirement.yml"),
        "id: controls\nlabel: controls\naxis: [gameplay]\n",
    )
    .unwrap();
    std::fs::write(
        root.join(".markharness/knowledge/controls/player-jump/feature.yml"),
        "id: player-jump\nrequirement: controls\nlabel: player-jump\naxis: [gameplay]\n",
    )
    .unwrap();
    std::fs::write(
        root.join(".markharness/knowledge/controls/player-jump/jump/behavior.yml"),
        "id: jump\nfeature: player-jump\nlabel: jump\naxis: [gameplay]\ndescription: |\n  Player presses jump.\npreconditions:\n  - \"Press the jump button.\"\n",
    )
    .unwrap();
    std::fs::write(
        base.join("condition.yml"),
        "id: ground\nbehavior: jump\nlabel: ground\ndescription: |\n  Jump from the ground.\nsteps:\n  - \"Do it.\"\nadditional_preconditions: []\n",
    )
    .unwrap();
    std::fs::create_dir_all(base.join("expected")).unwrap();
    std::fs::write(
        base.join("expected/001.yml"),
        "id: ground-001\ncondition: ground\ndescription: |\n  lands safely\nresults:\n  - \"Confirmed.\"\n",
    )
    .unwrap();
}

#[test]
fn validate_exits_zero_and_reports_ok_for_a_valid_tree() {
    let dir = tempfile::tempdir().unwrap();
    let init_output = run(&["init", "--dir", dir.path().to_str().unwrap()]);
    assert!(init_output.status.success());
    write_valid_tree(dir.path());

    let output = run(&["validate", "--dir", dir.path().to_str().unwrap()]);

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn validate_accepts_a_requirement_with_source_and_related_issues() {
    let dir = tempfile::tempdir().unwrap();
    let init_output = run(&["init", "--dir", dir.path().to_str().unwrap()]);
    assert!(init_output.status.success());
    write_valid_tree(dir.path());
    std::fs::write(
        dir.path().join(".markharness/knowledge/controls/requirement.yml"),
        "id: controls\nlabel: controls\naxis: [gameplay]\nsource: PRD-42\nrelated_issues: [JIRA-123, JIRA-456]\n",
    )
    .unwrap();

    let output = run(&["validate", "--dir", dir.path().to_str().unwrap()]);

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn validate_rejects_a_requirement_with_a_non_string_related_issues_item() {
    let dir = tempfile::tempdir().unwrap();
    let init_output = run(&["init", "--dir", dir.path().to_str().unwrap()]);
    assert!(init_output.status.success());
    write_valid_tree(dir.path());
    std::fs::write(
        dir.path()
            .join(".markharness/knowledge/controls/requirement.yml"),
        "id: controls\nlabel: controls\naxis: [gameplay]\nrelated_issues: [123]\n",
    )
    .unwrap();

    let output = run(&["validate", "--dir", dir.path().to_str().unwrap()]);

    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn validate_accepts_an_expected_result_with_generated_by_and_verified_by() {
    let dir = tempfile::tempdir().unwrap();
    let init_output = run(&["init", "--dir", dir.path().to_str().unwrap()]);
    assert!(init_output.status.success());
    write_valid_tree(dir.path());
    std::fs::write(
        dir.path()
            .join(".markharness/knowledge/controls/player-jump/jump/ground/expected/001.yml"),
        "id: ground-001\ncondition: ground\ndescription: |\n  lands safely\nresults:\n  - \"Confirmed.\"\ngenerated_by: llm\nverified_by:\n  human_review: true\n",
    )
    .unwrap();

    let output = run(&["validate", "--dir", dir.path().to_str().unwrap()]);

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn validate_rejects_an_expected_result_with_an_invalid_generated_by_value() {
    let dir = tempfile::tempdir().unwrap();
    let init_output = run(&["init", "--dir", dir.path().to_str().unwrap()]);
    assert!(init_output.status.success());
    write_valid_tree(dir.path());
    std::fs::write(
        dir.path()
            .join(".markharness/knowledge/controls/player-jump/jump/ground/expected/001.yml"),
        "id: ground-001\ncondition: ground\ndescription: |\n  lands safely\nresults:\n  - \"Confirmed.\"\ngenerated_by: made-up\n",
    )
    .unwrap();

    let output = run(&["validate", "--dir", dir.path().to_str().unwrap()]);

    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn validate_rejects_a_verified_by_without_human_review() {
    let dir = tempfile::tempdir().unwrap();
    let init_output = run(&["init", "--dir", dir.path().to_str().unwrap()]);
    assert!(init_output.status.success());
    write_valid_tree(dir.path());
    std::fs::write(
        dir.path()
            .join(".markharness/knowledge/controls/player-jump/jump/ground/expected/001.yml"),
        "id: ground-001\ncondition: ground\ndescription: |\n  lands safely\nresults:\n  - \"Confirmed.\"\nverified_by: {}\n",
    )
    .unwrap();

    let output = run(&["validate", "--dir", dir.path().to_str().unwrap()]);

    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn validate_exits_one_and_lists_issues_for_an_invalid_feature() {
    let dir = tempfile::tempdir().unwrap();
    let init_output = run(&["init", "--dir", dir.path().to_str().unwrap()]);
    assert!(init_output.status.success());
    write_valid_tree(dir.path());
    std::fs::write(
        dir.path()
            .join(".markharness/knowledge/controls/player-jump/feature.yml"),
        "id: player-jump\nrequirement: controls\nlabel: player-jump\naxis: [not-registered]\n",
    )
    .unwrap();

    let output = run(&["validate", "--dir", dir.path().to_str().unwrap()]);

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("not-registered"),
        "unexpected stdout: {stdout}"
    );
}
