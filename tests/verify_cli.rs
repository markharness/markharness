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

fn run_git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

/// Commits with an explicit committer date so `order_by_recency` sees a
/// deterministic ordering, regardless of how fast this test actually runs
/// (back-to-back commits can otherwise land in the same wall-clock second,
/// which tie-breaks to name order and silently inverts "newest").
fn commit_all_with_date(root: &Path, message: &str, hour_offset: u32) {
    run_git(root, &["add", "-A"]);
    let date = format!("2026-01-01T{hour_offset:02}:00:00+00:00");
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["commit", "-q", "-m", message])
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_DATE", &date)
        .status()
        .unwrap();
    assert!(status.success(), "git commit failed");
}

/// A full Requirement->Feature->Behavior->Condition->ExpectedResult chain
/// (`generate`'s structural input) whose Feature label is the only thing
/// that changes between v1/v2, so the resulting `case_id` is always
/// `tc-req-todo-todo-edit-edit-existing-todo-edit-existing-todo`
/// (`generate::generate_testcases` derives `case_id` as
/// `tc-{requirement.id}-{feature.id}-{behavior.id}-{condition.id}`).
fn write_full_chain(root: &Path, label: &str) {
    let dir = root.join("knowledge/req-todo/todo-edit/edit-existing-todo");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        root.join("knowledge/req-todo/requirement.yml"),
        "id: req-todo\nlabel: req-todo\naxis: [ui]\n",
    )
    .unwrap();
    std::fs::write(
        root.join("knowledge/req-todo/todo-edit/feature.yml"),
        format!("id: todo-edit\nrequirement: req-todo\nlabel: {label}\naxis: [ui]\n"),
    )
    .unwrap();
    std::fs::write(
        dir.parent().unwrap().join("behavior.yml"),
        "id: edit-existing-todo\nfeature: todo-edit\nlabel: edit-existing-todo\naxis: [ui]\ndescription: |\n  User edits an existing todo.\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("condition.yml"),
        "id: edit-existing-todo\nbehavior: edit-existing-todo\nlabel: edit-existing-todo\ndescription: |\n  Title is changed.\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("expected")).unwrap();
    std::fs::write(
        dir.join("expected/001.yml"),
        "id: edit-existing-todo-001\ncondition: edit-existing-todo\ndescription: |\n  The todo is updated.\n",
    )
    .unwrap();
}

/// A project with two milestones (`test1`, `test2`) where `todo-edit`
/// changed between them, `changes/test2.yaml` computed via the real CLI
/// (`generate` + `changes compute`), impacting `tc-req-todo-todo-edit-edit-existing-todo-edit-existing-todo`.
fn init_project_with_pending_change() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let output = run(&["init", "--dir", dir.path().to_str().unwrap()]);
    assert!(output.status.success());
    run_git(dir.path(), &["init", "-q"]);
    run_git(dir.path(), &["config", "user.email", "test@example.com"]);
    run_git(dir.path(), &["config", "user.name", "Test"]);

    write_full_chain(dir.path(), "v1");
    commit_all_with_date(dir.path(), "v1", 1);
    run_git(dir.path(), &["tag", "test1"]);
    let init1 = run(&[
        "milestone",
        "init",
        "test1",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);
    assert!(init1.status.success(), "{init1:?}");

    write_full_chain(dir.path(), "v2");
    commit_all_with_date(dir.path(), "v2", 2);
    run_git(dir.path(), &["tag", "test2"]);
    let init2 = run(&[
        "milestone",
        "init",
        "test2",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);
    assert!(init2.status.success(), "{init2:?}");

    let generate = Command::new(bin())
        .arg("generate")
        .current_dir(dir.path())
        .output()
        .expect("failed to run markharness binary");
    assert!(generate.status.success(), "{generate:?}");
    assert!(
        dir.path()
            .join(
                "generated/testcases/req-todo/todo-edit/edit-existing-todo/edit-existing-todo.yml"
            )
            .is_file()
    );

    let compute = run(&[
        "changes",
        "compute",
        "test1",
        "test2",
        "--dir",
        dir.path().to_str().unwrap(),
        "--no-cache",
    ]);
    assert!(compute.status.success(), "{compute:?}");
    let changes_content = std::fs::read_to_string(dir.path().join("changes/test2.yaml")).unwrap();
    assert!(
        changes_content.contains("tc-req-todo-todo-edit-edit-existing-todo-edit-existing-todo"),
        "expected changes/test2.yaml to impact tc-req-todo-todo-edit-edit-existing-todo-edit-existing-todo, got:\n{changes_content}"
    );

    dir
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

#[test]
fn verify_pending_reports_the_impacted_testcase_before_reexecution() {
    let dir = init_project_with_pending_change();

    let output = run(&[
        "verify",
        "pending",
        "--from",
        "test1",
        "--to",
        "test2",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("tc-req-todo-todo-edit-edit-existing-todo-edit-existing-todo"),
        "unexpected stdout: {stdout}"
    );
    assert!(stdout.contains("pending"), "unexpected stdout: {stdout}");
}

#[test]
fn verify_pending_fail_on_pending_exits_one_when_pending_exists() {
    let dir = init_project_with_pending_change();

    let output = run(&[
        "verify",
        "pending",
        "--from",
        "test1",
        "--to",
        "test2",
        "--fail-on-pending",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);

    assert_eq!(output.status.code(), Some(1), "{output:?}");
}

#[test]
fn verify_pending_exits_two_when_no_milestone_pair_is_available() {
    let dir = tempfile::tempdir().unwrap();
    let init = run(&["init", "--dir", dir.path().to_str().unwrap()]);
    assert!(init.status.success());

    let output = run(&["verify", "pending", "--dir", dir.path().to_str().unwrap()]);

    assert_eq!(output.status.code(), Some(2), "{output:?}");
}

#[test]
fn verify_trace_reports_the_reflected_change_after_reexecution() {
    let dir = init_project_with_pending_change();

    let record = run(&[
        "execution",
        "record",
        "tc-req-todo-todo-edit-edit-existing-todo-edit-existing-todo",
        "--milestone",
        "test2",
        "--result",
        "pass",
        "--executor",
        "yamada",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);
    assert!(record.status.success(), "{record:?}");

    let output = run(&[
        "verify",
        "trace",
        "tc-req-todo-todo-edit-edit-existing-todo-edit-existing-todo",
        "--milestone",
        "test2",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("todo-edit--test1--test2"),
        "unexpected stdout: {stdout}"
    );
    assert!(
        stdout.contains("change_type: (未記録)"),
        "unexpected stdout: {stdout}"
    );

    let pending_after = run(&[
        "verify",
        "pending",
        "--from",
        "test1",
        "--to",
        "test2",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);
    let pending_stdout = String::from_utf8_lossy(&pending_after.stdout);
    assert!(
        !pending_stdout.contains("tc-req-todo-todo-edit-edit-existing-todo-edit-existing-todo"),
        "expected re-executed case to no longer be pending: {pending_stdout}"
    );
}

#[test]
fn verify_trace_exits_two_when_no_verified_blobs_recorded() {
    let dir = init_project_with_pending_change();

    let output = run(&[
        "verify",
        "trace",
        "tc-req-todo-todo-edit-edit-existing-todo-edit-existing-todo",
        "--milestone",
        "test2",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);

    assert_eq!(output.status.code(), Some(2), "{output:?}");
}
