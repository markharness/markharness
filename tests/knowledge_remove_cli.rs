// Integration test fixtures write directly to a scratch repo before
// invoking the CLI binary; that's outside fs_safety's managed-root scope
// (see clippy.toml / src/lib.rs).
#![allow(clippy::disallowed_methods)]

use std::fs;
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

fn write(dir: &std::path::Path, relative: &str, contents: &str) {
    let path = dir.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn setup_root() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join(markharness::project_root::MARKER_FILE);
    fs::create_dir_all(marker.parent().unwrap()).unwrap();
    fs::write(marker, "schema_version = 1\n").unwrap();
    write(
        dir.path(),
        ".markharness/knowledge/requirements/controls/requirement.yml",
        "id: controls\nsource: native\nlabel: controls\naxis: []\n",
    );
    write(
        dir.path(),
        ".markharness/knowledge/features/player-jump/feature.yml",
        "id: player-jump\nrequirement_uids: []\nlabel: player-jump\naxis: []\n",
    );
    write(
        dir.path(),
        ".markharness/knowledge/features/player-jump/jump/behavior.yml",
        "id: jump\nfeature: player-jump\nlabel: jump\naxis: []\ndescription: |\n  d\nprocedures: {}\n",
    );
    write(
        dir.path(),
        ".markharness/knowledge/features/player-jump/jump/basic/scenario.yml",
        "id: basic\nbehavior: jump\nlabel: basic\ndescription: |\n  d\nphases:\n  - steps:\n      - action: \"Do it.\"\n    results:\n      - \"Confirmed.\"\n",
    );
    dir
}

#[test]
fn removes_a_scenario_scoped_by_feature_and_behavior() {
    let dir = setup_root();

    let output = run(&[
        "knowledge",
        "remove",
        "scenario",
        "basic",
        "--feature",
        "player-jump",
        "--behavior",
        "jump",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);

    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !dir.path()
            .join(".markharness/knowledge/features/player-jump/jump/basic/scenario.yml")
            .exists()
    );
    assert!(
        dir.path()
            .join(".markharness/knowledge/features/player-jump/jump/behavior.yml")
            .exists()
    );
}

#[test]
fn removing_a_feature_cascades_and_reports_json() {
    let dir = setup_root();

    let output = run(&[
        "knowledge",
        "remove",
        "feature",
        "player-jump",
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
    ]);

    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["deleted"].as_array().unwrap().len(), 3);
    assert!(
        !dir.path()
            .join(".markharness/knowledge/features/player-jump/feature.yml")
            .exists()
    );
    assert!(
        !dir.path()
            .join(".markharness/knowledge/features/player-jump/jump/behavior.yml")
            .exists()
    );
    assert!(
        !dir.path()
            .join(".markharness/knowledge/features/player-jump/jump/basic/scenario.yml")
            .exists()
    );
}

#[test]
fn removing_an_unknown_key_exits_nonzero_with_an_error() {
    let dir = setup_root();

    let output = run(&[
        "knowledge",
        "remove",
        "requirement",
        "does-not-exist",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("does-not-exist"), "stderr={stderr}");
}

#[test]
fn ambiguous_behavior_slug_is_rejected_and_lists_both_candidates() {
    let dir = setup_root();
    write(
        dir.path(),
        ".markharness/knowledge/features/player-dash/feature.yml",
        "id: player-dash\nrequirement_uids: []\nlabel: player-dash\naxis: []\n",
    );
    write(
        dir.path(),
        ".markharness/knowledge/features/player-dash/jump/behavior.yml",
        "id: jump\nfeature: player-dash\nlabel: jump\naxis: []\ndescription: |\n  d\nprocedures: {}\n",
    );

    let output = run(&[
        "knowledge",
        "remove",
        "behavior",
        "jump",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("player-jump/jump/behavior.yml")
            && stderr.contains("player-dash/jump/behavior.yml"),
        "stderr={stderr}"
    );
    // Nothing should have been deleted when resolution fails.
    assert!(
        dir.path()
            .join(".markharness/knowledge/features/player-jump/jump/behavior.yml")
            .exists()
    );
    assert!(
        dir.path()
            .join(".markharness/knowledge/features/player-dash/jump/behavior.yml")
            .exists()
    );
}

#[test]
fn removing_a_requirement_detaches_it_from_a_referencing_feature() {
    let dir = setup_root();
    // `requirement_uids` names only Requirement UIDs, never display ids
    // (ADR 0027 §5) — a migrated Requirement carries `uid:`, and only that
    // uid can ever appear in a referencing Feature's `requirement_uids`.
    write(
        dir.path(),
        ".markharness/knowledge/requirements/controls/requirement.yml",
        "id: controls\nsource: native\nlabel: controls\naxis: []\nuid: 01ARZ3NDEKTSV4RRFFQ69G5FR0\n",
    );
    write(
        dir.path(),
        ".markharness/knowledge/features/player-jump/feature.yml",
        "id: player-jump\nrequirement_uids: [01ARZ3NDEKTSV4RRFFQ69G5FR0]\nlabel: player-jump\naxis: []\n",
    );

    let output = run(&[
        "knowledge",
        "remove",
        "requirement",
        "controls",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);

    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("detached"), "stdout={stdout}");
    let feature_content = fs::read_to_string(
        dir.path()
            .join(".markharness/knowledge/features/player-jump/feature.yml"),
    )
    .unwrap();
    assert!(feature_content.contains("requirement_uids: []"));
}

/// Regression for a Codex stop-time review finding: `--behavior` given
/// without `--feature` must be rejected outright, never silently ignored.
/// Without this check, an operator confirming a Scenario's parent Behavior
/// via `--behavior` alone (forgetting `--feature`) would have it dropped
/// entirely and `basic` resolved as an unscoped, globally-unique slug —
/// deleting the real "basic" Scenario even though its actual parent
/// Behavior ("jump") does not match the "not-jump" given here.
#[test]
fn scenario_removal_rejects_behavior_flag_without_feature() {
    let dir = setup_root();

    let output = run(&[
        "knowledge",
        "remove",
        "scenario",
        "basic",
        "--behavior",
        "not-jump",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--feature"), "stderr={stderr}");
    assert!(
        dir.path()
            .join(".markharness/knowledge/features/player-jump/jump/basic/scenario.yml")
            .exists(),
        "nothing must be deleted when the combination is rejected"
    );
}
