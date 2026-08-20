#![allow(clippy::disallowed_methods)]

use std::process::Command;

use markharness::derived_index::rebuild_indexes;

#[test]
fn indexes_are_reconstructible_from_git_changes_and_executions() {
    let repo = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(repo.path())
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.email", "test@example.com"]);
    git(&["config", "user.name", "Test"]);
    std::fs::create_dir_all(repo.path().join(".markharness/knowledge/shop/checkout")).unwrap();
    std::fs::write(
        repo.path()
            .join(".markharness/knowledge/shop/requirement.yml"),
        "id: shop\nlabel: Shop\naxis: []\n",
    )
    .unwrap();
    std::fs::write(
        repo.path()
            .join(".markharness/knowledge/shop/checkout/feature.yml"),
        "id: checkout\nrequirement: shop\nlabel: Checkout\naxis: []\n",
    )
    .unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "fixture"]);
    std::fs::create_dir_all(
        repo.path()
            .join(markharness::project_root::MARKHARNESS_DIR)
            .join("changes"),
    )
    .unwrap();
    std::fs::write(repo.path().join(".markharness/changes/head.yaml"), "- event_id: checkout--base--head\n  feature_id: checkout\n  from_milestone: base\n  to_milestone: head\n  from_tree_sha: old\n  to_tree_sha: new\n  impacted_testcases: [tc-checkout]\n").unwrap();
    std::fs::create_dir_all(repo.path().join(".markharness/executions/head")).unwrap();
    std::fs::write(repo.path().join(".markharness/executions/head/results.yml"), "- case_id: tc-checkout\n  result: pass\n  executor: ci\n  executed_at: 2026-08-18T10:00:00Z\n  verified_feature_tree_shas:\n    checkout: new\n").unwrap();

    let first = rebuild_indexes(repo.path(), "HEAD").unwrap();
    let first_bytes = std::fs::read(&first.change_events).unwrap();
    std::fs::remove_dir_all(repo.path().join(".markharness-cache/index")).unwrap();
    let second = rebuild_indexes(repo.path(), "HEAD").unwrap();

    assert_eq!(first_bytes, std::fs::read(&second.change_events).unwrap());
    let changes: serde_json::Value = serde_json::from_slice(&first_bytes).unwrap();
    assert_eq!(changes["by_feature"]["checkout"][0], "checkout--base--head");
    let executions: serde_json::Value =
        serde_json::from_slice(&std::fs::read(second.executions).unwrap()).unwrap();
    assert_eq!(executions["by_case"]["tc-checkout"][0]["result"], "pass");
}

/// ADR 0013: two ChangeEvents recorded for the same Feature before and
/// after a rename (uid preserved) must land under the same `by_feature`
/// key — the Feature's `uid` — instead of being split across two entries
/// keyed by the (now-diverging) `feature_id` strings.
#[test]
fn change_event_index_groups_by_uid_across_a_rename() {
    let repo = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(repo.path())
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.email", "test@example.com"]);
    git(&["config", "user.name", "Test"]);
    std::fs::create_dir_all(repo.path().join(".markharness/knowledge/shop/checkout")).unwrap();
    std::fs::write(
        repo.path()
            .join(".markharness/knowledge/shop/requirement.yml"),
        "id: shop\nlabel: Shop\naxis: []\n",
    )
    .unwrap();
    std::fs::write(
        repo.path()
            .join(".markharness/knowledge/shop/checkout/feature.yml"),
        "id: checkout\nrequirement: shop\nlabel: Checkout\naxis: []\n",
    )
    .unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "fixture"]);
    std::fs::create_dir_all(
        repo.path()
            .join(markharness::project_root::MARKHARNESS_DIR)
            .join("changes"),
    )
    .unwrap();
    const UID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    // Recorded pre-rename: feature_id "checkout".
    std::fs::write(
        repo.path().join(".markharness/changes/m1.yaml"),
        format!(
            "- event_id: checkout--base--m1\n  feature_id: checkout\n  feature_uid: {UID}\n  from_milestone: base\n  to_milestone: m1\n  from_tree_sha: old\n  to_tree_sha: mid\n  impacted_testcases: []\n"
        ),
    )
    .unwrap();
    // Recorded post-rename: feature_id "checkout-flow", same uid.
    std::fs::write(
        repo.path().join(".markharness/changes/m2.yaml"),
        format!(
            "- event_id: checkout-flow--m1--m2\n  feature_id: checkout-flow\n  feature_uid: {UID}\n  from_milestone: m1\n  to_milestone: m2\n  from_tree_sha: mid\n  to_tree_sha: new\n  impacted_testcases: []\n"
        ),
    )
    .unwrap();

    let paths = rebuild_indexes(repo.path(), "HEAD").unwrap();

    let changes: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&paths.change_events).unwrap()).unwrap();
    let by_feature = changes["by_feature"].as_object().unwrap();
    assert_eq!(
        by_feature.len(),
        1,
        "expected both events under one uid-keyed entry, got {by_feature:?}"
    );
    let event_ids = by_feature[UID].as_array().unwrap();
    assert_eq!(
        event_ids,
        &vec![
            serde_json::Value::String("checkout--base--m1".to_string()),
            serde_json::Value::String("checkout-flow--m1--m2".to_string()),
        ]
    );
}
