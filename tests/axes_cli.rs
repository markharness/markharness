// Integration test fixtures write directly to a scratch repo before
// invoking the CLI binary; that's outside fs_safety's managed-root scope
// (see clippy.toml / src/lib.rs).
#![allow(clippy::disallowed_methods)]

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

fn init_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let output = run(&["init", "--dir", dir.path().to_str().unwrap()]);
    assert!(output.status.success());
    dir
}

#[test]
fn axes_add_creates_the_axis_file_with_label_defaulted_to_id() {
    let dir = init_project();

    let output = run(&[
        "axes",
        "add",
        "state",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);

    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("axes/state.yml")).unwrap(),
        "id: state\nlabel: state\n"
    );
}

#[test]
fn axes_add_json_reports_the_written_path() {
    let dir = init_project();

    let output = run(&[
        "axes",
        "add",
        "state",
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
    ]);

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["ok"], serde_json::json!(true));
    let written = parsed["written"].as_array().unwrap();
    assert!(
        written
            .iter()
            .any(|p| p.as_str().unwrap().ends_with("axes/state.yml")),
        "unexpected written list: {written:?}"
    );
}

#[test]
fn axes_prune_reports_an_unused_axis_and_leaves_it_in_place() {
    let dir = init_project();
    std::fs::write(
        dir.path().join("axes/orphan.yml"),
        "id: orphan\nlabel: orphan\n",
    )
    .unwrap();

    let output = run(&[
        "axes",
        "prune",
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
    ]);

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["axes"], serde_json::json!(["orphan"]));
    assert_eq!(parsed["deleted"], serde_json::json!(false));
    assert!(dir.path().join("axes/orphan.yml").exists());
}

#[test]
fn axes_prune_delete_removes_the_unused_axis_file() {
    let dir = init_project();
    std::fs::write(
        dir.path().join("axes/orphan.yml"),
        "id: orphan\nlabel: orphan\n",
    )
    .unwrap();

    let output = run(&[
        "axes",
        "prune",
        "--delete",
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
    ]);

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["axes"], serde_json::json!(["orphan"]));
    assert_eq!(parsed["deleted"], serde_json::json!(true));
    assert!(!dir.path().join("axes/orphan.yml").exists());
}

#[test]
fn axes_add_exits_two_and_does_not_overwrite_when_the_id_already_exists() {
    let dir = init_project();
    let first = run(&[
        "axes",
        "add",
        "state",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);
    assert!(first.status.success(), "{first:?}");

    let output = run(&[
        "axes",
        "add",
        "state",
        "--label",
        "Different label",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);

    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("already exists"),
        "unexpected stderr: {stderr}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("axes/state.yml")).unwrap(),
        "id: state\nlabel: state\n",
        "the pre-existing axis file must not be overwritten"
    );
}

#[test]
fn axes_add_exits_two_when_the_id_is_not_a_valid_slug() {
    let dir = init_project();

    let output = run(&[
        "axes",
        "add",
        "../../evil",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);

    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(!dir.path().join("axes/evil.yml").exists());
}

#[test]
fn axes_add_makes_the_axis_immediately_visible_to_axes_list() {
    let dir = init_project();
    let add = run(&[
        "axes",
        "add",
        "state",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);
    assert!(add.status.success(), "{add:?}");

    let output = run(&["axes", "list", "--dir", dir.path().to_str().unwrap()]);

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("state"), "unexpected stdout: {stdout}");
}
