//! `markharness binding set` — ADR 0020 / ADR 0025's `ExecutionBinding`.
//!
//! Covers the v2 design's acceptance criteria AC05, AC13, and AC32.
//!
//! Fixtures write directly to a scratch repo before invoking the CLI
//! binary; that's outside fs_safety's managed-root scope (see clippy.toml).
#![allow(clippy::disallowed_methods)]

use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_markharness")
}

fn run(args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .output()
        .expect("failed to run markharness")
}

fn init_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let output = run(&["init", "--dir", dir.path().to_str().unwrap()]);
    assert!(output.status.success(), "init failed: {output:?}");
    dir
}

fn binding_path(dir: &std::path::Path, case_uid: &str) -> std::path::PathBuf {
    dir.join(".markharness")
        .join("bindings")
        .join(format!("{case_uid}.yml"))
}

/// AC05: a manual binding records without any timestamp or executor field.
#[test]
fn binding_set_records_a_manual_binding_with_no_timestamp_or_executor() {
    let dir = init_project();
    let case_uid = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

    let output = run(&[
        "binding",
        "set",
        "--case-uid",
        case_uid,
        "--mode",
        "manual",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{output:?}");

    let content = std::fs::read_to_string(binding_path(dir.path(), case_uid)).unwrap();
    assert!(
        content.contains("record_kind: execution_binding"),
        "{content}"
    );
    assert!(content.contains("schema_version: 1"), "{content}");
    assert!(content.contains("mode: manual"), "{content}");
    for absent in ["executed_at", "executor", "result", "build", "environment"] {
        assert!(
            !content.contains(absent),
            "an ExecutionBinding must not carry `{absent}`: {content}"
        );
    }
}

#[test]
fn binding_set_records_an_automated_binding_with_a_reference() {
    let dir = init_project();
    let case_uid = "01ARZ3NDEKTSV4RRFFQ69G5FAW";

    let output = run(&[
        "binding",
        "set",
        "--case-uid",
        case_uid,
        "--mode",
        "automated",
        "--reference",
        "src/tests/login.spec.ts",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{output:?}");

    let content = std::fs::read_to_string(binding_path(dir.path(), case_uid)).unwrap();
    assert!(content.contains("mode: automated"), "{content}");
    assert!(
        content.contains("reference: src/tests/login.spec.ts"),
        "{content}"
    );
}

/// `binding set` replaces the existing binding for the same Case UID rather
/// than accumulating records — a binding is a declaration, not an execution
/// log (ADR 0025 §1).
#[test]
fn binding_set_replaces_the_existing_binding_for_the_same_case_uid() {
    let dir = init_project();
    let case_uid = "01ARZ3NDEKTSV4RRFFQ69G5FAX";
    let args_manual = [
        "binding",
        "set",
        "--case-uid",
        case_uid,
        "--mode",
        "manual",
        "--dir",
        dir.path().to_str().unwrap(),
    ];
    assert!(run(&args_manual).status.success());

    let output = run(&[
        "binding",
        "set",
        "--case-uid",
        case_uid,
        "--mode",
        "automated",
        "--reference",
        "tests/login.spec.ts",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{output:?}");

    let content = std::fs::read_to_string(binding_path(dir.path(), case_uid)).unwrap();
    assert!(content.contains("mode: automated"), "{content}");
    assert!(!content.contains("mode: manual"), "{content}");
}

/// AC32: execution-fact fields do not exist on the v2 binding schema, so a
/// hand-written file carrying them is rejected rather than silently kept.
#[test]
fn reading_a_binding_carrying_execution_fact_fields_is_rejected() {
    let dir = init_project();
    let case_uid = "01ARZ3NDEKTSV4RRFFQ69G5FAY";
    let path = binding_path(dir.path(), case_uid);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        "schema_version: 1\nrecord_kind: execution_binding\ncase_uid: 01ARZ3NDEKTSV4RRFFQ69G5FAY\nmode: automated\nresult: pass\nexecuted_at: 2026-09-11T00:00:00Z\n",
    )
    .unwrap();

    let output = run(&["binding", "list", "--dir", dir.path().to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("result"), "{stderr}");
}

/// AC32: the CLI offers no way to supply an execution fact in the first
/// place — the flags simply do not exist.
#[test]
fn binding_set_rejects_execution_fact_flags() {
    let dir = init_project();
    for flag in ["--result", "--executed-at", "--build", "--environment"] {
        let output = run(&[
            "binding",
            "set",
            "--case-uid",
            "01ARZ3NDEKTSV4RRFFQ69G5FAZ",
            "--mode",
            "manual",
            flag,
            "whatever",
            "--dir",
            dir.path().to_str().unwrap(),
        ]);
        assert!(
            !output.status.success(),
            "`{flag}` must not be accepted: {output:?}"
        );
    }
}

/// AC13: a binding is keyed by Case UID, so renaming the display id of the
/// Scenario it belongs to cannot break it. The binding file never stores a
/// display id to begin with.
#[test]
fn a_binding_stores_no_display_id() {
    let dir = init_project();
    let case_uid = "01ARZ3NDEKTSV4RRFFQ69G5FB0";
    assert!(
        run(&[
            "binding",
            "set",
            "--case-uid",
            case_uid,
            "--mode",
            "manual",
            "--dir",
            dir.path().to_str().unwrap(),
        ])
        .status
        .success()
    );

    let content = std::fs::read_to_string(binding_path(dir.path(), case_uid)).unwrap();
    assert!(!content.contains("case_id"), "{content}");
}

/// A Case UID becomes a path component, so a traversal-shaped value is
/// refused before any file is created — the same rule `generate` applies to
/// `id:` (ADR 0024 §3 applies the identical reasoning to `release_id`).
#[test]
fn binding_set_refuses_a_case_uid_that_would_escape_the_bindings_directory() {
    let dir = init_project();
    for hostile in ["../../etc/passwd", "..", ".", "a/b", "a\\b"] {
        let output = run(&[
            "binding",
            "set",
            "--case-uid",
            hostile,
            "--mode",
            "manual",
            "--dir",
            dir.path().to_str().unwrap(),
        ]);
        assert_eq!(
            output.status.code(),
            Some(2),
            "`{hostile}` must be refused: {output:?}"
        );
    }
    assert!(
        !dir.path().join(".markharness").join("bindings").exists(),
        "a refused binding must not create the bindings directory"
    );
}
