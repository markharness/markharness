// Integration test fixtures write directly to a scratch repo before
// invoking the CLI binary; that's outside fs_safety's managed-root scope
// (see clippy.toml / src/lib.rs).
#![allow(clippy::disallowed_methods)]

use std::process::{Command, Output};

mod common;
use common::write_full_tree;

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
    // `identity migrate` -> `migration_manifest::capture_case_signatures`
    // -> `git::write_tree_prefix` requires an actual git repository.
    let git_status = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(args)
            .status()
            .unwrap()
    };
    assert!(git_status(&["init", "-q"]).success());
    assert!(git_status(&["config", "user.email", "test@example.com"]).success());
    assert!(git_status(&["config", "user.name", "Test"]).success());
    dir
}

#[test]
fn identity_migrate_json_reports_kind_id_and_uid_for_every_migrated_element() {
    let dir = init_project();
    write_full_tree(dir.path(), "todo");

    let output = run(&[
        "identity",
        "migrate",
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
    ]);

    assert!(output.status.success(), "{output:?}");
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(body["dry_run"], false);
    let migrated = body["migrated"].as_array().unwrap();
    assert_eq!(migrated.len(), 5);
    let kinds: std::collections::BTreeSet<&str> = migrated
        .iter()
        .map(|entry| entry["kind"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        std::collections::BTreeSet::from([
            "requirement",
            "feature",
            "behavior",
            "condition",
            "expected_result",
        ])
    );
    for entry in migrated {
        assert!(entry["id"].as_str().is_some());
        assert!(entry["uid"].as_str().is_some());
    }
    assert!(body["conflicts"].as_array().unwrap().is_empty());
}

#[test]
fn identity_migrate_human_readable_output_names_every_kind() {
    let dir = init_project();
    write_full_tree(dir.path(), "todo");

    let output = run(&["identity", "migrate", "--dir", dir.path().to_str().unwrap()]);

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    for expected in [
        "migrated requirement 'req-todo'",
        "migrated feature 'todo'",
        "migrated behavior 'todo-add-task'",
        "migrated condition 'todo-add-task-empty-input'",
        "migrated expected_result 'todo-add-task-empty-input-001'",
    ] {
        assert!(
            stdout.contains(expected),
            "expected stdout to contain '{expected}', got: {stdout}"
        );
    }
}

#[test]
fn identity_migrate_dry_run_writes_nothing() {
    let dir = init_project();
    write_full_tree(dir.path(), "todo");

    let output = run(&[
        "identity",
        "migrate",
        "--dir",
        dir.path().to_str().unwrap(),
        "--dry-run",
        "--json",
    ]);

    assert!(output.status.success(), "{output:?}");
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(body["dry_run"], true);
    assert_eq!(body["migrated"].as_array().unwrap().len(), 5);

    assert!(!dir.path().join(".markharness/identity-events").exists());
    assert!(
        !dir.path()
            .join(".markharness/identity-migration-manifest.yml")
            .exists()
    );
    let feature_yml = std::fs::read_to_string(
        dir.path()
            .join(".markharness/knowledge/req-todo/todo/feature.yml"),
    )
    .unwrap();
    assert!(!feature_yml.contains("uid:"));
}

#[test]
fn identity_migrate_reports_no_op_when_everything_is_already_migrated() {
    let dir = init_project();
    write_full_tree(dir.path(), "todo");
    let first = run(&["identity", "migrate", "--dir", dir.path().to_str().unwrap()]);
    assert!(first.status.success(), "{first:?}");

    let second = run(&[
        "identity",
        "migrate",
        "--dir",
        dir.path().to_str().unwrap(),
        "--json",
    ]);

    assert!(second.status.success(), "{second:?}");
    let body: serde_json::Value = serde_json::from_slice(&second.stdout).unwrap();
    assert!(body["migrated"].as_array().unwrap().is_empty());
    assert!(body["conflicts"].as_array().unwrap().is_empty());
}

#[test]
fn identity_migrate_exits_nonzero_and_reports_conflicts_for_a_duplicate_id_within_one_kind() {
    let dir = init_project();
    write_full_tree(dir.path(), "todo");
    // A second Feature reusing "todo" as its id, under the same
    // Requirement — a duplicate within the Feature kind.
    std::fs::create_dir_all(
        dir.path()
            .join(".markharness/knowledge/req-todo/todo-again"),
    )
    .unwrap();
    std::fs::write(
        dir.path()
            .join(".markharness/knowledge/req-todo/todo-again/feature.yml"),
        "id: todo\nrequirement: req-todo\nlabel: todo again\naxis: []\n",
    )
    .unwrap();

    let output = run(&["identity", "migrate", "--dir", dir.path().to_str().unwrap()]);

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("duplicate feature id 'todo'"),
        "unexpected stderr: {stderr}"
    );
    assert!(!dir.path().join(".markharness/identity-events").exists());
}
