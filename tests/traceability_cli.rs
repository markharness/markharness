//! `markharness traceability` — a read-only view of Requirement/Feature/
//! Behavior/Scenario/TestCase relations for external tools such as
//! markharness-view (ADR 0032, docs/design/cli-read-model-design.md §5).
//!
//! Fixtures write directly to a scratch repo before invoking the CLI binary;
//! that's outside fs_safety's managed-root scope (see clippy.toml).
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

fn git(root: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("failed to run git")
}

fn commit(root: &Path, message: &str) {
    assert!(git(root, &["add", "-A"]).status.success());
    let output = git(root, &["commit", "-q", "-m", message]);
    assert!(output.status.success(), "{output:?}");
}

fn write(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

const REQUIREMENT_UID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const SCENARIO_UID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FB1";

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let root = dir.path();
    assert!(
        run(&["init", "--dir", root.to_str().unwrap()])
            .status
            .success()
    );
    assert!(git(root, &["init", "-q"]).status.success());
    assert!(
        git(root, &["config", "user.email", "test@example.com"])
            .status
            .success()
    );
    assert!(git(root, &["config", "user.name", "Test"]).status.success());

    write(
        &root.join(".markharness/axes/gameplay.yml"),
        "id: gameplay\nlabel: Gameplay\n",
    );
    write(
        &root.join(".markharness/knowledge/requirements/controls/requirement.yml"),
        &format!(
            "id: controls\nsource: native\nlabel: controls\naxis: [gameplay]\nuid: {REQUIREMENT_UID}\n"
        ),
    );
    write(
        &root.join(".markharness/knowledge/features/player-jump/feature.yml"),
        &format!(
            "id: player-jump\nrequirement_uids: [{REQUIREMENT_UID}]\nlabel: player-jump\naxis: [gameplay]\n"
        ),
    );
    write(
        &root.join(".markharness/knowledge/features/player-jump/jump/behavior.yml"),
        "id: jump\nfeature: player-jump\nlabel: jump\naxis: [gameplay]\ndescription: |\n  Jumping.\nprocedures: {}\n",
    );
    write(
        &root.join(".markharness/knowledge/features/player-jump/jump/ground/scenario.yml"),
        &format!(
            "id: ground\nbehavior: jump\nlabel: ground\nuid: {SCENARIO_UID}\ndescription: |\n  From the ground.\nphases:\n  - steps:\n      - action: \"Presses jump.\"\n    results:\n      - \"Rises.\"\n"
        ),
    );
    commit(root, "chore: initial knowledge");
    dir
}

fn traceability(root: &Path, extra: &[&str]) -> Output {
    let mut args = vec!["traceability", "--dir", root.to_str().unwrap()];
    args.extend_from_slice(extra);
    run(&args)
}

fn traceability_json(root: &Path, extra: &[&str]) -> serde_json::Value {
    let output = traceability(root, extra);
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("traceability must emit JSON")
}

#[test]
fn traceability_reports_the_envelope_and_full_hierarchy_at_head() {
    let dir = project();
    let value = traceability_json(dir.path(), &["--at", "HEAD"]);

    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["record_kind"], "traceability");

    assert_eq!(value["requirements"][0]["requirement_id"], "controls");
    assert_eq!(value["requirements"][0]["requirement_uid"], REQUIREMENT_UID);
    assert_eq!(value["requirements"][0]["source"], "native");

    assert_eq!(value["features"][0]["feature_id"], "player-jump");

    assert_eq!(value["behaviors"][0]["behavior_id"], "jump");
    assert_eq!(value["behaviors"][0]["feature_id"], "player-jump");
    // Known limitation (design doc §5.2): the generated-TestCase path this
    // read model derives Behavior nodes from carries no Behavior UID today,
    // even though `behavior.yml` itself has one.
    assert!(value["behaviors"][0]["behavior_uid"].is_null());

    assert_eq!(value["scenarios"][0]["scenario_id"], "ground");
    assert_eq!(value["scenarios"][0]["scenario_uid"], SCENARIO_UID);
    assert_eq!(value["scenarios"][0]["behavior_id"], "jump");

    assert_eq!(
        value["test_cases"][0]["case_id"],
        "tc-player-jump-jump-ground"
    );
    assert_eq!(
        value["test_cases"][0]["relative_path"],
        "player-jump/jump/ground.yml"
    );
}

#[test]
fn traceability_relates_the_scenario_to_the_requirement_it_contributes_to() {
    let dir = project();
    let value = traceability_json(dir.path(), &["--at", "HEAD"]);

    let relations = value["relations"].as_array().expect("relations array");
    let contributes_to = relations
        .iter()
        .find(|r| r["kind"] == "contributes_to" && r["to_uid"] == REQUIREMENT_UID)
        .unwrap_or_else(|| {
            panic!("expected a contributes_to relation into the Requirement, got {relations:?}")
        });
    assert_eq!(contributes_to["from_uid"], SCENARIO_UID);
}

#[test]
fn traceability_omits_relations_for_entities_without_a_uid() {
    // The Feature in the fixture has no `uid:` (not migrated), so it cannot
    // appear on either side of a relation — a relation without a stable UID
    // would be meaningless to an external reader across re-runs.
    let dir = project();
    let value = traceability_json(dir.path(), &["--at", "HEAD"]);

    assert!(value["features"][0]["feature_uid"].is_null());
    let relations = value["relations"].as_array().expect("relations array");
    assert!(
        relations
            .iter()
            .all(|r| r["from_uid"] != "player-jump" && r["to_uid"] != "player-jump"),
        "a Feature with no uid must not appear in relations: {relations:?}"
    );
}
