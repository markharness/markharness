//! `markharness traceability show --uid <UID>` — the content of one element
//! picked from the `traceability` tree, for a detail pane such as
//! markharness-gui's (ADR 0040, docs/design/cli-read-model-design.md §5.5).
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
const EXTERNAL_REQUIREMENT_UID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";
const FEATURE_UID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FA0";
const BEHAVIOR_UID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FA1";
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
            "id: controls\nsource: native\nlabel: controls\naxis: [gameplay]\ndescription: |\n  The player can control the character.\nuid: {REQUIREMENT_UID}\n"
        ),
    );
    write(
        &root.join(".markharness/knowledge/requirements/timing/requirement.yml"),
        &format!(
            "id: timing\nsource: external\nsource_locator: docs/requirements.sdoc\nsource_revision: 0123456789abcdef0123456789abcdef01234567\nsource_key: REQ-Timing-01\naxis: [gameplay]\nuid: {EXTERNAL_REQUIREMENT_UID}\n"
        ),
    );
    write(
        &root.join(".markharness/knowledge/features/player-jump/feature.yml"),
        &format!(
            "id: player-jump\nrequirement_uids: [{REQUIREMENT_UID}]\nlabel: player-jump\naxis: [gameplay]\ndescription: |\n  The player jumps.\nuid: {FEATURE_UID}\n"
        ),
    );
    write(
        &root.join(".markharness/knowledge/features/player-jump/jump/behavior.yml"),
        &format!(
            "id: jump\nfeature: player-jump\nlabel: jump\nuid: {BEHAVIOR_UID}\naxis: [gameplay]\ndescription: |\n  Jumping.\nprocedures:\n  start-game:\n    steps:\n      - \"Launches the game.\"\n      - \"Loads the stage.\"\n"
        ),
    );
    write(
        &root.join(".markharness/knowledge/features/player-jump/jump/ground/scenario.yml"),
        &format!(
            "id: ground\nbehavior: jump\nlabel: ground\nuid: {SCENARIO_UID}\ndescription: |\n  From the ground.\nimplementation_note: Uses the physics tick.\nphases:\n  - steps:\n      - use: start-game\n      - action: \"Presses jump.\"\n    results:\n      - \"Rises.\"\n"
        ),
    );
    commit(root, "chore: initial knowledge");
    dir
}

fn show(root: &Path, uid: &str, extra: &[&str]) -> Output {
    let mut args = vec![
        "traceability",
        "show",
        "--uid",
        uid,
        "--dir",
        root.to_str().unwrap(),
    ];
    args.extend_from_slice(extra);
    run(&args)
}

fn show_json(root: &Path, uid: &str, extra: &[&str]) -> serde_json::Value {
    let output = show(root, uid, extra);
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("traceability show must emit JSON")
}

fn case_uid(root: &Path) -> String {
    let output = run(&["traceability", "--dir", root.to_str().unwrap()]);
    assert!(output.status.success(), "{output:?}");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    value["test_cases"][0]["case_uid"]
        .as_str()
        .expect("the migrated Scenario has a case_uid")
        .to_string()
}

#[test]
fn show_reports_the_envelope_and_a_native_requirement() {
    let dir = project();
    let value = show_json(dir.path(), REQUIREMENT_UID, &["--at", "HEAD"]);

    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["record_kind"], "traceability_detail");
    assert_eq!(value["at"], "HEAD");
    assert_eq!(value["kind"], "requirement");
    assert_eq!(value["uid"], REQUIREMENT_UID);
    assert_eq!(value["requirement_id"], "controls");
    assert_eq!(value["axis"][0], "gameplay");
    assert_eq!(
        value["description"].as_str().unwrap().trim(),
        "The player can control the character."
    );
}

#[test]
fn show_omits_description_for_an_external_requirement() {
    // The external document owns an external Requirement's content
    // (ADR 0023), so there is no description to show.
    let dir = project();
    let value = show_json(dir.path(), EXTERNAL_REQUIREMENT_UID, &[]);

    assert_eq!(value["kind"], "requirement");
    assert_eq!(value["requirement_id"], "timing");
    assert!(
        value.get("description").is_none(),
        "description must be omitted, not null: {value}"
    );
}

#[test]
fn show_reports_a_feature() {
    let dir = project();
    let value = show_json(dir.path(), FEATURE_UID, &[]);

    assert_eq!(value["kind"], "feature");
    assert_eq!(value["feature_id"], "player-jump");
    assert_eq!(value["axis"][0], "gameplay");
    assert_eq!(
        value["description"].as_str().unwrap().trim(),
        "The player jumps."
    );
}

#[test]
fn show_reports_a_behavior_with_its_procedures() {
    let dir = project();
    let value = show_json(dir.path(), BEHAVIOR_UID, &[]);

    assert_eq!(value["kind"], "behavior");
    assert_eq!(value["behavior_id"], "jump");
    assert_eq!(value["axis"][0], "gameplay");
    assert_eq!(value["description"].as_str().unwrap().trim(), "Jumping.");
    assert_eq!(
        value["procedures"]["start-game"]["steps"],
        serde_json::json!(["Launches the game.", "Loads the stage."])
    );
}

#[test]
fn show_reports_a_scenario_as_written_without_expanding_use() {
    let dir = project();
    let value = show_json(dir.path(), SCENARIO_UID, &[]);

    assert_eq!(value["kind"], "scenario");
    assert_eq!(value["scenario_id"], "ground");
    assert_eq!(
        value["description"].as_str().unwrap().trim(),
        "From the ground."
    );
    assert_eq!(value["implementation_note"], "Uses the physics tick.");
    assert_eq!(
        value["phases"],
        serde_json::json!([{
            "steps": [{"use": "start-game"}, {"action": "Presses jump."}],
            "results": ["Rises."]
        }])
    );
}

#[test]
fn show_omits_implementation_note_when_the_scenario_has_none() {
    let dir = project();
    write(
        &dir.path()
            .join(".markharness/knowledge/features/player-jump/jump/ground/scenario.yml"),
        &format!(
            "id: ground\nbehavior: jump\nlabel: ground\nuid: {SCENARIO_UID}\ndescription: |\n  From the ground.\nphases:\n  - steps:\n      - action: \"Presses jump.\"\n    results:\n      - \"Rises.\"\n"
        ),
    );

    let value = show_json(dir.path(), SCENARIO_UID, &[]);

    assert!(
        value.get("implementation_note").is_none(),
        "implementation_note must be omitted, not null: {value}"
    );
}

#[test]
fn show_reports_a_test_case_with_use_expanded() {
    let dir = project();
    let uid = case_uid(dir.path());
    let value = show_json(dir.path(), &uid, &[]);

    assert_eq!(value["kind"], "test_case");
    assert_eq!(value["uid"], uid);
    assert_eq!(value["case_id"], "tc-player-jump-jump-ground");
    assert!(value["case_revision"].is_string());
    assert_eq!(
        value["phases"],
        serde_json::json!([{
            "steps": ["Launches the game.", "Loads the stage.", "Presses jump."],
            "results": ["Rises."]
        }])
    );
}

#[test]
fn show_reads_the_working_tree_when_at_is_omitted_and_the_ref_when_given() {
    let dir = project();
    write(
        &dir.path()
            .join(".markharness/knowledge/features/player-jump/feature.yml"),
        &format!(
            "id: player-jump\nrequirement_uids: [{REQUIREMENT_UID}]\nlabel: player-jump\naxis: [gameplay]\ndescription: |\n  Edited, not committed.\nuid: {FEATURE_UID}\n"
        ),
    );

    let working = show_json(dir.path(), FEATURE_UID, &[]);
    assert_eq!(working["at"], "working-tree");
    assert_eq!(
        working["description"].as_str().unwrap().trim(),
        "Edited, not committed."
    );

    let head = show_json(dir.path(), FEATURE_UID, &["--at", "HEAD"]);
    assert_eq!(
        head["description"].as_str().unwrap().trim(),
        "The player jumps."
    );
}

#[test]
fn show_fails_without_stdout_for_a_uid_that_does_not_exist() {
    let dir = project();
    let output = show(dir.path(), "01ARZ3NDEKTSV4RRFFQ69G5ZZZ", &[]);

    assert!(!output.status.success());
    assert!(
        output.stdout.is_empty(),
        "stdout must stay empty: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("01ARZ3NDEKTSV4RRFFQ69G5ZZZ") && stderr.contains("working-tree"),
        "the error must name the uid and the point it was looked up at: {stderr}"
    );
}

#[test]
fn traceability_without_show_still_reports_the_tree() {
    let dir = project();
    let output = run(&["traceability", "--dir", dir.path().to_str().unwrap()]);

    assert!(output.status.success(), "{output:?}");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["record_kind"], "traceability");
}
