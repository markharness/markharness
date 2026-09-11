//! `markharness release scope` and `markharness coverage`
//! (ADR 0024, v2 design §6.2).
//!
//! Covers acceptance criteria AC06, AC08, AC11, AC21, AC24, AC25, AC26,
//! AC27, AC28, and AC33.
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
/// Derived from `SCENARIO_UID` alone (ADR 0017 §3), so the test can name it
/// without depending on how it is spelled: it is read back from `coverage`.
fn case_uid_of(root: &Path) -> String {
    let value = coverage_json(root, &["--requirements", "all"]);
    value["requirements"][0]["cases"][0]["case_uid"]
        .as_str()
        .expect("the fixture case must have a case_uid")
        .to_string()
}

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

fn coverage(root: &Path, extra: &[&str]) -> Output {
    let mut args = vec!["coverage", "--dir", root.to_str().unwrap()];
    args.extend_from_slice(extra);
    run(&args)
}

fn coverage_json(root: &Path, extra: &[&str]) -> serde_json::Value {
    let output = coverage(root, extra);
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("coverage must emit JSON")
}

#[test]
fn coverage_reports_the_registered_state_without_a_release() {
    let dir = project();
    let value = coverage_json(dir.path(), &["--requirements", "all"]);

    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["record_kind"], "release_coverage");
    assert_eq!(value["requirements"][0]["requirement_id"], "controls");
    assert_eq!(value["requirements"][0]["feature_ids"][0], "player-jump");
    assert_eq!(
        value["requirements"][0]["cases"][0]["case_id"],
        "tc-player-jump-jump-ground"
    );
    // No release requested, so nothing claims anything was selected.
    assert!(value["release"].is_null(), "{value}");
    assert!(value["requirements"][0]["cases"][0]["selected"].is_null());
}

/// A binding shows how a case would be verified. Its presence is never a
/// claim that anything ran (ADR 0025 §1).
#[test]
fn coverage_reports_the_verification_means_when_a_binding_exists() {
    let dir = project();
    let case_uid = case_uid_of(dir.path());
    assert!(
        run(&[
            "binding",
            "set",
            "--case-uid",
            &case_uid,
            "--mode",
            "automated",
            "--reference",
            "tests/jump.spec.ts",
            "--dir",
            dir.path().to_str().unwrap(),
        ])
        .status
        .success()
    );

    let value = coverage_json(dir.path(), &["--requirements", "all"]);
    let case = &value["requirements"][0]["cases"][0];
    assert_eq!(case["binding_mode"], "automated");
    assert_eq!(case["binding_reference"], "tests/jump.spec.ts");
}

/// AC08: a Requirement nothing contributes to is a coverage gap.
#[test]
fn a_requirement_with_no_contributing_feature_is_a_gap() {
    let dir = project();
    write(
        &dir.path()
            .join(".markharness/knowledge/requirements/orphan/requirement.yml"),
        "id: orphan\nsource: native\nlabel: orphan\naxis: [gameplay]\nuid: 01ARZ3NDEKTSV4RRFFQ69G5FB9\n",
    );
    commit(dir.path(), "docs: add an unreferenced requirement");

    let value = coverage_json(dir.path(), &["--requirements", "orphan"]);
    let gaps = value["gaps"].as_array().unwrap();
    assert_eq!(gaps.len(), 1, "{value}");
    assert_eq!(gaps[0]["kind"], "requirement_has_no_feature");
    assert_eq!(gaps[0]["requirement_id"], "orphan");
}

/// AC21: a Feature that contributes but produces no TestCase is a gap in its
/// own right — read from `feature.yml`, so an empty Feature is still visible.
#[test]
fn a_feature_with_no_case_is_a_gap() {
    let dir = project();
    write(
        &dir.path()
            .join(".markharness/knowledge/features/player-duck/feature.yml"),
        &format!(
            "id: player-duck\nrequirement_uids: [{REQUIREMENT_UID}]\nlabel: player-duck\naxis: [gameplay]\n"
        ),
    );
    commit(dir.path(), "docs: add a feature with no scenario");

    let value = coverage_json(dir.path(), &["--requirements", "controls"]);
    let gaps = value["gaps"].as_array().unwrap();
    assert_eq!(gaps.len(), 1, "{value}");
    assert_eq!(gaps[0]["kind"], "feature_has_no_case");
    assert_eq!(gaps[0]["feature_id"], "player-duck");
}

/// AC24: a recorded scope reproduces what that release selected.
#[test]
fn a_recorded_scope_reports_what_the_release_selected() {
    let dir = project();
    let case_uid = case_uid_of(dir.path());
    assert!(
        run(&[
            "release",
            "scope",
            "set",
            "--release",
            "v1.0.0",
            "--case-uid",
            &case_uid,
            "--dir",
            dir.path().to_str().unwrap(),
        ])
        .status
        .success()
    );
    commit(dir.path(), "chore: record the v1.0.0 scope");

    let value = coverage_json(
        dir.path(),
        &["--requirements", "all", "--release", "v1.0.0"],
    );
    assert_eq!(value["release"]["release_id"], "v1.0.0");
    assert_eq!(value["release"]["selected_case_uids"][0], case_uid.as_str());
    assert_eq!(value["requirements"][0]["cases"][0]["selected"], true);
}

/// AC25: a case in the requested Requirements' scope that the release did
/// not select is a missed-selection candidate.
#[test]
fn an_unselected_case_in_scope_is_listed_as_a_candidate() {
    let dir = project();
    assert!(
        run(&[
            "release",
            "scope",
            "set",
            "--release",
            "v1.0.0",
            "--case-uid",
            "some-other-case",
            "--dir",
            dir.path().to_str().unwrap(),
        ])
        .status
        .success()
    );
    commit(dir.path(), "chore: record a scope missing our case");
    let case_uid = case_uid_of(dir.path());

    let value = coverage_json(
        dir.path(),
        &["--requirements", "all", "--release", "v1.0.0"],
    );
    assert_eq!(
        value["release"]["unselected_case_uids"][0],
        case_uid.as_str(),
        "{value}"
    );
}

/// AC26: a selected Case UID that does not exist at this ref is named, and
/// the stored selection is left exactly as it was.
#[test]
fn a_selected_case_absent_from_the_knowledge_is_reported_not_rewritten() {
    let dir = project();
    assert!(
        run(&[
            "release",
            "scope",
            "set",
            "--release",
            "v1.0.0",
            "--case-uid",
            "long-gone-case",
            "--dir",
            dir.path().to_str().unwrap(),
        ])
        .status
        .success()
    );
    commit(dir.path(), "chore: record a scope naming a vanished case");

    let value = coverage_json(
        dir.path(),
        &["--requirements", "all", "--release", "v1.0.0"],
    );
    assert_eq!(value["release"]["absent_case_uids"][0], "long-gone-case");

    let stored =
        std::fs::read_to_string(dir.path().join(".markharness/releases/v1.0.0.yml")).unwrap();
    assert!(stored.contains("long-gone-case"), "{stored}");
}

/// ADR 0024 §4: a release that never recorded a scope yields the registered
/// state only — never an invented selection.
#[test]
fn a_release_with_no_recorded_scope_yields_the_registered_state_only() {
    let dir = project();
    let value = coverage_json(
        dir.path(),
        &["--requirements", "all", "--release", "v9.9.9"],
    );
    assert!(value["release"].is_null(), "{value}");
    assert!(!value["requirements"].as_array().unwrap().is_empty());
}

/// AC11 / AC06: a past ref reproduces that ref's state, and the same inputs
/// reproduce the same output.
#[test]
fn a_past_ref_reproduces_that_refs_state_deterministically() {
    let dir = project();
    assert!(git(dir.path(), &["tag", "v1"]).status.success());
    write(
        &dir.path()
            .join(".markharness/knowledge/requirements/later/requirement.yml"),
        "id: later\nsource: native\nlabel: later\naxis: [gameplay]\nuid: 01ARZ3NDEKTSV4RRFFQ69G5FC0\n",
    );
    commit(dir.path(), "docs: add a requirement after v1");

    let at_tag = coverage_json(dir.path(), &["--requirements", "all", "--at", "v1"]);
    let again = coverage_json(dir.path(), &["--requirements", "all", "--at", "v1"]);
    assert_eq!(at_tag, again);

    let ids: Vec<&str> = at_tag["requirements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["requirement_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["controls"], "{at_tag}");

    let at_head = coverage_json(dir.path(), &["--requirements", "all"]);
    assert_eq!(at_head["requirements"].as_array().unwrap().len(), 2);
}

/// AC27: there is no way to record a date, a chooser, or a result.
#[test]
fn release_scope_set_rejects_execution_fact_flags() {
    let dir = project();
    for flag in ["--selected-at", "--selected-by", "--result", "--approved"] {
        let output = run(&[
            "release",
            "scope",
            "set",
            "--release",
            "v1.0.0",
            "--case-uid",
            "case-a",
            flag,
            "whatever",
            "--dir",
            dir.path().to_str().unwrap(),
        ]);
        assert!(
            !output.status.success(),
            "`{flag}` must not be accepted: {output:?}"
        );
    }
}

/// AC28: a traversal-shaped release id is refused before any file is made,
/// inside `.markharness/releases/` or outside it.
#[test]
fn release_scope_set_refuses_a_path_shaped_release_id() {
    let dir = project();
    for hostile in ["../../etc/passwd", "..", ".", ".hidden", "a/b", "V1"] {
        let output = run(&[
            "release",
            "scope",
            "set",
            "--release",
            hostile,
            "--case-uid",
            "case-a",
            "--dir",
            dir.path().to_str().unwrap(),
        ]);
        assert_eq!(
            output.status.code(),
            Some(2),
            "`{hostile}` must be refused: {output:?}"
        );
    }
    assert!(
        !dir.path().join(".markharness/releases").exists(),
        "a refused release id must not create the releases directory"
    );
}

/// AC33: a reader of a v2 scope learns which cases were selected and
/// nothing else — no reason, Case revision, build, or environment exists to
/// be read.
#[test]
fn release_scope_show_exposes_only_the_selected_case_uids() {
    let dir = project();
    assert!(
        run(&[
            "release",
            "scope",
            "set",
            "--release",
            "v1.0.0",
            "--case-uid",
            "case-a",
            "--dir",
            dir.path().to_str().unwrap(),
        ])
        .status
        .success()
    );
    commit(dir.path(), "chore: record the scope");

    let output = run(&[
        "release",
        "scope",
        "show",
        "--release",
        "v1.0.0",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{output:?}");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    // Compared as a set: the point is that these four fields are all that
    // exists, not what order the serializer emits them in.
    let mut keys: Vec<&str> = value
        .as_object()
        .unwrap()
        .keys()
        .map(|key| key.as_str())
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec!["case_uids", "record_kind", "release_id", "schema_version"],
        "{value}"
    );
}

#[test]
fn coverage_reports_an_unknown_requirement_rather_than_an_empty_answer() {
    let dir = project();
    let output = coverage(dir.path(), &["--requirements", "no-such-requirement"]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no-such-requirement"), "{stderr}");
}
