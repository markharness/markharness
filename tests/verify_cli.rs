// Integration test fixtures write directly to a scratch repo before
// invoking the CLI binary; that's outside fs_safety's managed-root scope
// (see clippy.toml / src/lib.rs).
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

/// A full Requirement->Feature->Behavior->Condition->ExpectedResult chain
/// (`generate`'s structural input) whose Feature label is the only thing
/// that changes between v1/v2, so the resulting `case_id` is always
/// `tc-todo-edit-edit-existing-todo-edit-existing-todo`
/// (`generate::generate_testcases` derives `case_id` as
/// `tc-{feature.id}-{behavior.id}-{condition.id}`).
fn write_full_chain(root: &Path, label: &str) {
    let dir = root.join(".markharness/knowledge/features/todo-edit/edit-existing-todo");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(root.join(".markharness/knowledge/requirements/req-todo")).unwrap();
    std::fs::write(
        root.join(".markharness/knowledge/requirements/req-todo/requirement.yml"),
        "id: req-todo\nsource: native\nlabel: req-todo\naxis: [ui]\nuid: 01ARZ3NDEKTSV4RRFFQ69G5FAV\n",
    )
    .unwrap();
    std::fs::write(
        root.join(".markharness/knowledge/features/todo-edit/feature.yml"),
        format!(
            "id: todo-edit\nrequirement_uids: [01ARZ3NDEKTSV4RRFFQ69G5FAV]\nlabel: {label}\naxis: [ui]\n"
        ),
    )
    .unwrap();
    std::fs::write(
        dir.parent().unwrap().join("behavior.yml"),
        "id: edit-existing-todo\nfeature: todo-edit\nlabel: edit-existing-todo\naxis: [ui]\ndescription: |\n  User edits an existing todo.\nprocedures: {}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("scenario.yml"),
        "id: edit-existing-todo\nbehavior: edit-existing-todo\nlabel: edit-existing-todo\ndescription: |\n  Title is changed.\nphases:\n  - steps:\n      - action: \"Do it.\"\n    results:\n      - \"The todo is updated.\"\n",
    )
    .unwrap();
}

#[test]
fn bare_verify_still_reports_up_to_date_when_generated_matches_knowledge() {
    let dir = tempfile::tempdir().unwrap();
    let init = run(&["init", "--dir", dir.path().to_str().unwrap()]);
    assert!(init.status.success());
    let generate = Command::new(bin())
        .arg("generate")
        .current_dir(dir.path())
        .output()
        .expect("failed to run markharness binary");
    assert!(generate.status.success(), "{generate:?}");

    let output = Command::new(bin())
        .arg("verify")
        .current_dir(dir.path())
        .output()
        .expect("failed to run markharness binary");

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("up to date"), "unexpected stdout: {stdout}");
}

/// Step C: bare `verify` used to only accept `env::current_dir()` and had no
/// `--dir`, forcing callers to `cd` into the target project first (the same
/// gap `generate` had before Step B).
#[test]
fn bare_verify_accepts_a_dir_option_targeting_a_directory_other_than_cwd() {
    let dir = tempfile::tempdir().unwrap();
    let init = run(&["init", "--dir", dir.path().to_str().unwrap()]);
    assert!(init.status.success());
    let generate = run(&["generate", "--dir", dir.path().to_str().unwrap()]);
    assert!(generate.status.success(), "{generate:?}");

    let unrelated_cwd = tempfile::tempdir().unwrap();
    let output = Command::new(bin())
        .args(["verify", "--dir", dir.path().to_str().unwrap()])
        .current_dir(unrelated_cwd.path())
        .output()
        .expect("failed to run markharness binary");

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("up to date"), "unexpected stdout: {stdout}");
}

#[test]
fn bare_verify_json_reports_would_change_false_when_up_to_date() {
    let dir = tempfile::tempdir().unwrap();
    let init = run(&["init", "--dir", dir.path().to_str().unwrap()]);
    assert!(init.status.success());
    let generate = run(&["generate", "--dir", dir.path().to_str().unwrap()]);
    assert!(generate.status.success(), "{generate:?}");

    let output = run(&["verify", "--dir", dir.path().to_str().unwrap(), "--json"]);

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["would_change"], serde_json::json!(false));
    assert_eq!(parsed["added"], serde_json::json!([]));
    assert_eq!(parsed["changed"], serde_json::json!([]));
    assert_eq!(parsed["removed"], serde_json::json!([]));
}

#[test]
fn bare_verify_json_reports_added_files_when_generated_is_stale() {
    let dir = tempfile::tempdir().unwrap();
    let init = run(&["init", "--dir", dir.path().to_str().unwrap()]);
    assert!(init.status.success());
    write_full_chain(dir.path(), "v1");

    let output = run(&["verify", "--dir", dir.path().to_str().unwrap(), "--json"]);

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["would_change"], serde_json::json!(true));
    let added = parsed["added"].as_array().unwrap();
    assert!(
        added
            .iter()
            .any(|p| p.as_str().unwrap().ends_with("edit-existing-todo.yml")),
        "unexpected added list: {added:?}"
    );
    assert!(
        added
            .iter()
            .any(|p| p.as_str().unwrap() == "traceability-index.json"),
        "unexpected added list: {added:?}"
    );
}
