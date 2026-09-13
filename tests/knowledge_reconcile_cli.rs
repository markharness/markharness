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

fn setup_root_with_axes(axis_ids: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(
        dir.path()
            .join(markharness::project_root::MARKHARNESS_DIR)
            .join("knowledge"),
    )
    .unwrap();
    fs::write(
        dir.path().join(markharness::project_root::MARKER_FILE),
        "schema_version = 1\n",
    )
    .unwrap();
    fs::create_dir_all(
        dir.path()
            .join(markharness::project_root::MARKHARNESS_DIR)
            .join("axes"),
    )
    .unwrap();
    for id in axis_ids {
        fs::write(
            dir.path()
                .join(markharness::project_root::MARKHARNESS_DIR)
                .join("axes")
                .join(format!("{id}.yml")),
            format!("id: {id}\nlabel: {id}\n"),
        )
        .unwrap();
    }
    dir
}

fn write_intent(dir: &tempfile::TempDir, contents: &str) -> std::path::PathBuf {
    let path = dir.path().join("intent.yml");
    fs::write(&path, contents).unwrap();
    path
}

const VALID_INTENT: &str = "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - key: req_todo
    id: todo
    source: native
    label: TODO management
    axis: [functional]

features:
  - key: feature_todo
    id: todo-management
    contributes_to: [req_todo]
    label: TODO management
    axis: [functional]
";

#[test]
fn valid_intent_exits_zero() {
    let dir = setup_root_with_axes(&["functional"]);
    let intent_file = write_intent(&dir, VALID_INTENT);

    let output = run(&[
        "knowledge",
        "reconcile",
        intent_file.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
    ]);

    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn valid_intent_with_json_reports_created_elements() {
    let dir = setup_root_with_axes(&["functional"]);
    let intent_file = write_intent(&dir, VALID_INTENT);

    let output = run(&[
        "knowledge",
        "reconcile",
        intent_file.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
    ]);

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"kind\":\"requirement\""), "{stdout}");
    assert!(stdout.contains("\"kind\":\"feature\""), "{stdout}");
}

#[test]
fn valid_intent_writes_canonical_knowledge_files_with_uid_embedded() {
    let dir = setup_root_with_axes(&["functional"]);
    let intent_file = write_intent(&dir, VALID_INTENT);

    let output = run(&[
        "knowledge",
        "reconcile",
        intent_file.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
    ]);
    assert!(output.status.success());

    let requirement = fs::read_to_string(
        dir.path()
            .join(markharness::project_root::MARKHARNESS_DIR)
            .join("knowledge/requirements/todo/requirement.yml"),
    )
    .unwrap();
    assert!(requirement.contains("uid:"), "{requirement}");

    let feature = fs::read_to_string(
        dir.path()
            .join(markharness::project_root::MARKHARNESS_DIR)
            .join("knowledge/features/todo-management/feature.yml"),
    )
    .unwrap();
    assert!(feature.contains("uid:"), "{feature}");
}

#[test]
fn rerunning_the_same_intent_reports_unchanged() {
    let dir = setup_root_with_axes(&["functional"]);
    let intent_file = write_intent(&dir, VALID_INTENT);

    let first = run(&[
        "knowledge",
        "reconcile",
        intent_file.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
    ]);
    assert!(first.status.success());

    let second = run(&[
        "knowledge",
        "reconcile",
        intent_file.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
    ]);

    assert!(second.status.success());
    let stdout = String::from_utf8_lossy(&second.stdout);
    assert!(stdout.contains("\"kind\":\"requirement\""), "{stdout}");
    assert!(stdout.contains("\"unchanged\""), "{stdout}");
}

#[test]
fn rerunning_with_different_content_for_the_same_id_reports_ambiguous_identity() {
    let dir = setup_root_with_axes(&["functional"]);
    let intent_file = write_intent(&dir, VALID_INTENT);

    let first = run(&[
        "knowledge",
        "reconcile",
        intent_file.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
    ]);
    assert!(first.status.success());

    let changed_intent = write_intent(
        &dir,
        &VALID_INTENT.replace("TODO management", "TODO management (changed)"),
    );
    let second = run(&[
        "knowledge",
        "reconcile",
        changed_intent.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
    ]);

    assert!(!second.status.success());
    let stdout = String::from_utf8_lossy(&second.stdout);
    assert!(
        stdout.contains("\"code\":\"ambiguous_identity\""),
        "{stdout}"
    );
}

#[test]
fn malformed_yaml_reports_invalid_format_and_exits_nonzero() {
    let dir = setup_root_with_axes(&[]);
    let intent_file = write_intent(&dir, "not: [valid");

    let output = run(&[
        "knowledge",
        "reconcile",
        intent_file.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
    ]);

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"code\":\"invalid_format\""), "{stdout}");
}

#[test]
fn unrecognized_format_reports_invalid_format() {
    let dir = setup_root_with_axes(&[]);
    let intent_file = write_intent(&dir, "format: some/other/v9\nmode: merge\n");

    let output = run(&[
        "knowledge",
        "reconcile",
        intent_file.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
    ]);

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"code\":\"invalid_format\""), "{stdout}");
}

#[test]
fn duplicate_key_reports_diagnostic() {
    let dir = setup_root_with_axes(&[]);
    let intent_file = write_intent(
        &dir,
        "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - key: dup
    id: a
    source: native
    label: A
    axis: []
  - key: dup
    id: b
    source: native
    label: B
    axis: []
",
    );

    let output = run(&[
        "knowledge",
        "reconcile",
        intent_file.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
    ]);

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"code\":\"duplicate_key\""), "{stdout}");
}

#[test]
fn unknown_local_reference_reports_diagnostic() {
    let dir = setup_root_with_axes(&[]);
    let intent_file = write_intent(
        &dir,
        "\
format: markharness/knowledge-intent/v1
mode: merge

features:
  - id: todo-management
    contributes_to: [does_not_exist]
    label: TODO management
    axis: []
",
    );

    let output = run(&[
        "knowledge",
        "reconcile",
        intent_file.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
    ]);

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"code\":\"unknown_local_reference\""),
        "{stdout}"
    );
}

#[test]
fn unknown_axis_reports_diagnostic() {
    let dir = setup_root_with_axes(&[]);
    let intent_file = write_intent(
        &dir,
        "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - key: req_todo
    id: todo
    source: native
    label: TODO management
    axis: [nonexistent]
",
    );

    let output = run(&[
        "knowledge",
        "reconcile",
        intent_file.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
    ]);

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"code\":\"unknown_axis\""), "{stdout}");
}

#[test]
fn uid_selected_patch_updates_label_and_reports_updated() {
    let dir = setup_root_with_axes(&["functional"]);
    let intent_file = write_intent(&dir, VALID_INTENT);

    let first = run(&[
        "knowledge",
        "reconcile",
        intent_file.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
    ]);
    assert!(first.status.success());
    let first_json: serde_json::Value = serde_json::from_slice(&first.stdout).expect("valid json");
    let requirement_uid = first_json["created"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "requirement")
        .unwrap()["uid"]
        .as_str()
        .unwrap()
        .to_string();

    let patch_intent = write_intent(
        &dir,
        &format!(
            "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - uid: {requirement_uid}
    label: TODO management (updated)
"
        ),
    );
    let second = run(&[
        "knowledge",
        "reconcile",
        patch_intent.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
    ]);

    assert!(second.status.success());
    let stdout = String::from_utf8_lossy(&second.stdout);
    assert!(stdout.contains("\"updated\""), "{stdout}");

    let content = fs::read_to_string(
        dir.path()
            .join(markharness::project_root::MARKHARNESS_DIR)
            .join("knowledge/requirements/todo/requirement.yml"),
    )
    .unwrap();
    assert!(
        content.contains("label: TODO management (updated)"),
        "{content}"
    );
}

/// ADR 0027 §7: the machine-readable result reports each element's
/// changed path, not just its identity, so a caller can tell which
/// canonical Knowledge files a run touched without guessing the layout.
#[test]
fn json_reports_the_changed_path_for_created_elements() {
    let dir = setup_root_with_axes(&["functional"]);
    let intent_file = write_intent(&dir, VALID_INTENT);

    let output = run(&[
        "knowledge",
        "reconcile",
        intent_file.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
    ]);
    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid json");
    let created = json["created"].as_array().unwrap();
    let path_of = |kind: &str| {
        created.iter().find(|e| e["kind"] == kind).unwrap()["path"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert_eq!(
        path_of("requirement"),
        ".markharness/knowledge/requirements/todo/requirement.yml"
    );
    assert_eq!(
        path_of("feature"),
        ".markharness/knowledge/features/todo-management/feature.yml"
    );
    assert!(
        created.iter().all(|e| e.get("previous_path").is_none()),
        "a creation never moved anything: {created:?}"
    );
}

/// A content patch and a rename both rewrite the file in place — the ADR's
/// rename keeps the directory — so both report `path` and no
/// `previous_path`. A caller can therefore treat the presence of
/// `previous_path` as "this file moved".
#[test]
fn json_reports_the_changed_path_for_a_patch_and_a_rename() {
    let dir = setup_root_with_axes(&["functional"]);
    let intent_file = write_intent(&dir, VALID_INTENT);
    let first = run(&[
        "knowledge",
        "reconcile",
        intent_file.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
    ]);
    assert!(first.status.success());
    let first_json: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    let requirement_uid = first_json["created"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "requirement")
        .unwrap()["uid"]
        .as_str()
        .unwrap()
        .to_string();

    for intent in [
        format!(
            "format: markharness/knowledge-intent/v1\nmode: merge\n\nrequirements:\n  - uid: {requirement_uid}\n    label: Patched\n"
        ),
        format!(
            "format: markharness/knowledge-intent/v1\nmode: merge\n\nrequirements:\n  - uid: {requirement_uid}\n    id: task\n"
        ),
    ] {
        let path = write_intent(&dir, &intent);
        let output = run(&[
            "knowledge",
            "reconcile",
            path.to_str().unwrap(),
            "--dir",
            dir.path().to_str().unwrap(),
            "--json",
        ]);
        assert!(output.status.success());
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let updated = &json["updated"].as_array().unwrap()[0];
        assert_eq!(
            updated["path"], ".markharness/knowledge/requirements/todo/requirement.yml",
            "even a rename keeps the directory: {updated:?}"
        );
        assert!(updated.get("previous_path").is_none(), "{updated:?}");
    }
}

/// A reparented Scenario is the one update that actually relocates a file,
/// so its result carries both the new `path` and the `previous_path` it
/// came from.
#[test]
fn json_reports_both_paths_for_a_reparented_scenario() {
    let dir = setup_root_with_axes(&[]);
    let create = write_intent(
        &dir,
        "\
format: markharness/knowledge-intent/v1
mode: merge

features:
  - id: todo-management
    label: TODO management
    axis: []
    behaviors:
      - id: capture
        description: Capture a TODO.
        scenarios:
          - id: empty-title
            description: An empty title cannot be added
            phases:
              - steps:
                  - action: Attempt to add an empty title
                results:
                  - No TODO is added
      - id: review
        description: Review a TODO.
",
    );
    let first = run(&[
        "knowledge",
        "reconcile",
        create.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
    ]);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first_json: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    let created = first_json["created"].as_array().unwrap();
    let uid_of = |kind: &str, id: &str| {
        created
            .iter()
            .find(|e| e["kind"] == kind && e["id"] == id)
            .unwrap()["uid"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let reparent = write_intent(
        &dir,
        &format!(
            "format: markharness/knowledge-intent/v1\nmode: merge\n\nfeatures:\n  - uid: {}\n    behaviors:\n      - uid: {}\n        scenarios:\n          - uid: {}\n",
            uid_of("feature", "todo-management"),
            uid_of("behavior", "review"),
            uid_of("scenario", "empty-title"),
        ),
    );
    let output = run(&[
        "knowledge",
        "reconcile",
        reparent.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let scenario = json["updated"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "scenario")
        .unwrap();
    assert_eq!(
        scenario["path"],
        ".markharness/knowledge/features/todo-management/review/empty-title/scenario.yml"
    );
    assert_eq!(
        scenario["previous_path"],
        ".markharness/knowledge/features/todo-management/capture/empty-title/scenario.yml"
    );
}

/// End-to-end proof through the binary that ADR 0027 §3's rename row
/// covers Behavior and Scenario too: the Behavior is rewritten in place,
/// the Scenario's file moves, and the JSON reports both paths.
#[test]
fn renaming_a_behavior_and_a_scenario_reports_their_paths() {
    let dir = setup_root_with_axes(&[]);
    let create = write_intent(
        &dir,
        "\
format: markharness/knowledge-intent/v1
mode: merge

features:
  - id: todo-management
    label: TODO management
    axis: []
    behaviors:
      - id: capture
        description: Capture a TODO.
        scenarios:
          - id: empty-title
            description: An empty title cannot be added
            phases:
              - steps:
                  - action: Attempt to add an empty title
                results:
                  - No TODO is added
",
    );
    let first = run(&[
        "knowledge",
        "reconcile",
        create.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
    ]);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first_json: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    let uid_of = |kind: &str| {
        first_json["created"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["kind"] == kind)
            .unwrap()["uid"]
            .as_str()
            .unwrap()
            .to_string()
    };

    let rename = write_intent(
        &dir,
        &format!(
            "format: markharness/knowledge-intent/v1\nmode: merge\n\nfeatures:\n  - uid: {}\n    behaviors:\n      - uid: {}\n        id: recorded\n        scenarios:\n          - uid: {}\n            id: blank-title\n",
            uid_of("feature"),
            uid_of("behavior"),
            uid_of("scenario"),
        ),
    );
    let output = run(&[
        "knowledge",
        "reconcile",
        rename.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let updated = json["updated"].as_array().unwrap();
    let behavior = updated.iter().find(|e| e["kind"] == "behavior").unwrap();
    assert_eq!(behavior["id"], "recorded");
    assert_eq!(
        behavior["path"],
        ".markharness/knowledge/features/todo-management/capture/behavior.yml"
    );
    let scenario = updated.iter().find(|e| e["kind"] == "scenario").unwrap();
    assert_eq!(scenario["id"], "blank-title");
    assert_eq!(
        scenario["previous_path"],
        ".markharness/knowledge/features/todo-management/capture/empty-title/scenario.yml"
    );
    assert_eq!(
        scenario["path"],
        ".markharness/knowledge/features/todo-management/capture/blank-title/scenario.yml"
    );
    assert!(
        dir.path()
            .join(markharness::project_root::MARKHARNESS_DIR)
            .join("knowledge/features/todo-management/capture/blank-title/scenario.yml")
            .is_file()
    );
}

#[test]
fn check_on_a_new_intent_reports_planned_changes_without_writing_and_exits_4() {
    let dir = setup_root_with_axes(&["functional"]);
    let intent_file = write_intent(&dir, VALID_INTENT);

    let output = run(&[
        "knowledge",
        "reconcile",
        intent_file.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
        "--check",
    ]);

    assert_eq!(
        output.status.code(),
        Some(4),
        "stdout={}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"kind\":\"requirement\""), "{stdout}");
    assert!(
        !dir.path()
            .join(markharness::project_root::MARKHARNESS_DIR)
            .join("knowledge/requirements/todo/requirement.yml")
            .is_file(),
        "--check must not write any Knowledge file"
    );
}

#[test]
fn check_on_an_already_reconciled_repository_exits_zero_with_unchanged() {
    let dir = setup_root_with_axes(&["functional"]);
    let intent_file = write_intent(&dir, VALID_INTENT);

    let first = run(&[
        "knowledge",
        "reconcile",
        intent_file.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
    ]);
    assert!(first.status.success());

    let second = run(&[
        "knowledge",
        "reconcile",
        intent_file.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
        "--check",
    ]);

    assert!(
        second.status.success(),
        "stdout={}",
        String::from_utf8_lossy(&second.stdout)
    );
    let stdout = String::from_utf8_lossy(&second.stdout);
    assert!(stdout.contains("\"unchanged\""), "{stdout}");
}

#[test]
fn missing_intent_file_reports_io_error() {
    let dir = setup_root_with_axes(&[]);

    let output = run(&[
        "knowledge",
        "reconcile",
        dir.path().join("does-not-exist.yml").to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
    ]);

    assert!(!output.status.success());
}

/// ADR 0028 §1: `--print-template` replaces `knowledge scaffold`, and the
/// usage it defines takes no other option — an Intent path and the
/// template are mutually exclusive requests.
#[test]
fn print_template_prints_a_blank_knowledge_intent_to_stdout() {
    let output = run(&["knowledge", "reconcile", "--print-template"]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("format: markharness/knowledge-intent/v1"),
        "{stdout}"
    );
    assert!(stdout.contains("requirements:"), "{stdout}");
    assert!(stdout.contains("scenarios:"), "{stdout}");
}

#[test]
fn reconcile_requires_either_an_intent_file_or_print_template() {
    let output = run(&["knowledge", "reconcile"]);

    assert!(!output.status.success(), "{output:?}");
}

#[test]
fn print_template_cannot_be_combined_with_an_intent_file_or_other_options() {
    for args in [
        vec!["knowledge", "reconcile", "--print-template", "intent.yml"],
        vec!["knowledge", "reconcile", "--print-template", "--check"],
        vec!["knowledge", "reconcile", "--print-template", "--json"],
        vec!["knowledge", "reconcile", "--print-template", "--dir", "."],
    ] {
        let output = run(&args);
        assert!(!output.status.success(), "{args:?} -> {output:?}");
    }
}

/// Ported from the deleted `knowledge apply` suite (ADR 0028 §2 keeps the
/// coverage, not the command): a multi-line description whose lines
/// contain `": "` must survive the round trip through the canonical block
/// scalar and still satisfy `markharness validate`. It pairs with the
/// `multiline_label` check — `description` is written as a block scalar and
/// may wrap, `label` is a plain scalar and may not.
#[test]
fn a_multiline_description_reparses_and_passes_project_validation() {
    let dir = tempfile::tempdir().unwrap();
    let init_output = run(&["init", "--dir", dir.path().to_str().unwrap()]);
    assert!(init_output.status.success(), "{init_output:?}");
    fs::write(
        dir.path().join(".markharness/axes/gameplay.yml"),
        "id: gameplay
label: gameplay
",
    )
    .unwrap();

    let intent_path = dir.path().join("intent.yml");
    fs::write(
        &intent_path,
        "format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - key: req_controls
    id: controls
    source: native
    label: controls
    axis: [gameplay]

features:
  - key: feature_player_jump
    id: player-jump
    contributes_to: [req_controls]
    label: player-jump
    axis: [gameplay]
    behaviors:
      - id: jump
        label: jump
        axis: [gameplay]
        description: |
          line one about foo.js: bar()
          line two about baz.js: qux()
        scenarios:
          - id: ground
            label: ground
            description: Jump from the ground and land
            phases:
              - steps:
                  - action: Do it.
                results:
                  - lands safely
",
    )
    .unwrap();

    let output = run(&[
        "knowledge",
        "reconcile",
        intent_path.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");

    let written = fs::read_to_string(
        dir.path()
            .join(".markharness/knowledge/features/player-jump/jump/behavior.yml"),
    )
    .unwrap();
    let behavior = markharness::knowledge::parse_behavior(&written).unwrap();
    assert_eq!(
        behavior.description,
        "line one about foo.js: bar()
line two about baz.js: qux()
"
    );

    let validate_output = run(&["validate", "--dir", dir.path().to_str().unwrap(), "--json"]);
    assert_eq!(
        validate_output.status.code(),
        Some(0),
        "{validate_output:?}"
    );
    assert_eq!(
        String::from_utf8_lossy(&validate_output.stdout).trim(),
        "{\"ok\":true}"
    );
}

/// `knowledge reconcile` is the only way Knowledge gets saved (ADR 0028
/// §1), so anything it writes must satisfy `markharness validate`. ADR
/// 0023 gives native and external Requirements disjoint field sets, and an
/// earlier version of this command wrote every new Requirement with a
/// `label` and no `source_revision` — producing an external Requirement
/// that validation rejected the moment it was written.
#[test]
fn a_new_external_requirement_is_written_in_a_state_project_validation_accepts() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        run(&["init", "--dir", dir.path().to_str().unwrap()])
            .status
            .success()
    );
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.email", "test@example.com"],
        vec!["config", "user.name", "Test"],
    ] {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(&args)
                .status()
                .unwrap()
                .success()
        );
    }
    fs::write(
        dir.path().join("spec.sdoc"),
        "the external spec
",
    )
    .unwrap();

    let intent_path = dir.path().join("intent.yml");
    fs::write(
        &intent_path,
        "format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - id: controls
    source: external
    axis: []
    source_locator: spec.sdoc
    source_revision: current
",
    )
    .unwrap();

    let output = run(&[
        "knowledge",
        "reconcile",
        intent_path.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");

    let written = fs::read_to_string(
        dir.path()
            .join(".markharness/knowledge/requirements/controls/requirement.yml"),
    )
    .unwrap();
    assert!(
        !written.contains("label:"),
        "an external Requirement must carry no label: {written}"
    );
    assert!(written.contains("source_revision: "), "{written}");

    let validate_output = run(&["validate", "--dir", dir.path().to_str().unwrap(), "--json"]);
    assert_eq!(
        validate_output.status.code(),
        Some(0),
        "{validate_output:?}"
    );
}

/// The companion to `a_new_external_requirement_is_written_in_a_state_project_validation_accepts`,
/// for the case that keeps producing invalid files: switching a saved
/// Requirement between ADR 0023's two modes. Whatever the switch writes
/// must still satisfy `markharness validate`.
#[test]
fn switching_a_requirement_between_modes_stays_valid_for_project_validation() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        run(&["init", "--dir", dir.path().to_str().unwrap()])
            .status
            .success()
    );
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.email", "test@example.com"],
        vec!["config", "user.name", "Test"],
    ] {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(&args)
                .status()
                .unwrap()
                .success()
        );
    }
    fs::write(
        dir.path().join("spec.sdoc"),
        "the external spec
",
    )
    .unwrap();

    let intent_path = dir.path().join("intent.yml");
    let reconcile = |path: &std::path::Path| {
        run(&[
            "knowledge",
            "reconcile",
            path.to_str().unwrap(),
            "--dir",
            dir.path().to_str().unwrap(),
        ])
    };
    let validate = || run(&["validate", "--dir", dir.path().to_str().unwrap(), "--json"]);

    fs::write(
        &intent_path,
        "format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - id: controls
    source: external
    axis: []
    source_locator: spec.sdoc
    source_revision: current
",
    )
    .unwrap();
    assert_eq!(reconcile(&intent_path).status.code(), Some(0));
    let uid = markharness::knowledge::parse_requirement(
        &fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/requirements/controls/requirement.yml"),
        )
        .unwrap(),
    )
    .unwrap()
    .uid
    .unwrap();

    // external -> native: the external mode stores no label, so the switch
    // has to be given one, and the pin must not survive it.
    fs::write(
        &intent_path,
        format!(
            "format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - uid: {uid}
    source: native
    label: controls
"
        ),
    )
    .unwrap();
    let output = reconcile(&intent_path);
    assert_eq!(output.status.code(), Some(0), "{output:?}");

    let written = fs::read_to_string(
        dir.path()
            .join(".markharness/knowledge/requirements/controls/requirement.yml"),
    )
    .unwrap();
    assert!(written.contains("label: controls"), "{written}");
    assert!(!written.contains("source_locator:"), "{written}");
    assert!(!written.contains("source_revision:"), "{written}");

    let validate_output = validate();
    assert_eq!(
        validate_output.status.code(),
        Some(0),
        "{validate_output:?}"
    );
}
