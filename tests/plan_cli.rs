#![allow(clippy::disallowed_methods)]

use std::path::Path;
use std::process::Command;

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

fn git_output(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn write_knowledge(root: &Path, condition_description: &str) {
    let base = root.join("knowledge/shop/checkout/pay/valid-card");
    std::fs::create_dir_all(base.join("expected")).unwrap();
    std::fs::write(
        root.join("knowledge/shop/requirement.yml"),
        "id: shop\nlabel: Shop\naxis: []\n",
    )
    .unwrap();
    std::fs::write(
        root.join("knowledge/shop/checkout/feature.yml"),
        "id: checkout\nrequirement: shop\nlabel: Checkout\naxis: []\n",
    )
    .unwrap();
    std::fs::write(
        root.join("knowledge/shop/checkout/pay/behavior.yml"),
        "id: pay\nfeature: checkout\nlabel: Pay\naxis: []\ndescription: Pay.\n",
    )
    .unwrap();
    std::fs::write(base.join("condition.yml"), format!("id: valid-card\nbehavior: pay\nlabel: Valid card\ndescription: {condition_description}\n")).unwrap();
    std::fs::write(
        base.join("expected/001.yml"),
        "id: accepted\ncondition: valid-card\ndescription: Accepted.\n",
    )
    .unwrap();
}

#[test]
fn plan_command_builds_a_versioned_plan_for_arbitrary_base_and_head_commits() {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-q", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "Test"]);
    write_knowledge(repo.path(), "A valid card.");
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-qm", "base"]);
    write_knowledge(repo.path(), "A supported valid card.");
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-qm", "head"]);

    let output = Command::new(env!("CARGO_BIN_EXE_markharness"))
        .current_dir(repo.path())
        .args([
            "plan", "--base", "HEAD~1", "--head", "HEAD", "--format", "json",
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let plan: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(plan["schema_version"], 1);
    assert_eq!(plan["summary"]["changed_features"], 1);
    assert_eq!(
        plan["affected_existing_tests"][0]["id"],
        "tc-shop-checkout-pay-valid-card"
    );
    assert_eq!(plan["affected_existing_tests"][0]["status"], "pending");
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../schema/verification_plan.schema.json")).unwrap();
    jsonschema::validator_for(&schema)
        .unwrap()
        .validate(&plan)
        .unwrap();

    let tree_sha = git_output(repo.path(), &["rev-parse", "HEAD:knowledge/shop/checkout"]);
    std::fs::create_dir_all(repo.path().join("executions/ci")).unwrap();
    std::fs::write(
        repo.path().join("executions/ci/results.yml"),
        format!(
            "- case_id: tc-shop-checkout-pay-valid-card\n  result: pass\n  executor: ci\n  executed_at: 2026-08-18T10:00:00Z\n  verified_feature_tree_shas:\n    checkout: {tree_sha}\n"
        ),
    )
    .unwrap();
    let verified = Command::new(env!("CARGO_BIN_EXE_markharness"))
        .current_dir(repo.path())
        .args([
            "plan", "--base", "HEAD~1", "--head", "HEAD", "--format", "json",
        ])
        .output()
        .unwrap();
    let verified_plan: serde_json::Value = serde_json::from_slice(&verified.stdout).unwrap();
    assert_eq!(
        verified_plan["affected_existing_tests"][0]["status"],
        "passed"
    );
    assert_eq!(verified_plan["summary"]["passed"], 1);

    let imported = repo.path().join("junit-canonical.json");
    std::fs::write(
        &imported,
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "artifacts": [{
                "canonical_id": "junit:test_case:checkout:external_pay",
                "source": "junit",
                "external_id": "checkout:external_pay",
                "kind": "test_case",
                "version": {"canonical_hash": format!("sha256:{}", "0".repeat(64))},
                "provenance": {"importer": "junit", "importer_version": "1", "source_locator": "junit.xml"}
            }],
            "relations": [{
                "from": "junit:test_case:checkout:external_pay",
                "relation_type": "verifies",
                "to": "markharness-native:condition:valid-card",
                "origin": {"kind": "stored"},
                "confidence": 1.0
            }],
            "evidence": [{
                "test_id": "junit:checkout:external_pay",
                "result": "pass",
                "executed_at": "2026-08-18T10:00:00Z",
                "bound_versions": {"checkout": tree_sha},
                "provenance": {"importer": "junit", "importer_version": "1", "source_locator": "junit.xml"}
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    let with_stored_trace = Command::new(env!("CARGO_BIN_EXE_markharness"))
        .current_dir(repo.path())
        .args([
            "plan",
            "--base",
            "HEAD~1",
            "--head",
            "HEAD",
            "--format",
            "json",
            "--evidence",
            imported.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let stored_plan: serde_json::Value = serde_json::from_slice(&with_stored_trace.stdout).unwrap();
    assert_eq!(stored_plan["summary"]["affected_tests"], 2);
    assert!(
        stored_plan["affected_existing_tests"]
            .as_array()
            .unwrap()
            .iter()
            .any(|test| test["id"] == "junit:checkout:external_pay" && test["origin"] == "stored")
    );
}
