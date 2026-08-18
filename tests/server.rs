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
        "knowledge/checkout/requirement.yml",
        "id: checkout\nlabel: Checkout\naxis: [commerce]\n",
    );
    write(
        dir.path(),
        "knowledge/checkout/pay/feature.yml",
        "id: pay\nrequirement: checkout\nlabel: Pay\naxis: [commerce]\n",
    );
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-qm", "base"]);
    write(
        dir.path(),
        "knowledge/checkout/pay/feature.yml",
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
