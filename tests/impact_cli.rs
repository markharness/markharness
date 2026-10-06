//! `markharness impact` — Change Impact and the alignment check
//! (ADR 0019, v2 design §5.3 and §6.1).
//!
//! Covers acceptance criteria AC03, AC10, AC10b, AC10c, AC12, AC14, AC15,
//! AC16, AC17, AC18, AC19, AC20, AC20b, AC29, AC31, and AC37.
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

/// A migrated project: one native Requirement with a uid, one Feature
/// pointing at that uid, one Behavior, one Scenario with a uid (so the
/// TestCase gets a derived Case UID).
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
    write_requirement_native(root, "controls", "controls");
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
    write_scenario(root, "Presses jump.", "Rises.");
    commit(root, "chore: initial knowledge");
    dir
}

fn write_requirement_native(root: &Path, id: &str, label: &str) {
    write(
        &root
            .join(".markharness/knowledge/requirements")
            .join(id)
            .join("requirement.yml"),
        &format!(
            "id: {id}\nsource: native\nlabel: {label}\naxis: [gameplay]\nuid: {REQUIREMENT_UID}\n"
        ),
    );
}

fn write_scenario(root: &Path, step: &str, result: &str) {
    write(
        &root.join(".markharness/knowledge/features/player-jump/jump/ground/scenario.yml"),
        &format!(
            "id: ground\nbehavior: jump\nlabel: ground\nuid: {SCENARIO_UID}\ndescription: |\n  From the ground.\nphases:\n  - steps:\n      - action: \"{step}\"\n    results:\n      - \"{result}\"\n"
        ),
    );
}

fn impact(root: &Path, extra: &[&str]) -> Output {
    let mut args = vec![
        "impact",
        "--base",
        "HEAD~1",
        "--head",
        "HEAD",
        "--dir",
        root.to_str().unwrap(),
    ];
    args.extend_from_slice(extra);
    run(&args)
}

fn impact_json(root: &Path) -> serde_json::Value {
    let output = impact(root, &[]);
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("impact must emit JSON")
}

fn statuses(value: &serde_json::Value) -> Vec<String> {
    value["requirements"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .flat_map(|requirement| {
            requirement["cases"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|case| case["status"].as_str().unwrap_or_default().to_string())
        })
        .collect()
}

/// AC03 / AC20: the Requirement changed, its related TestCase did not, and
/// nobody confirmed — the pair is reported unconfirmed.
#[test]
fn a_spec_change_with_an_untouched_case_is_unconfirmed() {
    let dir = project();
    write_requirement_native(dir.path(), "controls", "controls v2");
    commit(dir.path(), "docs: reword the requirement");

    let value = impact_json(dir.path());
    assert_eq!(statuses(&value), vec!["unconfirmed"]);
    assert_eq!(value["requirements"][0]["requirement_id"], "controls");
    assert_eq!(value["requirements"][0]["feature_ids"][0], "player-jump");
}

/// AC20b (Feature origin, §6.1 step 1): only the Feature changed, so the
/// Requirement is reached through it; `spec_changed` stays false because the
/// Requirement side did not move.
#[test]
fn a_feature_only_change_reaches_its_requirement_without_a_spec_change() {
    let dir = project();
    write_scenario(dir.path(), "Presses jump twice.", "Rises higher.");
    commit(dir.path(), "feat: change the scenario only");

    let value = impact_json(dir.path());
    assert_eq!(value["requirements"].as_array().unwrap().len(), 1);
    assert_eq!(value["requirements"][0]["requirement_id"], "controls");
    assert_eq!(value["requirements"][0]["spec_changed"], false);
    assert_eq!(value["requirements"][0]["feature_ids"][0], "player-jump");
    assert_eq!(statuses(&value), vec!["followed_up"]);
}

/// A Feature edit that leaves every case's revision alone has nothing that
/// says the case was reconciled with it, so the pair stays unconfirmed.
#[test]
fn a_feature_change_that_leaves_the_case_revision_is_unconfirmed() {
    let dir = project();
    write(
        &dir.path()
            .join(".markharness/knowledge/features/player-jump/feature.yml"),
        &format!(
            "id: player-jump
requirement_uids: [{REQUIREMENT_UID}]
label: player-jump v2
axis: [gameplay]
"
        ),
    );
    commit(dir.path(), "docs: relabel the feature");

    let value = impact_json(dir.path());
    assert_eq!(value["requirements"][0]["spec_changed"], false);
    assert_eq!(statuses(&value), vec!["unconfirmed"]);
}

/// A Feature with no Scenario generates no case, but it still contributes to
/// its Requirements, so changing it must still reach them.
#[test]
fn a_changed_feature_without_cases_still_reaches_its_requirement() {
    let dir = project();
    write(
        &dir.path()
            .join(".markharness/knowledge/features/idle/feature.yml"),
        &format!(
            "id: idle
requirement_uids: [{REQUIREMENT_UID}]
label: idle
axis: [gameplay]
"
        ),
    );
    commit(dir.path(), "feat: add a feature that has no scenario yet");

    let value = impact_json(dir.path());
    assert_eq!(value["requirements"].as_array().unwrap().len(), 1);
    assert_eq!(value["requirements"][0]["spec_changed"], false);
    assert_eq!(
        value["requirements"][0]["feature_ids"],
        serde_json::json!(["idle", "player-jump"])
    );
}

/// ADR 0031: a Scenario can contribute to a Requirement its Feature does not
/// list, and that Feature is then one of the Requirement's Features.
#[test]
fn a_scenario_level_contribution_makes_its_feature_a_related_feature() {
    let dir = project();
    let root = dir.path();
    write(
        &root.join(".markharness/knowledge/features/other/feature.yml"),
        "id: other
requirement_uids: []
label: other
axis: [gameplay]
",
    );
    write(
        &root.join(".markharness/knowledge/features/other/dash/behavior.yml"),
        "id: dash
feature: other
label: dash
axis: [gameplay]
description: |
  Dashing.
procedures: {}
",
    );
    write(
        &root.join(".markharness/knowledge/features/other/dash/ground/scenario.yml"),
        &format!(
            "id: ground
behavior: dash
label: ground
uid: 01ARZ3NDEKTSV4RRFFQ69G5FB2
requirement_uids: [{REQUIREMENT_UID}]
description: |
  From the ground.
phases:
  - steps:
      - action: \"Presses dash.\"
    results:
      - \"Moves.\"
"
        ),
    );
    commit(
        root,
        "feat: add a scenario that contributes to the requirement",
    );

    let value = impact_json(root);
    assert_eq!(value["requirements"].as_array().unwrap().len(), 1);
    assert_eq!(value["requirements"][0]["spec_changed"], false);
    assert_eq!(
        value["requirements"][0]["feature_ids"],
        serde_json::json!(["other", "player-jump"])
    );
}

/// A Requirement reached both ways appears once, with `spec_changed` true.
#[test]
fn a_requirement_changed_on_both_origins_is_reported_once() {
    let dir = project();
    write_requirement_native(dir.path(), "controls", "controls v2");
    write_scenario(dir.path(), "Presses jump twice.", "Rises higher.");
    commit(dir.path(), "feat: change both sides");

    let value = impact_json(dir.path());
    assert_eq!(value["requirements"].as_array().unwrap().len(), 1);
    assert_eq!(value["requirements"][0]["spec_changed"], true);
}

/// AC15: both sides moved in the same range, but nothing records that a
/// human judged them to still agree.
#[test]
fn a_same_range_change_to_both_sides_is_followed_up_not_confirmed() {
    let dir = project();
    write_requirement_native(dir.path(), "controls", "controls v2");
    write_scenario(dir.path(), "Presses jump twice.", "Rises higher.");
    commit(dir.path(), "feat: change both sides");

    assert_eq!(statuses(&impact_json(dir.path())), vec!["followed_up"]);
}

/// AC04: a trailer naming both sides confirms that pair.
#[test]
fn a_trailer_naming_both_sides_confirms_the_pair() {
    let dir = project();
    write_requirement_native(dir.path(), "controls", "controls v2");
    commit(
        dir.path(),
        "docs: reword the requirement\n\nSpec-Reviewed: requirement=controls case=tc-player-jump-jump-ground reason=no-change-required\n",
    );

    let value = impact_json(dir.path());
    assert_eq!(statuses(&value), vec!["confirmed"]);
    assert!(
        value["rejected_trailers"].as_array().unwrap().is_empty(),
        "{value}"
    );
}

/// AC12 / AC16: a trailer with no target cannot say which pair it confirms.
#[test]
fn a_trailer_without_a_target_confirms_nothing_and_is_reported() {
    let dir = project();
    write_requirement_native(dir.path(), "controls", "controls v2");
    commit(
        dir.path(),
        "docs: reword the requirement\n\nSpec-Reviewed: reason=no-change-required\n",
    );

    let value = impact_json(dir.path());
    assert_eq!(statuses(&value), vec!["unconfirmed"]);
    let rejected = value["rejected_trailers"].as_array().unwrap();
    assert_eq!(rejected.len(), 1, "{value}");
    assert!(
        rejected[0]["reason"]
            .as_str()
            .unwrap()
            .contains("requirement="),
        "{value}"
    );
}

/// AC14: the Requirement is confirmed, then changed again in the same range.
#[test]
fn a_later_change_to_the_requirement_voids_the_confirmation() {
    let dir = project();
    write_requirement_native(dir.path(), "controls", "controls v2");
    commit(
        dir.path(),
        "docs: reword\n\nSpec-Reviewed: requirement=controls case=tc-player-jump-jump-ground reason=no-change-required\n",
    );
    write_requirement_native(dir.path(), "controls", "controls v3");
    commit(dir.path(), "docs: reword again");

    let output = run(&[
        "impact",
        "--base",
        "HEAD~2",
        "--head",
        "HEAD",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{output:?}");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(statuses(&value), vec!["unconfirmed"], "{value}");
}

/// AC29: the case changes again after the pair was confirmed, even though
/// the Requirement did not move.
#[test]
fn a_later_change_to_the_case_voids_the_confirmation() {
    let dir = project();
    write_requirement_native(dir.path(), "controls", "controls v2");
    commit(
        dir.path(),
        "docs: reword\n\nSpec-Reviewed: requirement=controls case=tc-player-jump-jump-ground reason=no-change-required\n",
    );
    write_scenario(dir.path(), "Presses jump twice.", "Rises higher.");
    commit(dir.path(), "test: adjust the case");

    let output = run(&[
        "impact",
        "--base",
        "HEAD~2",
        "--head",
        "HEAD",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{output:?}");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(statuses(&value), vec!["followed_up"], "{value}");
}

/// AC37: the same range and rule version reproduce the same verdict, and the
/// output carries what a recomputation would need to compare against.
#[test]
fn the_output_carries_resolved_commits_and_rule_version_and_is_reproducible() {
    let dir = project();
    write_requirement_native(dir.path(), "controls", "controls v2");
    commit(dir.path(), "docs: reword the requirement");

    let first = impact_json(dir.path());
    let second = impact_json(dir.path());
    assert_eq!(first, second);
    assert_eq!(first["schema_version"], 1);
    assert_eq!(first["record_kind"], "change_impact");
    assert_eq!(first["rule_version"], 1);
    assert_eq!(first["base_commit"].as_str().unwrap().len(), 40);
    assert_eq!(first["head_commit"].as_str().unwrap().len(), 40);
}

/// `--fail-on-findings` is opt-in: whether a finding should fail CI is the
/// team's policy, so the default exit stays 0.
#[test]
fn findings_only_change_the_exit_code_when_asked() {
    let dir = project();
    write_requirement_native(dir.path(), "controls", "controls v2");
    commit(dir.path(), "docs: reword the requirement");

    assert_eq!(impact(dir.path(), &[]).status.code(), Some(0));
    assert_eq!(
        impact(dir.path(), &["--fail-on-findings"]).status.code(),
        Some(2)
    );
}

#[test]
fn a_clean_range_reports_no_findings_even_with_the_flag() {
    let dir = project();
    write(&dir.path().join("README.md"), "unrelated\n");
    commit(dir.path(), "docs: unrelated change");

    let output = impact(dir.path(), &["--fail-on-findings"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
}

/// AC10 / AC10c / AC18 / AC19: an external Requirement's spec change is the
/// `.sdoc` blob moving between base and head; a pin that merely lags behind
/// head is reported separately and never as a spec change.
#[test]
fn an_external_requirement_separates_a_spec_change_from_a_stale_pin() {
    let dir = project();
    let sdoc = dir.path().join("docs/requirements.sdoc");
    write(&sdoc, "[REQUIREMENT]\nTITLE: Controls\n");
    let blob = String::from_utf8(
        git(dir.path(), &["hash-object", "--", "docs/requirements.sdoc"])
            .stdout
            .clone(),
    )
    .unwrap()
    .trim()
    .to_string();
    write(
        &dir.path()
            .join(".markharness/knowledge/requirements/controls/requirement.yml"),
        &format!(
            "id: controls\nsource: external\nsource_locator: docs/requirements.sdoc\nsource_revision: {blob}\naxis: [gameplay]\nuid: {REQUIREMENT_UID}\n"
        ),
    );
    commit(dir.path(), "docs: point the requirement at the sdoc");

    // AC19: nothing moved since, so no spec change and no stale pin.
    let value = impact_json(dir.path());
    assert!(
        value["stale_pins"].as_array().unwrap().is_empty(),
        "{value}"
    );

    // AC10: the .sdoc itself changes -> a spec change.
    write(&sdoc, "[REQUIREMENT]\nTITLE: Controls, revised\n");
    commit(dir.path(), "docs: revise the sdoc");
    let value = impact_json(dir.path());
    assert_eq!(statuses(&value), vec!["unconfirmed"], "{value}");
    // AC10c: the pin now lags head, reported as a stale pin in its own right.
    let pins = value["stale_pins"].as_array().unwrap();
    assert_eq!(pins.len(), 1, "{value}");
    assert_eq!(pins[0]["requirement_id"], "controls");
}

/// AC10b: a native Requirement's spec change is its own file moving.
#[test]
fn a_native_requirements_own_edit_is_the_spec_change() {
    let dir = project();
    write(
        &dir.path().join("docs/unrelated.md"),
        "nothing to do with it\n",
    );
    commit(dir.path(), "docs: unrelated");
    let value = impact_json(dir.path());
    assert!(
        value["requirements"].as_array().unwrap().is_empty(),
        "an unrelated edit must not report a spec change: {value}"
    );
}

/// AC17: history that cannot be walked fails with a diagnostic rather than
/// reporting an empty, clean range.
#[test]
fn an_unwalkable_range_fails_with_a_diagnostic() {
    let dir = project();
    let output = run(&[
        "impact",
        "--base",
        "refs/heads/no-such-branch",
        "--head",
        "HEAD",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no-such-branch"), "{stderr}");
}
