// Integration test fixtures write directly to a scratch repo before
// invoking the CLI binary; that's outside fs_safety's managed-root scope
// (see clippy.toml / src/lib.rs).
#![allow(clippy::disallowed_methods)]

use std::fs;
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

const VALID_DRAFT: &str = "\
requirement:
  id: controls
  label: controls
  axis: [gameplay]

feature:
  id: player-jump
  label: player-jump
  axis: [gameplay, animation]

behavior:
  id: jump
  label: jump
  axis: [gameplay]
  description: Player presses jump.
  steps:
    - Press the jump button.

condition:
  id: ground
  label: ground
  description: Jump from the ground and land

expected:
  - description: lands safely
";

fn write_draft(dir: &Path, content: &str) -> std::path::PathBuf {
    let path = dir.join("draft.yml");
    fs::write(&path, content).unwrap();
    path
}

#[test]
fn scaffold_prints_the_blank_draft_template_to_stdout_by_default() {
    let output = run(&["knowledge", "scaffold"]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("requirement:"), "{stdout}");
    assert!(stdout.contains("condition:"), "{stdout}");
    assert!(stdout.contains("expected:"), "{stdout}");
}

#[test]
fn scaffold_out_writes_the_template_to_a_file_instead_of_stdout() {
    let dir = tempfile::tempdir().unwrap();
    let out_path = dir.path().join("draft.yml");

    let output = run(&["knowledge", "scaffold", "--out", out_path.to_str().unwrap()]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout).trim().is_empty(),
        "stdout should be silent when --out is given"
    );
    let written = fs::read_to_string(&out_path).unwrap();
    assert!(written.contains("requirement:"), "{written}");
}

#[test]
fn scaffold_out_refuses_to_overwrite_an_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let out_path = dir.path().join("draft.yml");
    fs::write(&out_path, "existing work in progress\n").unwrap();

    let output = run(&["knowledge", "scaffold", "--out", out_path.to_str().unwrap()]);

    assert!(!output.status.success(), "{output:?}");
    assert_eq!(
        fs::read_to_string(&out_path).unwrap(),
        "existing work in progress\n"
    );
}

#[test]
fn validate_exits_zero_and_prints_nothing_on_success() {
    let dir = setup_root_with_axes(&["gameplay", "animation"]);
    let draft_path = write_draft(dir.path(), VALID_DRAFT);

    let output = run(&[
        "knowledge",
        "validate",
        draft_path.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
    ]);

    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stdout).trim().is_empty());
}

#[test]
fn validate_json_prints_ok_true_on_success() {
    let dir = setup_root_with_axes(&["gameplay", "animation"]);
    let draft_path = write_draft(dir.path(), VALID_DRAFT);

    let output = run(&[
        "knowledge",
        "validate",
        draft_path.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "{\"ok\":true}");
}

#[test]
fn validate_exits_one_and_prints_human_error_on_validation_failure() {
    let dir = setup_root_with_axes(&["gameplay", "animation"]);
    let draft = VALID_DRAFT.replace("  description: Player presses jump.\n", "");
    let draft_path = write_draft(dir.path(), &draft);

    let output = run(&[
        "knowledge",
        "validate",
        draft_path.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
    ]);

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("error: missing_description:"),
        "stderr was: {stderr}"
    );
    assert!(stderr.contains("path=behavior.description"), "{stderr}");
}

#[test]
fn validate_json_prints_error_array_on_validation_failure() {
    let dir = setup_root_with_axes(&["gameplay", "animation"]);
    let draft = VALID_DRAFT.replace("  description: Player presses jump.\n", "");
    let draft_path = write_draft(dir.path(), &draft);

    let output = run(&[
        "knowledge",
        "validate",
        draft_path.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"ok\":false"), "{stdout}");
    assert!(
        stdout.contains("\"code\":\"missing_description\""),
        "{stdout}"
    );
    assert!(
        stdout.contains("\"path\":\"behavior.description\""),
        "{stdout}"
    );
}

#[test]
fn validate_exits_two_when_draft_file_does_not_exist() {
    let dir = setup_root_with_axes(&["gameplay", "animation"]);

    let output = run(&[
        "knowledge",
        "validate",
        dir.path().join("missing.yml").to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
    ]);

    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn validate_exits_two_when_draft_yaml_is_unparsable() {
    let dir = setup_root_with_axes(&["gameplay", "animation"]);
    let draft_path = write_draft(dir.path(), "requirement: [this is not a mapping");

    let output = run(&[
        "knowledge",
        "validate",
        draft_path.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
    ]);

    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn apply_exits_zero_and_writes_files_on_success() {
    let dir = setup_root_with_axes(&["gameplay", "animation"]);
    let draft_path = write_draft(dir.path(), VALID_DRAFT);

    let output = run(&[
        "knowledge",
        "apply",
        draft_path.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"ok\":true"), "{stdout}");
    assert!(stdout.contains("\"written\":["), "{stdout}");
    assert!(
        dir.path()
            .join(".markharness/knowledge/controls/player-jump/jump/ground/condition.yml")
            .exists()
    );
}

#[test]
fn apply_writes_a_multiline_description_that_reparses_and_validates() {
    let dir = tempfile::tempdir().unwrap();
    let init_output = run(&["init", "--dir", dir.path().to_str().unwrap()]);
    assert!(init_output.status.success(), "{init_output:?}");
    fs::write(
        dir.path().join(".markharness/axes/gameplay.yml"),
        "id: gameplay\nlabel: gameplay\n",
    )
    .unwrap();

    let draft = "\
requirement:
  id: controls
  label: controls
  axis: [gameplay]

feature:
  id: player-jump
  label: player-jump
  axis: [gameplay]

behavior:
  id: jump
  label: jump
  axis: [gameplay]
  description: |
    line one about foo.js: bar()
    line two about baz.js: qux()
  steps:
    - Press the jump button.

condition:
  id: ground
  label: ground
  description: Jump from the ground and land

expected:
  - description: lands safely
";
    let draft_path = write_draft(dir.path(), draft);

    let apply_output = run(&[
        "knowledge",
        "apply",
        draft_path.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(apply_output.status.code(), Some(0), "{apply_output:?}");

    let behavior_path = dir
        .path()
        .join(".markharness/knowledge/controls/player-jump/jump/behavior.yml");
    let written = fs::read_to_string(&behavior_path).unwrap();
    let behavior = markharness::knowledge::parse_behavior(&written).unwrap();
    assert_eq!(
        behavior.description,
        "line one about foo.js: bar()\nline two about baz.js: qux()\n"
    );

    let validate_output = run(&["validate", "--dir", dir.path().to_str().unwrap(), "--json"]);
    assert_eq!(
        validate_output.status.code(),
        Some(0),
        "{validate_output:?}"
    );
    let stdout = String::from_utf8_lossy(&validate_output.stdout);
    assert_eq!(stdout.trim(), "{\"ok\":true}");
}

#[test]
fn apply_exits_one_and_writes_nothing_on_validation_failure() {
    let dir = setup_root_with_axes(&["gameplay", "animation"]);
    let draft = VALID_DRAFT.replace("  description: Player presses jump.\n", "");
    let draft_path = write_draft(dir.path(), &draft);

    let output = run(&[
        "knowledge",
        "apply",
        draft_path.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
    ]);

    assert_eq!(output.status.code(), Some(1));
    assert!(!dir.path().join(".markharness/knowledge/controls").exists());
}

#[test]
fn apply_dry_run_validates_only_and_writes_nothing() {
    let dir = setup_root_with_axes(&["gameplay", "animation"]);
    let draft_path = write_draft(dir.path(), VALID_DRAFT);

    let output = run(&[
        "knowledge",
        "apply",
        draft_path.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--dry-run",
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "{\"ok\":true}");
    assert!(!dir.path().join(".markharness/knowledge/controls").exists());
}

const SECOND_CONDITION_REUSING_PARENT: &str = "\
requirement:
  id: controls

feature:
  id: player-jump

behavior:
  id: jump

condition:
  id: air
  label: air
  description: Jump in the air.

expected:
  - description: does not take fall damage
";

fn write_batch_draft(dir: &Path, name: &str, content: &str) {
    fs::write(dir.join(name), content).unwrap();
}

#[test]
fn apply_batch_applies_every_draft_in_file_name_order() {
    let dir = setup_root_with_axes(&["gameplay", "animation"]);
    let drafts_dir = dir.path().join("drafts");
    fs::create_dir_all(&drafts_dir).unwrap();
    write_batch_draft(&drafts_dir, "01-ground.yml", VALID_DRAFT);
    write_batch_draft(&drafts_dir, "02-air.yml", SECOND_CONDITION_REUSING_PARENT);

    let output = run(&[
        "knowledge",
        "apply",
        "--batch",
        drafts_dir.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"ok\":true"), "{stdout}");
    assert!(
        dir.path()
            .join(".markharness/knowledge/controls/player-jump/jump/ground/condition.yml")
            .exists()
    );
    assert!(
        dir.path()
            .join(".markharness/knowledge/controls/player-jump/jump/air/condition.yml")
            .exists()
    );
}

#[test]
fn apply_batch_writes_nothing_when_a_later_draft_is_invalid() {
    let dir = setup_root_with_axes(&["gameplay", "animation"]);
    let drafts_dir = dir.path().join("drafts");
    fs::create_dir_all(&drafts_dir).unwrap();
    write_batch_draft(&drafts_dir, "01-ground.yml", VALID_DRAFT);
    // A new condition ("air") missing its required description.
    write_batch_draft(
        &drafts_dir,
        "02-air.yml",
        "\
requirement:
  id: controls

feature:
  id: player-jump

behavior:
  id: jump

condition:
  id: air
  label: air
",
    );

    let output = run(&[
        "knowledge",
        "apply",
        "--batch",
        drafts_dir.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
    ]);

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("02-air.yml"), "{stderr}");
    assert!(
        !dir.path()
            .join(".markharness/knowledge/controls/player-jump/jump/ground/condition.yml")
            .exists(),
        "the first draft's files must be rolled back when the second draft is invalid"
    );
}

#[test]
fn apply_batch_json_reports_which_file_failed_validation() {
    let dir = setup_root_with_axes(&["gameplay", "animation"]);
    let drafts_dir = dir.path().join("drafts");
    fs::create_dir_all(&drafts_dir).unwrap();
    let draft = VALID_DRAFT.replace("  description: Player presses jump.\n", "");
    write_batch_draft(&drafts_dir, "01-ground.yml", &draft);

    let output = run(&[
        "knowledge",
        "apply",
        "--batch",
        drafts_dir.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"ok\":false"), "{stdout}");
    assert!(stdout.contains("\"file\":\"01-ground.yml\""), "{stdout}");
    assert!(
        stdout.contains("\"code\":\"missing_description\""),
        "{stdout}"
    );
}

#[test]
fn apply_batch_dry_run_validates_only_and_writes_nothing() {
    let dir = setup_root_with_axes(&["gameplay", "animation"]);
    let drafts_dir = dir.path().join("drafts");
    fs::create_dir_all(&drafts_dir).unwrap();
    write_batch_draft(&drafts_dir, "01-ground.yml", VALID_DRAFT);

    let output = run(&[
        "knowledge",
        "apply",
        "--batch",
        drafts_dir.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--dry-run",
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "{\"ok\":true}");
    assert!(!dir.path().join(".markharness/knowledge/controls").exists());
}

#[test]
fn validate_batch_exits_zero_and_writes_nothing_for_a_valid_batch() {
    let dir = setup_root_with_axes(&["gameplay", "animation"]);
    let drafts_dir = dir.path().join("drafts");
    fs::create_dir_all(&drafts_dir).unwrap();
    write_batch_draft(&drafts_dir, "01-ground.yml", VALID_DRAFT);

    let output = run(&[
        "knowledge",
        "validate",
        "--batch",
        drafts_dir.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "{\"ok\":true}");
    assert!(!dir.path().join(".markharness/knowledge/controls").exists());
}

#[test]
fn validate_batch_lets_a_later_draft_reuse_a_parent_an_earlier_draft_creates() {
    let dir = setup_root_with_axes(&["gameplay", "animation"]);
    let drafts_dir = dir.path().join("drafts");
    fs::create_dir_all(&drafts_dir).unwrap();
    write_batch_draft(&drafts_dir, "01-ground.yml", VALID_DRAFT);
    write_batch_draft(&drafts_dir, "02-air.yml", SECOND_CONDITION_REUSING_PARENT);

    let output = run(&[
        "knowledge",
        "validate",
        "--batch",
        drafts_dir.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "{\"ok\":true}");
}

#[test]
fn validate_batch_json_reports_every_failing_file_not_just_the_first() {
    let dir = setup_root_with_axes(&["gameplay", "animation"]);
    let drafts_dir = dir.path().join("drafts");
    fs::create_dir_all(&drafts_dir).unwrap();
    write_batch_draft(&drafts_dir, "01-broken.yml", "not: [valid yaml");
    let missing_description = VALID_DRAFT.replace("  description: Player presses jump.\n", "");
    write_batch_draft(&drafts_dir, "02-invalid.yml", &missing_description);

    let output = run(&[
        "knowledge",
        "validate",
        "--batch",
        drafts_dir.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"ok\":false"), "{stdout}");
    assert!(stdout.contains("\"file\":\"01-broken.yml\""), "{stdout}");
    assert!(stdout.contains("\"file\":\"02-invalid.yml\""), "{stdout}");
    assert!(
        stdout.contains("\"code\":\"missing_description\""),
        "{stdout}"
    );
}

#[test]
fn apply_batch_dry_run_json_reports_every_failing_file_not_just_the_first() {
    let dir = setup_root_with_axes(&["gameplay", "animation"]);
    let drafts_dir = dir.path().join("drafts");
    fs::create_dir_all(&drafts_dir).unwrap();
    write_batch_draft(&drafts_dir, "01-broken.yml", "not: [valid yaml");
    let missing_description = VALID_DRAFT.replace("  description: Player presses jump.\n", "");
    write_batch_draft(&drafts_dir, "02-invalid.yml", &missing_description);

    let output = run(&[
        "knowledge",
        "apply",
        "--batch",
        drafts_dir.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--dry-run",
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"file\":\"01-broken.yml\""), "{stdout}");
    assert!(stdout.contains("\"file\":\"02-invalid.yml\""), "{stdout}");
    assert!(!dir.path().join(".markharness/knowledge/controls").exists());
}

#[test]
fn apply_batch_exits_two_when_the_directory_has_no_yml_files() {
    let dir = setup_root_with_axes(&["gameplay", "animation"]);
    let drafts_dir = dir.path().join("drafts");
    fs::create_dir_all(&drafts_dir).unwrap();

    let output = run(&[
        "knowledge",
        "apply",
        "--batch",
        drafts_dir.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
    ]);

    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no *.yml files"), "{stderr}");
    assert!(
        stderr.contains(&drafts_dir.to_string_lossy().to_string()),
        "{stderr}"
    );
}

#[test]
fn apply_batch_json_reports_error_when_the_directory_has_no_yml_files() {
    let dir = setup_root_with_axes(&["gameplay", "animation"]);
    let drafts_dir = dir.path().join("drafts");
    fs::create_dir_all(&drafts_dir).unwrap();
    // A .yaml file must not satisfy the .yml-only batch convention.
    fs::write(drafts_dir.join("draft.yaml"), VALID_DRAFT).unwrap();

    let output = run(&[
        "knowledge",
        "apply",
        "--batch",
        drafts_dir.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"ok\":false"), "{stdout}");
    assert!(stdout.contains("\"error\":"), "{stdout}");
    assert!(!dir.path().join(".markharness/knowledge/controls").exists());
}

#[test]
fn apply_batch_dry_run_exits_two_when_the_directory_has_no_yml_files() {
    let dir = setup_root_with_axes(&["gameplay", "animation"]);
    let drafts_dir = dir.path().join("drafts");
    fs::create_dir_all(&drafts_dir).unwrap();

    let output = run(&[
        "knowledge",
        "apply",
        "--batch",
        drafts_dir.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--dry-run",
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(2), "{output:?}");
}

#[test]
fn validate_batch_exits_two_when_the_directory_has_no_yml_files() {
    let dir = setup_root_with_axes(&["gameplay", "animation"]);
    let drafts_dir = dir.path().join("drafts");
    fs::create_dir_all(&drafts_dir).unwrap();

    let output = run(&[
        "knowledge",
        "validate",
        "--batch",
        drafts_dir.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"ok\":false"), "{stdout}");
}

#[test]
fn apply_batch_and_draft_file_are_mutually_exclusive() {
    let dir = setup_root_with_axes(&["gameplay", "animation"]);
    let draft_path = write_draft(dir.path(), VALID_DRAFT);
    let drafts_dir = dir.path().join("drafts");
    fs::create_dir_all(&drafts_dir).unwrap();

    let output = run(&[
        "knowledge",
        "apply",
        draft_path.to_str().unwrap(),
        "--batch",
        drafts_dir.to_str().unwrap(),
    ]);

    assert!(!output.status.success());
}

#[test]
fn apply_strip_redundant_prefix_strips_condition_id_and_succeeds() {
    let dir = setup_root_with_axes(&["gameplay", "animation"]);
    let draft = VALID_DRAFT.replace("id: ground", "id: jump-ground");
    let draft_path = write_draft(dir.path(), &draft);

    let output = run(&[
        "knowledge",
        "apply",
        draft_path.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
        "--strip-redundant-prefix",
    ]);

    assert_eq!(output.status.code(), Some(0));
    assert!(
        dir.path()
            .join(".markharness/knowledge/controls/player-jump/jump/ground/condition.yml")
            .exists()
    );
}
