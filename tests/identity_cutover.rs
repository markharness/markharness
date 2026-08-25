#![allow(clippy::disallowed_methods)]
//! Step 35 (design doc §13 Phase 5, ADR 0013 「移行」節): end-to-end
//! coverage for the schema version 2 public cutover — all five
//! `EntityKind`s flip to UID mode together, as one operation, and the
//! `[identity] mode = "uid"` marker (not a count of migrated elements) is
//! what governs whether a uid-less element is legitimate pre-migration
//! state or a rejected data-integrity violation.

use std::path::Path;

use markharness::identity;
use markharness::validate;

mod common;
use common::write_full_tree;

fn init_git_repo(dir: &Path) {
    let status = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .status()
            .unwrap()
    };
    assert!(status(&["init", "-q"]).success());
    assert!(status(&["config", "user.email", "test@example.com"]).success());
    assert!(status(&["config", "user.name", "Test"]).success());
    assert!(status(&["config", "core.autocrlf", "false"]).success());
}

#[test]
fn cutover_flips_all_five_kinds_together_and_marks_the_project_marker() {
    let dir = tempfile::tempdir().unwrap();
    markharness::init::run_init(dir.path()).unwrap();
    init_git_repo(dir.path());
    write_full_tree(dir.path(), "todo");
    assert!(!identity::is_uid_mode(dir.path()).unwrap());

    let report = identity::migrate_entities(dir.path()).unwrap();

    let kinds: std::collections::BTreeSet<identity::EntityKind> =
        report.migrated.iter().map(|m| m.kind).collect();
    assert_eq!(
        kinds,
        identity::EntityKind::ALL.into_iter().collect(),
        "cutover must migrate every kind in the same operation, got {report:?}"
    );
    assert!(identity::is_uid_mode(dir.path()).unwrap());
    assert!(
        validate::validate_all(dir.path()).unwrap().is_empty(),
        "a freshly cut-over project must validate cleanly"
    );
}

#[test]
fn a_uid_less_element_added_after_cutover_is_rejected_by_validate_until_repaired() {
    let dir = tempfile::tempdir().unwrap();
    markharness::init::run_init(dir.path()).unwrap();
    init_git_repo(dir.path());
    write_full_tree(dir.path(), "todo");
    identity::migrate_entities(dir.path()).unwrap();
    assert!(identity::is_uid_mode(dir.path()).unwrap());

    // Simulates copy/import/hand-editing introducing a uid-less Feature
    // into an already cut-over project (ADR 0013 検証規則).
    std::fs::create_dir_all(
        dir.path()
            .join(".markharness/knowledge/req-todo/todo-again"),
    )
    .unwrap();
    std::fs::write(
        dir.path()
            .join(".markharness/knowledge/req-todo/todo-again/feature.yml"),
        "id: todo-again\nrequirement: req-todo\nlabel: todo again\naxis: []\n",
    )
    .unwrap();

    let issues = validate::validate_all(dir.path()).unwrap();
    assert!(
        issues
            .iter()
            .any(|i| i.path.contains("todo-again") && i.message.contains("identity migrate")),
        "expected a UID-mode violation naming the uid-less feature, got: {issues:?}"
    );

    // The documented repair path: re-run `identity migrate`, which is
    // still safe to call incrementally even after cutover.
    identity::migrate_entities(dir.path()).unwrap();

    assert!(
        validate::validate_all(dir.path()).unwrap().is_empty(),
        "repairing via identity migrate must clear the violation"
    );
}
