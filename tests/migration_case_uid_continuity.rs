#![allow(clippy::disallowed_methods)]
//! Step 32's golden fixture: a full pre-migration -> migrate ->
//! post-migration-rename lifecycle, proving that legacy identifiers
//! (a `case_id` string recorded before a rename, a `ChangeEvent` spanning
//! the rename) still resolve to the same identity afterward — via
//! `uid`-based `ChangeEvent` tracking (ADR 0013, established in earlier
//! phases) for Features, and via the migration manifest (Step 31) for
//! TestCases, which have no identity event log of their own.

use std::fs;
use std::path::Path;
use std::process::Command;

use markharness::changes::{ChangeOptions, compute_changes};
use markharness::execution::{ExecutionResult, RecordArgs, read_all_results, record_execution};
use markharness::generate::{generate_testcases, serialize_testcase};
use markharness::identity::{self, migration_manifest};

mod common;
use common::write_full_tree;

fn run_git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

fn commit_and_tag_milestone(root: &Path, milestone: &str, hour_offset: u32) {
    fs::create_dir_all(root.join(".markharness/executions").join(milestone)).unwrap();
    fs::write(
        root.join(".markharness/executions")
            .join(milestone)
            .join("milestone.yml"),
        format!("id: {milestone}\n"),
    )
    .unwrap();
    run_git(root, &["add", "-A"]);
    let date = format!("2026-01-01T{hour_offset:02}:00:00+00:00");
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["commit", "-q", "-m", milestone])
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_DATE", &date)
        .status()
        .unwrap();
    assert!(status.success());
    run_git(root, &["tag", milestone]);
}

fn write_generated_testcase(root: &Path, testcase: &markharness::generate::TestCase) {
    let path = root
        .join(".markharness/generated/testcases")
        .join(testcase.relative_path());
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serialize_testcase(testcase)).unwrap();
}

#[test]
fn legacy_case_id_and_change_event_identity_survive_migration_and_a_later_rename() {
    let dir = tempfile::tempdir().unwrap();
    run_git(dir.path(), &["init", "-q"]);
    run_git(dir.path(), &["config", "user.email", "test@example.com"]);
    run_git(dir.path(), &["config", "user.name", "Test"]);

    // 1. Pre-migration: no uid anywhere. Record an execution under the
    // legacy case_id, matching what a real project's history looks like
    // before ADR 0013.
    write_full_tree(dir.path(), "todo");
    let pre_migration_testcases =
        generate_testcases(&dir.path().join(".markharness/knowledge")).unwrap();
    let legacy_case_id = pre_migration_testcases[0].case_id.clone();
    assert_eq!(pre_migration_testcases[0].case_uid, None);
    write_generated_testcase(dir.path(), &pre_migration_testcases[0]);
    commit_and_tag_milestone(dir.path(), "v1", 1);
    record_execution(
        dir.path(),
        &RecordArgs {
            milestone: "v1",
            case_id: &legacy_case_id,
            result: ExecutionResult::Pass,
            executor: "yamada",
            note: None,
        },
    )
    .unwrap();

    // 2. Migrate: every element gets a uid; the manifest records
    // legacy_case_id -> case_uid.
    let migrate_report = identity::migrate_entities(dir.path()).unwrap();
    assert_eq!(migrate_report.migrated.len(), 5);
    commit_and_tag_milestone(dir.path(), "v2", 2);

    let post_migrate_testcases =
        generate_testcases(&dir.path().join(".markharness/knowledge")).unwrap();
    assert_eq!(post_migrate_testcases[0].case_id, legacy_case_id);
    let case_uid = post_migrate_testcases[0]
        .case_uid
        .clone()
        .expect("case_uid must be computable once every element has a uid");

    let manifest = migration_manifest::read(dir.path()).unwrap();
    assert_eq!(
        migration_manifest::resolve_case_uid(&manifest, &legacy_case_id),
        Ok(Some(case_uid.as_str())),
        "the manifest must resolve the pre-migration case_id to the same case_uid"
    );

    // 3. Post-migration rename: the Feature's id changes, so the
    // TestCase's case_id string changes too (it embeds the Feature id) —
    // but case_uid must not, since it is a pure function of the five
    // uids, none of which a rename touches.
    identity::rename_id(dir.path(), "todo", "todo-v2").unwrap();
    commit_and_tag_milestone(dir.path(), "v3", 3);

    let post_rename_testcases =
        generate_testcases(&dir.path().join(".markharness/knowledge")).unwrap();
    let renamed_case_id = post_rename_testcases[0].case_id.clone();
    assert_ne!(
        renamed_case_id, legacy_case_id,
        "the rename must actually change the case_id string, or this test proves nothing"
    );
    assert_eq!(
        post_rename_testcases[0].case_uid.as_deref(),
        Some(case_uid.as_str()),
        "case_uid must stay identical across a rename of one of its contributing elements"
    );

    // 4. Refreshing the manifest (as a later `identity migrate` run would)
    // must add a *second* entry for the new case_id, without disturbing
    // the original — both now resolve to the same case_uid.
    identity::migrate_entities(dir.path()).unwrap();
    let refreshed_manifest = migration_manifest::read(dir.path()).unwrap();
    assert_eq!(
        migration_manifest::resolve_case_uid(&refreshed_manifest, &legacy_case_id),
        Ok(Some(case_uid.as_str())),
        "the original mapping must survive the refresh"
    );
    assert_eq!(
        migration_manifest::resolve_case_uid(&refreshed_manifest, &renamed_case_id),
        Ok(Some(case_uid.as_str())),
        "the post-rename case_id must resolve to the same case_uid as the pre-rename one"
    );

    // 5. The actual regression this manifest exists to prevent: the
    // execution recorded back in step 1, under `legacy_case_id`, at
    // milestone v1 — long before migration or the rename — must still
    // resolve (via the manifest) to the *same* case_uid the freshly
    // regenerated, post-rename TestCase resolves to. A tool correlating
    // "have we re-verified this TestCase since it last changed" has to be
    // able to make this connection, or the old execution looks like it
    // belongs to a TestCase that no longer exists.
    let all_results = read_all_results(dir.path()).unwrap();
    let legacy_execution = all_results
        .iter()
        .find(|entry| entry.case_id == legacy_case_id)
        .expect("the pre-migration execution recorded in step 1 must still be on record");
    let legacy_execution_case_uid =
        migration_manifest::resolve_case_uid(&refreshed_manifest, &legacy_execution.case_id)
            .unwrap()
            .expect("the legacy execution's case_id must resolve to a case_uid");
    let current_case_uid = post_rename_testcases[0]
        .case_uid
        .as_deref()
        .expect("the current, post-rename TestCase must have a case_uid");
    assert_eq!(
        legacy_execution_case_uid, current_case_uid,
        "an execution recorded under the pre-migration case_id must resolve to the same \
         case_uid as the current, post-rename TestCase"
    );

    // 6. The Feature rename between v2 and v3 (both post-migration) must
    // be tracked as a single ChangeEvent via its uid, not a delete+add —
    // reconfirming ADR 0013's core guarantee holds in this full lifecycle,
    // not just in isolation.
    let events = compute_changes(dir.path(), "v2", "v3", ChangeOptions::default()).unwrap();
    assert_eq!(
        events.len(),
        1,
        "expected the rename to produce a single ChangeEvent, got {events:?}"
    );
    assert_eq!(events[0].feature_id, "todo-v2");
    assert!(events[0].feature_uid.is_some());
}
