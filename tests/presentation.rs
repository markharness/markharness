use markharness::plan::{PlanSummary, VerificationPlan};
use markharness::presentation::{
    CommandOutcome, HumanPresenter, JsonPresenter, PresentedResult, Presenter,
};

fn empty_plan(summary: PlanSummary) -> VerificationPlan {
    VerificationPlan {
        schema_version: 1,
        base: "base".to_string(),
        head: "head".to_string(),
        summary,
        changed_features: Vec::new(),
        affected_existing_tests: Vec::new(),
        new_required_tests: Vec::new(),
        obsolete_tests: Vec::new(),
    }
}

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

/// Standards review: `docs/en/design/verification-plan-canonical-model-design.md`
/// §5 allows only *optional* field additions within one `schema_version` —
/// a field that is always present, even as an empty array, is a required
/// field in practice and changes the v1 contract's shape for every
/// existing consumer. `warnings` must be omitted, not emitted as `[]`,
/// when there's nothing to report.
#[test]
fn json_presenter_omits_warnings_for_changes_computed_when_there_are_none() {
    let result = JsonPresenter.present(&CommandOutcome::ChangesComputed {
        count: 3,
        to: "v2".to_string(),
        warnings: Vec::new(),
    });

    assert!(
        !result.stdout.contains("warnings"),
        "unexpected stdout: {}",
        result.stdout
    );
}

/// ADR 0017 §5: unresolved (mutually conflicting) evidence must stop a
/// verification plan from reading as clean — it needs the same attention as
/// an outright failure. Regression test for a bug where `plan_exit_code`
/// ignored `summary.unresolved`, letting a plan with contradictory evidence
/// exit 0.
#[test]
fn plan_exit_code_is_nonzero_when_evidence_is_unresolved() {
    let plan = empty_plan(PlanSummary {
        unresolved: 1,
        ..PlanSummary::default()
    });

    let json_result = JsonPresenter.present(&CommandOutcome::PlanBuilt(plan.clone()));
    let human_result = HumanPresenter.present(&CommandOutcome::PlanBuilt(plan));

    assert_eq!(json_result.exit_code, 1);
    assert_eq!(human_result.exit_code, 1);
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
