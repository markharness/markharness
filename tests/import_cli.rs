#![allow(clippy::disallowed_methods)]

use std::process::Command;

#[test]
fn import_junit_emits_a_versioned_json_contract() {
    let dir = tempfile::tempdir().unwrap();
    let report = dir.path().join("junit.xml");
    std::fs::write(
        &report,
        r#"<testsuite name="checkout"><testcase classname="checkout" name="pays"/></testsuite>"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_markharness"))
        .args([
            "import",
            "--source",
            "junit",
            "--input",
            report.to_str().unwrap(),
            "--bind",
            "pay=tree-sha",
            "--format",
            "json",
            "--dir",
            dir.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["evidence"][0]["test_id"], "junit:checkout:pays");
    assert_eq!(json["evidence"][0]["bound_versions"]["pay"], "tree-sha");
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../schema/canonical_snapshot.schema.json")).unwrap();
    jsonschema::validator_for(&schema)
        .unwrap()
        .validate(&json)
        .unwrap();
}

#[test]
fn junit_import_matches_the_stage1_golden_contract() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new(env!("CARGO_BIN_EXE_markharness"))
        .current_dir(root)
        .args([
            "import",
            "--source",
            "junit",
            "--input",
            "tests/fixtures/stage1/junit.xml",
            "--bind",
            "pay=feature-tree-sha",
            "--format",
            "json",
            "--dir",
            ".",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        std::fs::read_to_string(root.join("tests/fixtures/stage1/junit-import.golden.json"))
            .unwrap()
    );
}
