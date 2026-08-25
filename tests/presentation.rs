use markharness::presentation::{
    CommandOutcome, HumanPresenter, JsonPresenter, PresentedResult, Presenter,
};
use markharness::verify::PendingReport;

#[test]
fn human_presenter_renders_generated_outcome_without_side_effects() {
    let result = HumanPresenter.present(&CommandOutcome::Generated {
        count: 2,
        written: Vec::new(),
    });

    assert_eq!(
        result,
        PresentedResult {
            stdout: "generated 2 testcase(s) into .markharness/generated/testcases/\n".to_string(),
            stderr: String::new(),
            exit_code: 0,
        }
    );
}

#[test]
fn json_presenter_wraps_generated_outcome_in_versioned_contract() {
    let result = JsonPresenter.present(&CommandOutcome::Generated {
        count: 2,
        written: Vec::new(),
    });

    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stderr, "");
    assert_eq!(
        result.stdout,
        "{\"generated\":2,\"ok\":true,\"outcome\":\"generated\",\"schema_version\":1,\"written\":[]}\n"
    );
}

/// ADR 0013 検証規則: `changes compute` only ever compares two
/// `.markharness` snapshots, never full commit history — that distinction
/// must be machine-readable in its JSON output, not just documented, so a
/// CI gate can tell it apart from `identity audit`.
#[test]
fn json_presenter_marks_changes_computed_with_the_two_snapshot_audit_scope() {
    let result = JsonPresenter.present(&CommandOutcome::ChangesComputed {
        count: 3,
        to: "v2".to_string(),
        warnings: Vec::new(),
    });

    assert_eq!(result.exit_code, 0);
    assert!(
        result.stdout.contains("\"audit_scope\":\"two_snapshot\""),
        "unexpected stdout: {}",
        result.stdout
    );
}

/// Issue #29 §6: a legacy-schema-version fallback must be machine-readable
/// in JSON output, not just a human-facing message, so a caller consuming
/// `--json` can still see it.
#[test]
fn json_presenter_includes_warnings_for_changes_computed() {
    let result = JsonPresenter.present(&CommandOutcome::ChangesComputed {
        count: 0,
        to: "v2".to_string(),
        warnings: vec!["legacy schema version 1 assumed at ref v1".to_string()],
    });

    assert!(
        result
            .stdout
            .contains("\"warnings\":[\"legacy schema version 1 assumed at ref v1\"]"),
        "unexpected stdout: {}",
        result.stdout
    );
}

#[test]
fn human_presenter_prints_warnings_for_changes_computed() {
    let result = HumanPresenter.present(&CommandOutcome::ChangesComputed {
        count: 0,
        to: "v2".to_string(),
        warnings: vec!["legacy schema version 1 assumed at ref v1".to_string()],
    });

    assert!(
        result
            .stdout
            .contains("warning: legacy schema version 1 assumed at ref v1\n"),
        "unexpected stdout: {}",
        result.stdout
    );
}

/// Same `audit_scope` contract for `verify pending`'s JSON output.
#[test]
fn json_presenter_marks_pending_with_the_two_snapshot_audit_scope() {
    let result = JsonPresenter.present(&CommandOutcome::Pending {
        report: PendingReport::default(),
        fail_on_pending: false,
    });

    assert_eq!(result.exit_code, 0);
    assert!(
        result.stdout.contains("\"audit_scope\":\"two_snapshot\""),
        "unexpected stdout: {}",
        result.stdout
    );
}
