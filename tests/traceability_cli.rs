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
        &format!(
            "id: jump\nfeature: player-jump\nlabel: jump\nuid: {BEHAVIOR_UID}\naxis: [gameplay]\ndescription: |\n  Jumping.\nprocedures: {{}}\n"
        ),
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
    assert_eq!(value["requirements"][0]["label"], "controls");
    assert!(value["requirements"][0]["source_locator"].is_null());
    assert!(value["requirements"][0]["source_key"].is_null());

    assert_eq!(value["features"][0]["feature_id"], "player-jump");
    assert_eq!(value["features"][0]["label"], "player-jump");

    assert_eq!(value["behaviors"][0]["behavior_id"], "jump");
    assert_eq!(value["behaviors"][0]["behavior_uid"], BEHAVIOR_UID);
    assert_eq!(value["behaviors"][0]["feature_id"], "player-jump");
    assert_eq!(value["behaviors"][0]["label"], "jump");

    assert_eq!(value["scenarios"][0]["scenario_id"], "ground");
    assert_eq!(value["scenarios"][0]["scenario_uid"], SCENARIO_UID);
    assert_eq!(value["scenarios"][0]["behavior_id"], "jump");
    assert_eq!(value["scenarios"][0]["label"], "ground");

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
fn traceability_exposes_source_locator_and_source_key_for_an_external_requirement() {
    // An external Requirement's content lives in a StrictDoc .sdoc file, not
    // in requirement.yml. Knowing source: "external" alone doesn't let a
    // reader reach that content — source_locator/source_key do (ADR 0023,
    // ADR 0030).
    let dir = project();
    write(
        &dir.path()
            .join(".markharness/knowledge/requirements/timing/requirement.yml"),
        "id: timing\nsource: external\nsource_locator: docs/requirements.sdoc\nsource_revision: 0123456789abcdef0123456789abcdef01234567\nsource_key: REQ-Timing-01\naxis: [gameplay]\n",
    );
    commit(dir.path(), "chore: add an external requirement");

    let value = traceability_json(dir.path(), &["--at", "HEAD"]);

    let timing = value["requirements"]
        .as_array()
        .expect("requirements array")
        .iter()
        .find(|r| r["requirement_id"] == "timing")
        .expect("expected the external Requirement to be present");
    assert_eq!(timing["source"], "external");
    assert_eq!(timing["source_locator"], "docs/requirements.sdoc");
    assert_eq!(timing["source_key"], "REQ-Timing-01");
    // markharness never owns external content (ADR 0023), so it never
    // fabricates a representative label for it either.
    assert!(timing["label"].is_null());
}

#[test]
fn traceability_rejects_a_native_requirement_carrying_external_only_fields() {
    // ADR 0023: source_locator/source_key belong exclusively to source:
    // external. If a hand-edited requirement.yml claims source: native but
    // still carries them (validate would normally catch this first),
    // traceability must not silently pass them through — a consumer such as
    // markharness-view would otherwise be misdirected to unrelated external
    // content for what is actually a native Requirement.
    let dir = project();
    write(
        &dir.path()
            .join(".markharness/knowledge/requirements/timing/requirement.yml"),
        "id: timing\nsource: native\nlabel: timing\nsource_locator: docs/requirements.sdoc\nsource_key: REQ-Timing-01\naxis: [gameplay]\n",
    );
    commit(dir.path(), "chore: add a malformed requirement");

    let output = traceability(dir.path(), &["--at", "HEAD"]);

    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("timing") && stderr.contains("native"),
        "expected an error naming the offending Requirement and its mode: {stderr}"
    );
}

#[test]
fn traceability_rejects_a_native_requirement_carrying_only_source_revision() {
    // Regression: an earlier version of this check only looked at
    // source_locator/source_key, so a native Requirement carrying just
    // source_revision (also external-only, ADR 0023) slipped through
    // unrejected even though `validate` would refuse it.
    let dir = project();
    write(
        &dir.path()
            .join(".markharness/knowledge/requirements/timing/requirement.yml"),
        "id: timing\nsource: native\nlabel: timing\nsource_revision: 0123456789abcdef0123456789abcdef01234567\naxis: [gameplay]\n",
    );
    commit(dir.path(), "chore: add a malformed requirement");

    let output = traceability(dir.path(), &["--at", "HEAD"]);

    assert_eq!(output.status.code(), Some(2), "{output:?}");
}

#[test]
fn traceability_rejects_a_native_requirement_without_a_label() {
    let dir = project();
    write(
        &dir.path()
            .join(".markharness/knowledge/requirements/timing/requirement.yml"),
        "id: timing\nsource: native\naxis: [gameplay]\n",
    );
    commit(dir.path(), "chore: add a malformed requirement");

    let output = traceability(dir.path(), &["--at", "HEAD"]);

    assert_eq!(output.status.code(), Some(2), "{output:?}");
}

#[test]
fn traceability_rejects_an_external_requirement_carrying_a_label() {
    let dir = project();
    write(
        &dir.path()
            .join(".markharness/knowledge/requirements/timing/requirement.yml"),
        "id: timing\nsource: external\nlabel: timing\nsource_locator: docs/requirements.sdoc\nsource_revision: 0123456789abcdef0123456789abcdef01234567\nsource_key: REQ-Timing-01\naxis: [gameplay]\n",
    );
    commit(dir.path(), "chore: add a malformed requirement");

    let output = traceability(dir.path(), &["--at", "HEAD"]);

    assert_eq!(output.status.code(), Some(2), "{output:?}");
}

#[test]
fn traceability_rejects_an_external_requirement_missing_source_locator_or_source_key() {
    let dir = project();
    write(
        &dir.path()
            .join(".markharness/knowledge/requirements/timing/requirement.yml"),
        "id: timing\nsource: external\naxis: [gameplay]\n",
    );
    commit(dir.path(), "chore: add a malformed external requirement");

    let output = traceability(dir.path(), &["--at", "HEAD"]);

    assert_eq!(output.status.code(), Some(2), "{output:?}");
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
fn traceability_reads_the_working_tree_when_at_is_omitted() {
    // ADR 0033: unlike impact/coverage, traceability has no two-point or
    // release-auditing requirement, so omitting --at reads uncommitted
    // Knowledge — the same way generate/verify already do — instead of
    // requiring a commit first.
    let dir = project();
    write(
        &dir.path()
            .join(".markharness/knowledge/requirements/timing/requirement.yml"),
        "id: timing\nsource: native\nlabel: timing\naxis: [gameplay]\n",
    );
    // Deliberately not committed.

    let value = traceability_json(dir.path(), &[]);

    assert!(
        value["requirements"]
            .as_array()
            .expect("requirements array")
            .iter()
            .any(|r| r["requirement_id"] == "timing"),
        "expected the uncommitted Requirement to be visible: {value}"
    );
}

#[test]
fn traceability_reports_working_tree_as_the_at_value_when_at_is_omitted() {
    let dir = project();
    let value = traceability_json(dir.path(), &[]);

    assert_eq!(value["at"], "working-tree");
}

#[test]
fn traceability_at_head_does_not_see_uncommitted_changes() {
    let dir = project();
    write(
        &dir.path()
            .join(".markharness/knowledge/requirements/timing/requirement.yml"),
        "id: timing\nsource: native\nlabel: timing\naxis: [gameplay]\n",
    );
    // Deliberately not committed.

    let value = traceability_json(dir.path(), &["--at", "HEAD"]);

    assert_eq!(value["at"], "HEAD");
    assert!(
        value["requirements"]
            .as_array()
            .expect("requirements array")
            .iter()
            .all(|r| r["requirement_id"] != "timing"),
        "an uncommitted Requirement must not appear when reading a Git ref: {value}"
    );
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

#[test]
fn traceability_names_each_parent_by_uid_when_two_features_share_slugs() {
    // `jump`/`ground` exist under both Features, so a child's `*_id` alone
    // cannot say which parent it hangs from; the parent uid can.
    const DASH_FEATURE_UID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FC2";
    const DASH_BEHAVIOR_UID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FA3";
    const DASH_SCENARIO_UID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FB2";
    let dir = project();
    let root = dir.path();
    write(
        &root.join(".markharness/knowledge/features/player-dash/feature.yml"),
        &format!(
            "id: player-dash\nrequirement_uids: [{REQUIREMENT_UID}]\nlabel: player-dash\naxis: [gameplay]\nuid: {DASH_FEATURE_UID}\n"
        ),
    );
    write(
        &root.join(".markharness/knowledge/features/player-dash/jump/behavior.yml"),
        &format!(
            "id: jump\nfeature: player-dash\nlabel: jump\nuid: {DASH_BEHAVIOR_UID}\naxis: [gameplay]\ndescription: |\n  Jumping while dashing.\nprocedures: {{}}\n"
        ),
    );
    write(
        &root.join(".markharness/knowledge/features/player-dash/jump/ground/scenario.yml"),
        &format!(
            "id: ground\nbehavior: jump\nlabel: ground\nuid: {DASH_SCENARIO_UID}\ndescription: |\n  From the ground.\nphases:\n  - steps:\n      - action: \"Presses jump.\"\n    results:\n      - \"Rises.\"\n"
        ),
    );

    let value = traceability_json(root, &[]);

    let behavior = |uid: &str| {
        value["behaviors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["behavior_uid"] == uid)
            .unwrap_or_else(|| panic!("behavior {uid} missing: {value}"))
            .clone()
    };
    assert_eq!(behavior(DASH_BEHAVIOR_UID)["feature_uid"], DASH_FEATURE_UID);
    // The original Feature has no `uid:` (not migrated), so its Behavior
    // reports a null parent uid rather than omitting the key.
    assert!(behavior(BEHAVIOR_UID).get("feature_uid").unwrap().is_null());

    let scenario = |uid: &str| {
        value["scenarios"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["scenario_uid"] == uid)
            .unwrap_or_else(|| panic!("scenario {uid} missing: {value}"))
            .clone()
    };
    assert_eq!(
        scenario(DASH_SCENARIO_UID)["behavior_uid"],
        DASH_BEHAVIOR_UID
    );
    assert_eq!(scenario(SCENARIO_UID)["behavior_uid"], BEHAVIOR_UID);

    let test_cases = value["test_cases"].as_array().unwrap();
    let parents: Vec<&serde_json::Value> = test_cases.iter().map(|t| &t["scenario_uid"]).collect();
    assert!(parents.contains(&&serde_json::json!(DASH_SCENARIO_UID)));
    assert!(parents.contains(&&serde_json::json!(SCENARIO_UID)));
}

#[test]
fn traceability_lists_a_behavior_that_has_no_scenario() {
    let dir = project();
    let root = dir.path();
    write(
        &root.join(".markharness/knowledge/features/player-jump/double-jump/behavior.yml"),
        "id: double-jump\nfeature: player-jump\nlabel: double jump\nuid: 01ARZ3NDEKTSV4RRFFQ69G5FA2\naxis: [gameplay]\ndescription: |\n  Jumping twice.\nprocedures: {}\n",
    );

    let value = traceability_json(root, &[]);

    let behaviors = value["behaviors"].as_array().unwrap();
    let double_jump = behaviors
        .iter()
        .find(|b| b["behavior_id"] == "double-jump")
        .expect("a Behavior without Scenarios must still be listed");
    assert_eq!(double_jump["behavior_uid"], "01ARZ3NDEKTSV4RRFFQ69G5FA2");
    assert_eq!(double_jump["feature_id"], "player-jump");
    assert_eq!(double_jump["label"], "double jump");
    assert!(
        value["scenarios"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["behavior_id"] != "double-jump")
    );
    assert_eq!(behaviors.len(), 2);
}

const TIMING_UID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";
const AIR_SCENARIO_UID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FB2";

/// `project()` plus a second Requirement and a second Scenario under the same
/// Feature. The Feature contributes to `controls`; the new Scenario names
/// `timing` itself, so (ADR 0031) it contributes only to `timing`.
fn project_with_a_scenario_that_overrides_its_feature() -> tempfile::TempDir {
    let dir = project();
    let root = dir.path();
    write(
        &root.join(".markharness/knowledge/requirements/timing/requirement.yml"),
        &format!(
            "id: timing\nsource: native\nlabel: timing\naxis: [gameplay]\nuid: {TIMING_UID}\n"
        ),
    );
    write(
        &root.join(".markharness/knowledge/features/player-jump/jump/air/scenario.yml"),
        &format!(
            "id: air\nbehavior: jump\nlabel: air\nuid: {AIR_SCENARIO_UID}\nrequirement_uids: [{TIMING_UID}]\ndescription: |\n  In the air.\nphases:\n  - steps:\n      - action: \"Presses jump again.\"\n    results:\n      - \"Rises.\"\n"
        ),
    );
    commit(root, "chore: add a scenario that names its own requirement");
    dir
}

fn case_uids_of(value: &serde_json::Value, requirement_id: &str) -> Vec<String> {
    let requirement = value["requirements"]
        .as_array()
        .expect("requirements array")
        .iter()
        .find(|r| r["requirement_id"] == requirement_id)
        .unwrap_or_else(|| panic!("no requirement {requirement_id} in {value}"));
    requirement["case_uids"]
        .as_array()
        .unwrap_or_else(|| panic!("requirement {requirement_id} has no case_uids: {requirement}"))
        .iter()
        .map(|uid| uid.as_str().unwrap().to_string())
        .collect()
}

fn coverage_case_uids_of(root: &Path, requirement_id: &str) -> Vec<String> {
    let output = run(&[
        "coverage",
        "--requirements",
        "all",
        "--at",
        "HEAD",
        "--dir",
        root.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{output:?}");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let requirement = value["requirements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["requirement_id"] == requirement_id)
        .unwrap();
    let mut uids: Vec<String> = requirement["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|case| case["case_uid"].as_str().map(str::to_string))
        .collect();
    uids.sort();
    uids
}

#[test]
fn traceability_lists_the_cases_of_each_requirement_by_the_rule_coverage_uses() {
    let dir = project_with_a_scenario_that_overrides_its_feature();
    let value = traceability_json(dir.path(), &[]);

    // `air` names `timing`, so it replaces the Feature's `controls` rather
    // than adding to it: `controls` keeps only `ground`.
    for requirement_id in ["controls", "timing"] {
        assert_eq!(
            case_uids_of(&value, requirement_id),
            coverage_case_uids_of(dir.path(), requirement_id),
            "{requirement_id}"
        );
    }
    assert_eq!(case_uids_of(&value, "controls").len(), 1);
    assert_eq!(case_uids_of(&value, "timing").len(), 1);
}

#[test]
fn traceability_case_uids_follow_an_uncommitted_edit() {
    let dir = project_with_a_scenario_that_overrides_its_feature();
    // Drop the override without committing: `air` falls back to the Feature.
    write(
        &dir.path()
            .join(".markharness/knowledge/features/player-jump/jump/air/scenario.yml"),
        &format!(
            "id: air\nbehavior: jump\nlabel: air\nuid: {AIR_SCENARIO_UID}\ndescription: |\n  In the air.\nphases:\n  - steps:\n      - action: \"Presses jump again.\"\n    results:\n      - \"Rises.\"\n"
        ),
    );

    let value = traceability_json(dir.path(), &[]);

    assert_eq!(case_uids_of(&value, "controls").len(), 2);
    assert!(case_uids_of(&value, "timing").is_empty());
}

#[test]
fn traceability_gives_an_empty_case_uids_to_a_requirement_with_no_uid() {
    let dir = project();
    write(
        &dir.path()
            .join(".markharness/knowledge/requirements/legacy/requirement.yml"),
        "id: legacy\nsource: native\nlabel: legacy\naxis: [gameplay]\n",
    );

    let value = traceability_json(dir.path(), &[]);

    assert!(case_uids_of(&value, "legacy").is_empty());
}
