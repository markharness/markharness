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
    let base = root.join(".markharness/knowledge/features/player-jump/jump/ground");
    std::fs::create_dir_all(&base).unwrap();
    std::fs::create_dir_all(root.join(".markharness/knowledge/requirements/controls")).unwrap();
    std::fs::write(
        root.join(".markharness/axes/gameplay.yml"),
        "id: gameplay\nlabel: Gameplay\n",
    )
    .unwrap();
    std::fs::write(
        root.join(".markharness/knowledge/requirements/controls/requirement.yml"),
        "id: controls\nsource: native\nlabel: controls\naxis: [gameplay]\n",
    )
    .unwrap();
    std::fs::write(
        root.join(".markharness/knowledge/features/player-jump/feature.yml"),
        "id: player-jump\nrequirement_uids: [controls]\nlabel: player-jump\naxis: [gameplay]\n",
    )
    .unwrap();
    std::fs::write(
        root.join(".markharness/knowledge/features/player-jump/jump/behavior.yml"),
        "id: jump\nfeature: player-jump\nlabel: jump\naxis: [gameplay]\ndescription: |\n  Player presses jump.\nprocedures: {}\n",
    )
    .unwrap();
    std::fs::write(
        base.join("scenario.yml"),
        "id: ground\nbehavior: jump\nlabel: ground\ndescription: |\n  Jump from the ground.\nphases:\n  - steps:\n      - action: \"Do it.\"\n    results:\n      - \"lands safely\"\n",
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
fn validate_accepts_a_native_requirement_with_related_issues() {
    let dir = tempfile::tempdir().unwrap();
    let init_output = run(&["init", "--dir", dir.path().to_str().unwrap()]);
    assert!(init_output.status.success());
    write_valid_tree(dir.path());
    std::fs::write(
        dir.path().join(".markharness/knowledge/requirements/controls/requirement.yml"),
        "id: controls\nsource: native\nlabel: controls\naxis: [gameplay]\nrelated_issues: [JIRA-123, JIRA-456]\n",
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

fn assert_validate_passes(dir: &Path) {
    let output = run(&["validate", "--dir", dir.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn reconcile(dir: &Path, intent: &str) {
    let intent_path = dir.join("intent.yml");
    std::fs::write(&intent_path, intent).unwrap();
    let output = run(&[
        "knowledge",
        "reconcile",
        intent_path.to_str().unwrap(),
        "--dir",
        dir.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn feature_intent(requirements: &str, contributes_to: &str) -> String {
    format!(
        "format: markharness/knowledge-intent/v1
mode: merge

{requirements}features:
  - key: feature
    id: player-jump
    label: player-jump
    axis: []
    description: Jump.
{contributes_to}    behaviors:
      - id: jump
        label: jump
        axis: []
        description: Presses jump.
        scenarios:
          - id: ground
            label: ground
            description: Jump from the ground.
            phases:
              - steps:
                  - action: Do it.
                results:
                  - Lands safely.
"
    )
}

/// A Feature is related to Requirements by `contributes_to`, but the
/// relation is optional (zero or more): a Feature may exist before any
/// Requirement is linked.
#[test]
fn validate_accepts_a_feature_without_requirement() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        run(&["init", "--dir", dir.path().to_str().unwrap()])
            .status
            .success()
    );
    write_valid_tree(dir.path());
    std::fs::write(
        dir.path()
            .join(".markharness/knowledge/features/player-jump/feature.yml"),
        "id: player-jump\nrequirement_uids: []\nlabel: player-jump\naxis: [gameplay]\n",
    )
    .unwrap();

    assert_validate_passes(dir.path());
}

#[test]
fn validate_accepts_a_feature_reconcile_created_without_a_requirement() {
    for contributes_to in ["", "    contributes_to: []\n"] {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            run(&["init", "--dir", dir.path().to_str().unwrap()])
                .status
                .success()
        );

        reconcile(dir.path(), &feature_intent("", contributes_to));

        assert_validate_passes(dir.path());
    }
}

#[test]
fn validate_accepts_a_feature_left_without_a_requirement_by_knowledge_remove() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        run(&["init", "--dir", dir.path().to_str().unwrap()])
            .status
            .success()
    );
    reconcile(
        dir.path(),
        &feature_intent(
            "requirements:
  - key: req
    id: controls
    source: native
    label: controls
    axis: []

",
            "    contributes_to: [req]\n",
        ),
    );
    assert_validate_passes(dir.path());

    let removed = run(&[
        "knowledge",
        "remove",
        "requirement",
        "controls",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);
    assert!(removed.status.success());

    assert_validate_passes(dir.path());
}

#[test]
fn validate_rejects_a_requirement_with_a_non_string_related_issues_item() {
    let dir = tempfile::tempdir().unwrap();
    let init_output = run(&["init", "--dir", dir.path().to_str().unwrap()]);
    assert!(init_output.status.success());
    write_valid_tree(dir.path());
    std::fs::write(
        dir.path()
            .join(".markharness/knowledge/requirements/controls/requirement.yml"),
        "id: controls\nsource: native\nlabel: controls\naxis: [gameplay]\nrelated_issues: [123]\n",
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
            .join(".markharness/knowledge/features/player-jump/jump/ground/scenario.yml"),
        "id: ground\nbehavior: jump\nlabel: ground\ndescription: |\n  Jump from the ground.\nphases:\n  - steps:\n      - action: \"Do it.\"\n    results:\n      - \"lands safely\"\ngenerated_by: llm\nverified_by:\n  human_review: true\n",
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
            .join(".markharness/knowledge/features/player-jump/jump/ground/scenario.yml"),
        "id: ground\nbehavior: jump\nlabel: ground\ndescription: |\n  Jump from the ground.\nphases:\n  - steps:\n      - action: \"Do it.\"\n    results:\n      - \"lands safely\"\ngenerated_by: made-up\n",
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
            .join(".markharness/knowledge/features/player-jump/jump/ground/scenario.yml"),
        "id: ground\nbehavior: jump\nlabel: ground\ndescription: |\n  Jump from the ground.\nphases:\n  - steps:\n      - action: \"Do it.\"\n    results:\n      - \"lands safely\"\nverified_by: {}\n",
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
            .join(".markharness/knowledge/features/player-jump/feature.yml"),
        "id: player-jump\nrequirement_uids: [controls]\nlabel: player-jump\naxis: [not-registered]\n",
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
