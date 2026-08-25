#![allow(clippy::disallowed_methods)]

use std::fs;
use std::process::Command;

use std::collections::BTreeMap;

use markharness::canonical::{
    ArtifactKind, EvidenceResult, RelationOriginKind, import_junit, import_native,
};

fn git(root: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn junit_import_normalizes_results_and_preserves_version_bindings() {
    let xml = r#"<?xml version="1.0"?>
<testsuite name="checkout" timestamp="2026-08-18T10:00:00Z">
  <testcase classname="checkout" name="accepts_card"/>
  <testcase classname="checkout" name="rejects_card"><failure message="declined"/></testcase>
  <testcase classname="checkout" name="pending_card"><skipped/></testcase>
</testsuite>"#;
    let bindings = BTreeMap::from([("pay".to_string(), "feature-tree-sha".to_string())]);

    let snapshot = import_junit(xml, "reports/junit.xml", bindings.clone()).unwrap();

    assert_eq!(snapshot.schema_version, 1);
    assert_eq!(snapshot.evidence.len(), 3);
    assert_eq!(snapshot.evidence[0].test_id, "junit:checkout:accepts_card");
    assert_eq!(snapshot.evidence[0].result, EvidenceResult::Pass);
    assert_eq!(snapshot.evidence[0].bound_versions, bindings);
    assert_eq!(snapshot.evidence[1].result, EvidenceResult::Skip);
    assert_eq!(snapshot.evidence[2].result, EvidenceResult::Fail);
    assert!(
        snapshot
            .artifacts
            .iter()
            .all(|artifact| artifact.kind == ArtifactKind::TestCase)
    );
}

#[test]
fn junit_import_marks_declared_condition_trace_as_stored() {
    let xml = r#"<testsuite name="checkout">
  <testcase classname="checkout" name="accepts_card">
    <properties>
      <property name="markharness.condition" value="valid-card"/>
    </properties>
  </testcase>
</testsuite>"#;

    let snapshot = import_junit(xml, "reports/junit.xml", BTreeMap::new()).unwrap();

    assert_eq!(snapshot.relations.len(), 1);
    assert_eq!(
        snapshot.relations[0].origin.kind,
        RelationOriginKind::Stored
    );
    assert_eq!(snapshot.relations[0].relation_type, "verifies");
    assert_eq!(
        snapshot.relations[0].to,
        "markharness-native:condition:valid-card"
    );
}

#[test]
fn junit_import_accepts_the_common_testsuites_wrapper() {
    let xml = r#"<testsuites>
  <testsuite name="unit"><testcase classname="cart" name="adds_item"/></testsuite>
  <testsuite name="integration"><testcase classname="checkout" name="pays"/></testsuite>
</testsuites>"#;

    let snapshot = import_junit(xml, "reports/junit.xml", BTreeMap::new()).unwrap();

    assert_eq!(snapshot.evidence.len(), 2);
    assert_eq!(snapshot.evidence[0].test_id, "junit:cart:adds_item");
    assert_eq!(snapshot.evidence[1].test_id, "junit:checkout:pays");
}

#[test]
fn native_import_exposes_versioned_artifacts_and_derived_generation_relations() {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-q", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "Test"]);
    git(repo.path(), &["config", "core.autocrlf", "false"]);
    let base = repo
        .path()
        .join(".markharness/knowledge/checkout/pay/card/valid-card");
    fs::create_dir_all(base.join("expected")).unwrap();
    fs::write(
        repo.path()
            .join(".markharness/knowledge/checkout/requirement.yml"),
        "id: checkout\nlabel: Checkout\naxis: []\n",
    )
    .unwrap();
    fs::write(
        repo.path()
            .join(".markharness/knowledge/checkout/pay/feature.yml"),
        "id: pay\nrequirement: checkout\nlabel: Pay\naxis: []\n",
    )
    .unwrap();
    fs::write(
        repo.path()
            .join(".markharness/knowledge/checkout/pay/card/behavior.yml"),
        "id: card\nfeature: pay\nlabel: Card\naxis: []\ndescription: Pay by card.\n",
    )
    .unwrap();
    fs::write(
        base.join("condition.yml"),
        "id: valid-card\nbehavior: card\nlabel: Valid card\ndescription: A valid card.\n",
    )
    .unwrap();
    fs::write(
        base.join("expected/001.yml"),
        "id: accepted\ncondition: valid-card\ndescription: Payment is accepted.\n",
    )
    .unwrap();
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-qm", "fixture"]);
    fs::write(
        base.join("condition.yml"),
        "id: working-tree-only\nbehavior: card\nlabel: Working tree\ndescription: Not committed.\n",
    )
    .unwrap();

    let snapshot = import_native(repo.path(), "HEAD").unwrap();

    let feature = snapshot
        .artifacts
        .iter()
        .find(|artifact| artifact.kind == ArtifactKind::Feature)
        .unwrap();
    assert_eq!(feature.source, "markharness-native");
    assert_eq!(feature.external_id, "pay");
    assert!(feature.version.git_oid.is_some());
    assert!(snapshot.relations.iter().any(|relation| {
        relation.from == "markharness-native:test_case:tc-checkout-pay-card-valid-card"
            && relation.to == "markharness-native:condition:valid-card"
            && relation.origin.kind == RelationOriginKind::Derived
            && relation.origin.rule.as_deref() == Some("markharness-generate")
    }));
    assert!(
        !snapshot
            .artifacts
            .iter()
            .any(|artifact| artifact.external_id == "working-tree-only")
    );
}

/// ADR 0013: a Feature artifact must carry its `uid` (when the Feature has
/// one) alongside `external_id`, so a consumer holding two snapshots taken
/// at different times can recognize the same Feature across a rename
/// instead of relying solely on `external_id`.
#[test]
fn native_import_carries_the_feature_uid_when_the_feature_has_one() {
    const UID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-q", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "Test"]);
    git(repo.path(), &["config", "core.autocrlf", "false"]);
    fs::create_dir_all(repo.path().join(".markharness/knowledge/checkout/pay")).unwrap();
    fs::write(
        repo.path()
            .join(".markharness/knowledge/checkout/requirement.yml"),
        "id: checkout\nlabel: Checkout\naxis: []\n",
    )
    .unwrap();
    fs::write(
        repo.path()
            .join(".markharness/knowledge/checkout/pay/feature.yml"),
        format!("id: pay\nrequirement: checkout\nlabel: Pay\naxis: []\nuid: {UID}\n"),
    )
    .unwrap();
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-qm", "fixture"]);

    let snapshot = import_native(repo.path(), "HEAD").unwrap();

    let feature = snapshot
        .artifacts
        .iter()
        .find(|artifact| artifact.kind == ArtifactKind::Feature)
        .unwrap();
    assert_eq!(feature.uid.as_deref(), Some(UID));
}
