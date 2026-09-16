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
    assert!(value["requirements"][0]["source_locator"].is_null());
    assert!(value["requirements"][0]["source_key"].is_null());

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
