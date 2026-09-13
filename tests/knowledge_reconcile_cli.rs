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
fn valid_intent_with_json_reports_ok_true() {
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
    assert_eq!(stdout.trim(), "{\"ok\":true}");
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
