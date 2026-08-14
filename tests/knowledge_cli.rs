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
    fs::create_dir_all(dir.path().join("knowledge")).unwrap();
    fs::create_dir_all(dir.path().join("axes")).unwrap();
    for id in axis_ids {
        fs::write(
            dir.path().join("axes").join(format!("{id}.yml")),
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
    let draft = VALID_DRAFT.replace("description: Player presses jump.\n", "");
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
    let draft = VALID_DRAFT.replace("description: Player presses jump.\n", "");
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
            .join("knowledge/controls/player-jump/jump/ground/condition.yml")
            .exists()
    );
}

#[test]
fn apply_exits_one_and_writes_nothing_on_validation_failure() {
    let dir = setup_root_with_axes(&["gameplay", "animation"]);
    let draft = VALID_DRAFT.replace("description: Player presses jump.\n", "");
    let draft_path = write_draft(dir.path(), &draft);

    let output = run(&[
        "knowledge",
        "apply",
        draft_path.to_str().unwrap(),
        "--dir",
        dir.path().to_str().unwrap(),
    ]);

    assert_eq!(output.status.code(), Some(1));
    assert!(!dir.path().join("knowledge/controls").exists());
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
    assert!(!dir.path().join("knowledge/controls").exists());
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
            .join("knowledge/controls/player-jump/jump/ground/condition.yml")
            .exists()
    );
}
