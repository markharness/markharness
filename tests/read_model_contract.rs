//! The public contract of the read outputs (`traceability`, `impact`,
//! `coverage`): their JSON Schema under `schema/` and the representative
//! fixture under `tests/fixtures/read-models/<record_kind>/v1/`
//! (docs/design/cli-read-model-design.md §11, §13.3, §13.4).
//!
//! Each test builds a scratch repo, runs the real CLI, and requires the
//! output to validate against the schema and to equal the fixture, so the
//! schema, the fixture and the CLI cannot drift apart unnoticed.
//!
//! Fixtures write directly to a scratch repo before invoking the CLI binary;
//! that's outside fs_safety's managed-root scope (see clippy.toml).
#![allow(clippy::disallowed_methods)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};

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

fn succeed(args: &[&str]) {
    let output = run(args);
    assert!(output.status.success(), "{args:?}: {output:?}");
}

fn output_json(args: &[&str]) -> Value {
    let output = run(args);
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("a read command must emit JSON")
}

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn read_json(relative: &str) -> Value {
    let content = std::fs::read_to_string(repo_path(relative))
        .unwrap_or_else(|e| panic!("cannot read {relative}: {e}"));
    serde_json::from_str(&content).unwrap_or_else(|e| panic!("{relative} is not JSON: {e}"))
}

fn schema_errors(schema_file: &str, instance: &Value) -> Vec<String> {
    let schema = read_json(&format!("schema/{schema_file}"));
    let validator = jsonschema::validator_for(&schema)
        .unwrap_or_else(|e| panic!("{schema_file} is not a valid schema: {e}"));
    validator
        .iter_errors(instance)
        .map(|e| format!("{e} at {}", e.instance_path()))
        .collect()
}

/// One read model's public contract.
struct Contract {
    record_kind: &'static str,
    schema_file: &'static str,
    /// JSON pointers to Git commit ids in the output. A commit id depends on
    /// the scratch repo's whole tree (including files `init` writes), so it
    /// is checked for shape only and then taken from the fixture.
    commit_pointers: &'static [&'static str],
    /// File stem of the fixture the generic schema checks run against.
    representative: &'static str,
}

const TRACEABILITY: Contract = Contract {
    record_kind: "traceability",
    schema_file: "traceability-read-model.schema.json",
    commit_pointers: &[],
    representative: "representative",
};

/// One element's detail has a different shape per `kind`, so each kind has
/// its own fixture; a Behavior's, which carries every field, stands in for
/// the generic checks.
const TRACEABILITY_DETAIL: Contract = Contract {
    record_kind: "traceability_detail",
    schema_file: "traceability-detail-read-model.schema.json",
    commit_pointers: &[],
    representative: "behavior",
};

const CHANGE_IMPACT: Contract = Contract {
    record_kind: "change_impact",
    schema_file: "change-impact-read-model.schema.json",
    commit_pointers: &[
        "/base_commit",
        "/head_commit",
        "/rejected_trailers/0/commit",
    ],
    representative: "representative",
};

const RELEASE_COVERAGE: Contract = Contract {
    record_kind: "release_coverage",
    schema_file: "release-coverage-read-model.schema.json",
    commit_pointers: &["/at_commit"],
    representative: "representative",
};

impl Contract {
    fn fixture_path(&self, name: &str) -> String {
        format!(
            "tests/fixtures/read-models/{}/v1/{name}.json",
            self.record_kind
        )
    }

    fn fixture(&self) -> Value {
        read_json(&self.fixture_path(self.representative))
    }

    fn assert_matches_cli_output(&self, actual: Value) {
        self.assert_matches_fixture(self.representative, actual);
    }

    fn assert_matches_fixture(&self, name: &str, mut actual: Value) {
        let fixture = read_json(&self.fixture_path(name));
        for pointer in self.commit_pointers {
            let id = actual
                .pointer(pointer)
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("{pointer} must be a string in {actual}"));
            assert!(
                id.len() == 40 && id.bytes().all(|b| b.is_ascii_hexdigit()),
                "{pointer} must be a 40-hex commit id, got {id}"
            );
            *actual.pointer_mut(pointer).unwrap() = fixture
                .pointer(pointer)
                .unwrap_or_else(|| panic!("the fixture lacks {pointer}"))
                .clone();
        }

        assert_eq!(
            schema_errors(self.schema_file, &actual),
            Vec::<String>::new(),
            "the CLI output violates {}:\n{actual:#}",
            self.schema_file
        );
        assert_eq!(
            actual,
            fixture,
            "the CLI output differs from {}; update the fixture only for an intended contract change:\n{actual:#}",
            self.fixture_path(name)
        );
    }
}

fn init_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let root = dir.path();
    succeed(&["init", "--dir", root.to_str().unwrap()]);
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
    dir
}

fn write_native_requirement(root: &Path, id: &str, label: &str, uid: &str) {
    write(
        &root.join(format!(
            ".markharness/knowledge/requirements/{id}/requirement.yml"
        )),
        &format!("id: {id}\nsource: native\nlabel: {label}\naxis: [gameplay]\nuid: {uid}\n"),
    );
}

fn write_feature(root: &Path, id: &str, requirement_uid: &str, uid: Option<&str>) {
    let uid_line = uid.map(|u| format!("uid: {u}\n")).unwrap_or_default();
    write(
        &root.join(format!(".markharness/knowledge/features/{id}/feature.yml")),
        &format!(
            "id: {id}\nrequirement_uids: [{requirement_uid}]\nlabel: {id}\naxis: [gameplay]\n{uid_line}"
        ),
    );
}

fn write_behavior(root: &Path, feature: &str, id: &str, uid: &str) {
    write(
        &root.join(format!(
            ".markharness/knowledge/features/{feature}/{id}/behavior.yml"
        )),
        &format!(
            "id: {id}\nfeature: {feature}\nlabel: {id}\nuid: {uid}\naxis: [gameplay]\ndescription: |\n  Behavior {id}.\nprocedures: {{}}\n"
        ),
    );
}

fn write_scenario(root: &Path, feature: &str, behavior: &str, id: &str, uid: &str, step: &str) {
    write(
        &root.join(format!(
            ".markharness/knowledge/features/{feature}/{behavior}/{id}/scenario.yml"
        )),
        &format!(
            "id: {id}\nbehavior: {behavior}\nlabel: {id}\nuid: {uid}\ndescription: |\n  Scenario {id}.\nphases:\n  - steps:\n      - action: \"{step}\"\n    results:\n      - \"Happens.\"\n"
        ),
    );
}

const CONTROLS_UID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const JUMP_BEHAVIOR_UID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FA1";
const GROUND_SCENARIO_UID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FB1";

/// controls -> player-jump -> jump -> ground (all with UIDs).
fn write_controls_tree(root: &Path, feature_uid: Option<&str>) {
    write_native_requirement(root, "controls", "Player controls", CONTROLS_UID);
    write_feature(root, "player-jump", CONTROLS_UID, feature_uid);
    write_behavior(root, "player-jump", "jump", JUMP_BEHAVIOR_UID);
    write_scenario(
        root,
        "player-jump",
        "jump",
        "ground",
        GROUND_SCENARIO_UID,
        "Presses jump.",
    );
}

fn dir_arg(dir: &tempfile::TempDir) -> &str {
    dir.path().to_str().unwrap()
}

#[test]
fn traceability_output_matches_its_schema_and_fixture() {
    let dir = init_repo();
    let root = dir.path();
    write_controls_tree(root, Some("01ARZ3NDEKTSV4RRFFQ69G5FC1"));
    // A Behavior nothing exercises yet must still be listed.
    write_behavior(
        root,
        "player-jump",
        "double-jump",
        "01ARZ3NDEKTSV4RRFFQ69G5FA2",
    );
    // An external Requirement is reached through source_locator/source_key.
    write(
        &root.join(".markharness/knowledge/requirements/timing/requirement.yml"),
        "id: timing\nsource: external\nsource_locator: docs/requirements.sdoc\nsource_revision: 0123456789abcdef0123456789abcdef01234567\nsource_key: REQ-Timing-01\naxis: [gameplay]\n",
    );
    commit(root, "chore: knowledge");

    let actual = output_json(&["traceability", "--at", "HEAD", "--dir", dir_arg(&dir)]);

    TRACEABILITY.assert_matches_cli_output(actual);
}

#[test]
fn traceability_detail_output_matches_its_schema_and_fixtures() {
    const FEATURE_UID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FC1";
    let dir = init_repo();
    let root = dir.path();
    write_controls_tree(root, Some(FEATURE_UID));
    write(
        &root.join(".markharness/knowledge/requirements/controls/requirement.yml"),
        &format!(
            "id: controls\nsource: native\nlabel: Player controls\naxis: [gameplay]\ndescription: |\n  The player can control the character.\nuid: {CONTROLS_UID}\n"
        ),
    );
    write(
        &root.join(".markharness/knowledge/features/player-jump/feature.yml"),
        &format!(
            "id: player-jump\nrequirement_uids: [{CONTROLS_UID}]\nlabel: player-jump\naxis: [gameplay]\ndescription: |\n  The player jumps.\nuid: {FEATURE_UID}\n"
        ),
    );
    write(
        &root.join(".markharness/knowledge/features/player-jump/jump/behavior.yml"),
        &format!(
            "id: jump\nfeature: player-jump\nlabel: jump\nuid: {JUMP_BEHAVIOR_UID}\naxis: [gameplay]\ndescription: |\n  Jumping.\nprocedures:\n  start-game:\n    steps:\n      - \"Launches the game.\"\n      - \"Loads the stage.\"\n"
        ),
    );
    write(
        &root.join(".markharness/knowledge/features/player-jump/jump/ground/scenario.yml"),
        &format!(
            "id: ground\nbehavior: jump\nlabel: ground\nuid: {GROUND_SCENARIO_UID}\ndescription: |\n  From the ground.\nimplementation_note: Uses the physics tick.\nphases:\n  - steps:\n      - use: start-game\n      - action: \"Presses jump.\"\n    results:\n      - \"Rises.\"\n"
        ),
    );
    commit(root, "chore: knowledge");

    let tree = output_json(&["traceability", "--at", "HEAD", "--dir", dir_arg(&dir)]);
    let case_uid = tree["test_cases"][0]["case_uid"]
        .as_str()
        .expect("the migrated Scenario has a case_uid")
        .to_string();

    for (kind, uid) in [
        ("requirement", CONTROLS_UID),
        ("feature", FEATURE_UID),
        ("behavior", JUMP_BEHAVIOR_UID),
        ("scenario", GROUND_SCENARIO_UID),
        ("test_case", case_uid.as_str()),
    ] {
        let actual = output_json(&[
            "traceability",
            "show",
            "--uid",
            uid,
            "--at",
            "HEAD",
            "--dir",
            dir_arg(&dir),
        ]);
        assert_eq!(actual["kind"], kind);
        TRACEABILITY_DETAIL.assert_matches_fixture(kind, actual);
    }
}

#[test]
fn change_impact_output_matches_its_schema_and_fixture() {
    let dir = init_repo();
    let root = dir.path();

    // Base: four Requirements, each with one Case; `timing` is external and
    // pinned to the current blob of its .sdoc.
    write_controls_tree(root, None);
    write_native_requirement(root, "audio", "Audio", "01ARZ3NDEKTSV4RRFFQ69G5FAW");
    write_feature(root, "game-audio", "01ARZ3NDEKTSV4RRFFQ69G5FAW", None);
    write_behavior(root, "game-audio", "play", "01ARZ3NDEKTSV4RRFFQ69G5FA3");
    write_scenario(
        root,
        "game-audio",
        "play",
        "start",
        "01ARZ3NDEKTSV4RRFFQ69G5FB2",
        "Starts a sound.",
    );
    write_native_requirement(root, "menu", "Menu", "01ARZ3NDEKTSV4RRFFQ69G5FAX");
    write_feature(root, "game-menu", "01ARZ3NDEKTSV4RRFFQ69G5FAX", None);
    write_behavior(root, "game-menu", "open", "01ARZ3NDEKTSV4RRFFQ69G5FA4");
    write_scenario(
        root,
        "game-menu",
        "open",
        "pause",
        "01ARZ3NDEKTSV4RRFFQ69G5FB3",
        "Opens the menu.",
    );
    let sdoc = root.join("docs/requirements.sdoc");
    write(&sdoc, "[REQUIREMENT]\nTITLE: Timing\n");
    let blob =
        String::from_utf8(git(root, &["hash-object", "--", "docs/requirements.sdoc"]).stdout)
            .unwrap()
            .trim()
            .to_string();
    write(
        &root.join(".markharness/knowledge/requirements/timing/requirement.yml"),
        &format!(
            "id: timing\nsource: external\nsource_locator: docs/requirements.sdoc\nsource_revision: {blob}\nsource_key: REQ-Timing-01\naxis: [gameplay]\nuid: 01ARZ3NDEKTSV4RRFFQ69G5FAY\n"
        ),
    );
    write_feature(root, "game-timing", "01ARZ3NDEKTSV4RRFFQ69G5FAY", None);
    write_behavior(root, "game-timing", "tick", "01ARZ3NDEKTSV4RRFFQ69G5FA5");
    write_scenario(
        root,
        "game-timing",
        "tick",
        "frame",
        "01ARZ3NDEKTSV4RRFFQ69G5FB4",
        "Advances a frame.",
    );
    commit(root, "chore: base knowledge");

    // Head: one pair of each alignment status, a stale pin, and a trailer
    // that cannot be counted.
    write_native_requirement(root, "controls", "Player controls v2", CONTROLS_UID);
    write_native_requirement(root, "audio", "Audio v2", "01ARZ3NDEKTSV4RRFFQ69G5FAW");
    write_scenario(
        root,
        "game-audio",
        "play",
        "start",
        "01ARZ3NDEKTSV4RRFFQ69G5FB2",
        "Starts a louder sound.",
    );
    write_native_requirement(root, "menu", "Menu v2", "01ARZ3NDEKTSV4RRFFQ69G5FAX");
    write(&sdoc, "[REQUIREMENT]\nTITLE: Timing, revised\n");
    commit(
        root,
        "docs: change several specs\n\nSpec-Reviewed: requirement=menu case=tc-game-menu-open-pause reason=no-change-required\nSpec-Reviewed: reason=no-change-required\n",
    );

    let actual = output_json(&[
        "impact",
        "--base",
        "HEAD~1",
        "--head",
        "HEAD",
        "--dir",
        dir_arg(&dir),
    ]);

    CHANGE_IMPACT.assert_matches_cli_output(actual);
}

#[test]
fn release_coverage_output_matches_its_schema_and_fixture() {
    let dir = init_repo();
    let root = dir.path();
    write_controls_tree(root, None);
    // A second Case of the same Requirement that the release will not select.
    write_scenario(
        root,
        "player-jump",
        "jump",
        "air",
        "01ARZ3NDEKTSV4RRFFQ69G5FB5",
        "Presses jump in the air.",
    );
    // A Requirement nothing contributes to, and a Feature with no Case.
    write_native_requirement(root, "orphan", "Orphan", "01ARZ3NDEKTSV4RRFFQ69G5FAZ");
    write_feature(root, "player-duck", CONTROLS_UID, None);
    commit(root, "chore: knowledge");

    let coverage = |extra: &[&str]| {
        let mut args = vec!["coverage", "--requirements", "all", "--dir", dir_arg(&dir)];
        args.extend_from_slice(extra);
        output_json(&args)
    };
    let ground_case_uid = coverage(&[])["requirements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["requirement_id"] == "controls")
        .and_then(|r| {
            r["cases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["case_id"] == "tc-player-jump-jump-ground")
        })
        .and_then(|c| c["case_uid"].as_str())
        .expect("the ground Case must have a case_uid")
        .to_string();
    succeed(&[
        "binding",
        "set",
        "--case-uid",
        &ground_case_uid,
        "--mode",
        "automated",
        "--reference",
        "tests/jump.spec.ts",
        "--dir",
        dir_arg(&dir),
    ]);
    succeed(&[
        "release",
        "scope",
        "set",
        "--release",
        "v1.0.0",
        "--case-uid",
        &ground_case_uid,
        "--case-uid",
        "long-gone-case",
        "--dir",
        dir_arg(&dir),
    ]);
    write(&root.join("tests/jump.spec.ts"), "// test\n");
    commit(root, "chore: binding, referenced test and release scope");

    let actual = coverage(&["--release", "v1.0.0"]);

    RELEASE_COVERAGE.assert_matches_cli_output(actual);
}

fn contracts() -> [Contract; 4] {
    [
        TRACEABILITY,
        TRACEABILITY_DETAIL,
        CHANGE_IMPACT,
        RELEASE_COVERAGE,
    ]
}

#[test]
fn each_schema_pins_its_record_kind_and_schema_version() {
    for contract in contracts() {
        let fixture = contract.fixture();
        assert_eq!(fixture["record_kind"], contract.record_kind);
        assert_eq!(fixture["schema_version"], 1);

        let mut wrong_kind = fixture.clone();
        wrong_kind["record_kind"] = json!("something_else");
        assert!(
            !schema_errors(contract.schema_file, &wrong_kind).is_empty(),
            "{} accepted a foreign record_kind",
            contract.schema_file
        );

        let mut wrong_version = fixture.clone();
        wrong_version["schema_version"] = json!(2);
        assert!(
            !schema_errors(contract.schema_file, &wrong_version).is_empty(),
            "{} accepted schema_version 2",
            contract.schema_file
        );
    }
}

#[test]
fn each_schema_requires_every_top_level_field_of_its_fixture() {
    for contract in contracts() {
        let fixture = contract.fixture();
        for key in fixture.as_object().unwrap().keys() {
            // `release` is absent when no release was requested.
            if contract.record_kind == "release_coverage" && key == "release" {
                continue;
            }
            let mut without = fixture.clone();
            without.as_object_mut().unwrap().remove(key);
            assert!(
                !schema_errors(contract.schema_file, &without).is_empty(),
                "{} accepted an output without `{key}`",
                contract.schema_file
            );
        }
    }
}

/// Design §11 item 7: a reader must keep working when a field is added, so
/// the schemas must not forbid unknown fields.
#[test]
fn each_schema_tolerates_an_added_field() {
    for contract in contracts() {
        let mut fixture = contract.fixture();
        fixture["added_later"] = json!(true);
        assert_eq!(
            schema_errors(contract.schema_file, &fixture),
            Vec::<String>::new(),
            "{} rejected an added top-level field",
            contract.schema_file
        );
    }
}

/// ADR 0023: a native Requirement has a label and no source fields; an
/// external one has the source fields and no label.
#[test]
fn the_traceability_schema_keeps_native_and_external_requirements_apart() {
    let fixture = TRACEABILITY.fixture();
    let requirements = fixture["requirements"].as_array().unwrap();
    let index_of = |source: &str| {
        requirements
            .iter()
            .position(|r| r["source"] == source)
            .unwrap_or_else(|| panic!("the fixture needs a {source} Requirement"))
    };
    let native = index_of("native");
    let external = index_of("external");

    let mutations: [(usize, &str, Value); 4] = [
        (native, "source_locator", json!("docs/requirements.sdoc")),
        (native, "label", Value::Null),
        (external, "label", json!("a label")),
        (external, "source_key", Value::Null),
    ];
    for (index, field, value) in mutations {
        let mut mutated = fixture.clone();
        mutated["requirements"][index][field] = value;
        assert!(
            !schema_errors(TRACEABILITY.schema_file, &mutated).is_empty(),
            "accepted requirements[{index}].{field} contradicting its source"
        );
    }
}

/// The detail schema is one `oneOf` over five kinds; a payload whose fields
/// belong to another kind than its `kind` says must not pass.
#[test]
fn the_traceability_detail_schema_keeps_each_kind_to_its_own_fields() {
    let kinds = [
        "requirement",
        "feature",
        "behavior",
        "scenario",
        "test_case",
    ];
    for kind in kinds {
        let fixture = read_json(&TRACEABILITY_DETAIL.fixture_path(kind));
        for other in kinds.into_iter().filter(|other| *other != kind) {
            let mut mutated = fixture.clone();
            mutated["kind"] = json!(other);
            assert!(
                !schema_errors(TRACEABILITY_DETAIL.schema_file, &mutated).is_empty(),
                "accepted a {kind} payload labelled {other}"
            );
        }
        let mut unknown = fixture.clone();
        unknown["kind"] = json!("something_else");
        assert!(
            !schema_errors(TRACEABILITY_DETAIL.schema_file, &unknown).is_empty(),
            "accepted a {kind} payload with an unknown kind"
        );
    }
}
