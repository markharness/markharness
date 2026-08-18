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
    std::fs::create_dir_all(repo.path().join("knowledge/shop/checkout")).unwrap();
    std::fs::write(
        repo.path().join("knowledge/shop/requirement.yml"),
        "id: shop\nlabel: Shop\naxis: []\n",
    )
    .unwrap();
    std::fs::write(
        repo.path().join("knowledge/shop/checkout/feature.yml"),
        "id: checkout\nrequirement: shop\nlabel: Checkout\naxis: []\n",
    )
    .unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "fixture"]);
    std::fs::create_dir_all(repo.path().join("changes")).unwrap();
    std::fs::write(repo.path().join("changes/head.yaml"), "- event_id: checkout--base--head\n  feature_id: checkout\n  from_milestone: base\n  to_milestone: head\n  from_tree_sha: old\n  to_tree_sha: new\n  impacted_testcases: [tc-checkout]\n").unwrap();
    std::fs::create_dir_all(repo.path().join("executions/head")).unwrap();
    std::fs::write(repo.path().join("executions/head/results.yml"), "- case_id: tc-checkout\n  result: pass\n  executor: ci\n  executed_at: 2026-08-18T10:00:00Z\n  verified_feature_tree_shas:\n    checkout: new\n").unwrap();

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
