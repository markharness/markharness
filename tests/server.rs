use std::path::Path;
use std::process::Command;

use markharness::server::{DashboardConfig, handle_request};

fn write(root: &Path, relative: &str, contents: &str) {
    markharness::fs_safety::replace_file(root, &root.join(relative), contents.as_bytes()).unwrap();
}

fn git(root: &Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .unwrap()
            .success()
    );
}

fn repository() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["config", "user.email", "test@example.com"]);
    git(dir.path(), &["config", "user.name", "Test"]);
    write(
        dir.path(),
        ".markharness/knowledge/checkout/requirement.yml",
        "id: checkout\nlabel: Checkout\naxis: [commerce]\n",
    );
    write(
        dir.path(),
        ".markharness/knowledge/checkout/pay/feature.yml",
        "id: pay\nrequirement: checkout\nlabel: Pay\naxis: [commerce]\n",
    );
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-qm", "base"]);
    write(
        dir.path(),
        ".markharness/knowledge/checkout/pay/feature.yml",
        "id: pay\nrequirement: checkout\nlabel: Pay securely\naxis: [commerce]\n",
    );
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-qm", "head"]);
    dir
}

#[test]
fn serves_embedded_dashboard_assets() {
    let dir = repository();
    let response = handle_request(dir.path(), "GET", "/", &DashboardConfig::default());

    assert_eq!(response.status, 200);
    assert_eq!(response.content_type, "text/html; charset=utf-8");
    assert!(
        String::from_utf8(response.body)
            .unwrap()
            .contains("Release Verification")
    );
}

#[test]
fn config_api_exposes_the_cli_selected_initial_range() {
    let dir = repository();
    let response = handle_request(
        dir.path(),
        "GET",
        "/api/config",
        &DashboardConfig {
            base: "main".to_string(),
            head: "feature".to_string(),
        },
    );

    let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(body["base"], "main");
    assert_eq!(body["head"], "feature");
}

#[test]
fn plan_api_returns_the_domain_plan_contract_without_writing_repository_files() {
    let dir = repository();
    let before = git_managed_diff(dir.path());
    let response = handle_request(
        dir.path(),
        "GET",
        "/api/plan?base=HEAD~1&head=HEAD",
        &DashboardConfig::default(),
    );

    assert_eq!(response.status, 200);
    let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(body["schema_version"], 1);
    assert_eq!(body["summary"]["changed_features"], 1);
    assert_eq!(before, git_managed_diff(dir.path()));
}

#[test]
fn feature_history_api_reports_versions_and_change_events() {
    let dir = repository();
    let response = handle_request(
        dir.path(),
        "GET",
        "/api/features?ref=HEAD",
        &DashboardConfig::default(),
    );

    assert_eq!(response.status, 200);
    let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(body["features"][0]["id"], "pay");
    assert!(body["features"][0]["tree_sha"].as_str().unwrap().len() >= 40);
}

/// ADR 0013: `change_events` for a Feature must include events recorded
/// under an earlier `feature_id`, when the Feature's `uid` is preserved
/// across the rename — not just events matching the *current* id.
#[test]
fn feature_history_api_includes_change_events_recorded_before_a_uid_preserving_rename() {
    const UID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["config", "user.email", "test@example.com"]);
    git(dir.path(), &["config", "user.name", "Test"]);
    write(
        dir.path(),
        ".markharness/knowledge/checkout/requirement.yml",
        "id: checkout\nlabel: Checkout\naxis: []\n",
    );
    write(
        dir.path(),
        ".markharness/knowledge/checkout/pay/feature.yml",
        &format!("id: pay\nrequirement: checkout\nlabel: Pay\naxis: []\nuid: {UID}\n"),
    );
    write(
        dir.path(),
        ".markharness/changes/m1.yaml",
        &format!(
            "- event_id: pay--base--m1\n  feature_id: pay\n  feature_uid: {UID}\n  from_milestone: base\n  to_milestone: m1\n  from_tree_sha: null\n  to_tree_sha: old\n  impacted_testcases: []\n"
        ),
    );
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-qm", "base"]);
    write(
        dir.path(),
        ".markharness/knowledge/checkout/pay-securely/feature.yml",
        &format!(
            "id: pay-securely\nrequirement: checkout\nlabel: Pay securely\naxis: []\nuid: {UID}\n"
        ),
    );
    markharness::fs_safety::remove_dir_all_no_follow(
        dir.path(),
        &dir.path().join(".markharness/knowledge/checkout/pay"),
    )
    .unwrap();
    write(
        dir.path(),
        ".markharness/changes/m2.yaml",
        &format!(
            "- event_id: pay-securely--m1--m2\n  feature_id: pay-securely\n  feature_uid: {UID}\n  from_milestone: m1\n  to_milestone: m2\n  from_tree_sha: old\n  to_tree_sha: new\n  impacted_testcases: []\n"
        ),
    );
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-qm", "renamed"]);

    let response = handle_request(
        dir.path(),
        "GET",
        "/api/features?ref=HEAD",
        &DashboardConfig::default(),
    );

    assert_eq!(response.status, 200);
    let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
    let feature = body["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"] == "pay-securely")
        .unwrap();
    assert_eq!(feature["uid"], UID);
    let change_events: Vec<&str> = feature["change_events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(
        change_events,
        vec!["pay--base--m1", "pay-securely--m1--m2"],
        "expected both pre- and post-rename events under the renamed Feature, got {body}"
    );
}

fn git_managed_diff(root: &Path) -> String {
    String::from_utf8(
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["diff", "--name-only", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
}
