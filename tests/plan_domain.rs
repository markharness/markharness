use std::collections::BTreeMap;

use markharness::canonical::EvidenceResult;
use markharness::changes::ChangeEvent;
use markharness::identity::{CaseRevision, CaseUid, Environment, ExecutionUid, TargetRevision};
use markharness::plan::{
    BoundVersions, NewRequiredTest, PlanEvidence, PlanInput, ProposalAdapter, ProposalDecision,
    StoredTrace, TestStatus, build_plan, build_plan_with_adapter, evaluate_proposals,
};

fn bound_versions(
    case_uid: &str,
    case_revision: &str,
    target_revision: &str,
    environment: Option<&str>,
) -> BoundVersions {
    BoundVersions {
        case_uid: CaseUid::new(case_uid).unwrap(),
        case_revision: CaseRevision::new(case_revision).unwrap(),
        target_revision: TargetRevision::new(target_revision).unwrap(),
        environment: environment.map(|e| Environment::new(e).unwrap()),
    }
}

#[derive(serde::Deserialize)]
struct HistoricalFixture {
    input: PlanInput,
    human_required_tests: Vec<String>,
}

/// ADR 0017 §5: "対象・環境が不明な記録は保存できても合格を満たさない" is
/// unconditional — an execution record with no recorded `environment`
/// (unknown) must never be applicable, even when the plan itself has no
/// specific environment requirement (`environment: None`). Regression test
/// for a bug where `None == None` was read as "matches" instead of "the
/// record's environment is unknown, so it never satisfies anything".
#[test]
fn plan_engine_never_treats_unknown_environment_evidence_as_applicable_even_with_no_requirement() {
    let change = ChangeEvent {
        event_id: "checkout--base--head".to_string(),
        feature_id: "checkout".to_string(),
        feature_uid: None,
        feature_id_at_from: None,
        feature_id_at_to: None,
        from_milestone: "base".to_string(),
        to_milestone: "head".to_string(),
        from_tree_sha: Some("old".to_string()),
        to_tree_sha: Some("new".to_string()),
        impacted_testcases: vec!["tc-checkout".to_string()],
        impact_reason: markharness::changes::ImpactReason::default(),
        change_type: None,
        true_divergences: vec![],
        related_events: vec![],
    };
    // No "environment" key at all: an unknown-environment record.
    let evidence = vec![PlanEvidence {
        test_id: "tc-checkout".to_string(),
        result: EvidenceResult::Pass,
        executed_at: Some("2026-08-18T10:00:00Z".to_string()),
        execution_uid: None,
        bound_versions: bound_versions("case-checkout-1", "rev-new", "head", None),
    }];

    let plan = build_plan(PlanInput {
        base: "base".to_string(),
        head: "head".to_string(),
        changes: vec![change],
        evidence,
        stored_traces: vec![],
        target_revision: TargetRevision::new("head").unwrap(),
        // No environment requirement — must NOT be read as "match only
        // evidence with no environment", which would let the unknown-
        // environment record above count as Passed.
        environment: None,
        case_versions: BTreeMap::from([(
            "tc-checkout".to_string(),
            markharness::plan::CaseVersion {
                case_uid: CaseUid::new("case-checkout-1").unwrap(),
                case_revision: CaseRevision::new("rev-new").unwrap(),
            },
        )]),
    });

    assert_eq!(
        plan.affected_existing_tests[0].status,
        TestStatus::Stale,
        "an unknown-environment record must never count as Passed, even when the plan has no \
         environment requirement"
    );
}

/// Defense in depth alongside `execution::record_execution`'s own
/// blank-environment rejection: a blank `environment` value (a hand-edited
/// record, or an externally imported evidence blob via `--bind`) must never
/// reach `evidence_status` as a "known" environment that could satisfy a
/// plan with no specific requirement. Since `BoundVersions.environment` is
/// now `Option<Environment>` (not `Option<String>`), this is enforced at
/// deserialization — a blank value can no longer be constructed as an
/// `Environment` at all, in Rust code or from a YAML/JSON file — rather than
/// by a runtime filter inside `evidence_status`.
#[test]
fn plan_evidence_deserialization_rejects_a_blank_environment_value() {
    let json = r#"{
        "test_id": "tc-checkout",
        "result": "pass",
        "executed_at": "2026-08-18T10:00:00Z",
        "bound_versions": {
            "case_uid": "case-checkout-1",
            "case_revision": "rev-new",
            "target_revision": "head",
            "environment": "   "
        }
    }"#;

    let result: Result<PlanEvidence, _> = serde_json::from_str(json);

    assert!(
        result.is_err(),
        "a blank environment value must be rejected at deserialization, not silently accepted \
         as a known environment"
    );
}

/// ADR 0017 §5: "日時だけで独立した結果を上書き・優先しない" — two applicable
/// records (identical case_uid/case_revision/target_revision/environment)
/// that disagree must not be resolved by picking whichever has the later
/// `executed_at`. Regression test for a bug where `max_by_key(executed_at)`
/// silently let a later "pass" hide an earlier "fail" (or vice versa).
#[test]
fn plan_engine_marks_conflicting_applicable_evidence_as_unresolved_instead_of_picking_by_time() {
    let change = ChangeEvent {
        event_id: "checkout--base--head".to_string(),
        feature_id: "checkout".to_string(),
        feature_uid: None,
        feature_id_at_from: None,
        feature_id_at_to: None,
        from_milestone: "base".to_string(),
        to_milestone: "head".to_string(),
        from_tree_sha: Some("old".to_string()),
        to_tree_sha: Some("new".to_string()),
        impacted_testcases: vec!["tc-checkout".to_string()],
        impact_reason: markharness::changes::ImpactReason::default(),
        change_type: None,
        true_divergences: vec![],
        related_events: vec![],
    };
    let evidence = vec![
        PlanEvidence {
            test_id: "tc-checkout".to_string(),
            result: EvidenceResult::Fail,
            executed_at: Some("2026-08-18T10:00:00Z".to_string()),
            execution_uid: Some(ExecutionUid::new("exec-fail").unwrap()),
            bound_versions: bound_versions("case-checkout-1", "rev-new", "head", Some("ci")),
        },
        // Recorded later, but disagrees with the fail above — the plan must
        // not let this later timestamp silently override it.
        PlanEvidence {
            test_id: "tc-checkout".to_string(),
            result: EvidenceResult::Pass,
            executed_at: Some("2026-08-18T11:00:00Z".to_string()),
            execution_uid: Some(ExecutionUid::new("exec-pass").unwrap()),
            bound_versions: bound_versions("case-checkout-1", "rev-new", "head", Some("ci")),
        },
    ];

    let plan = build_plan(PlanInput {
        base: "base".to_string(),
        head: "head".to_string(),
        changes: vec![change],
        evidence,
        stored_traces: vec![],
        target_revision: TargetRevision::new("head").unwrap(),
        environment: Some(Environment::new("ci").unwrap()),
        case_versions: BTreeMap::from([(
            "tc-checkout".to_string(),
            markharness::plan::CaseVersion {
                case_uid: CaseUid::new("case-checkout-1").unwrap(),
                case_revision: CaseRevision::new("rev-new").unwrap(),
            },
        )]),
    });

    assert_eq!(
        plan.affected_existing_tests[0].status,
        TestStatus::Unresolved,
        "conflicting applicable evidence must be unresolved, not silently picked by timestamp"
    );
    assert_eq!(plan.summary.unresolved, 1);
    assert_eq!(
        plan.affected_existing_tests[0].execution_uids,
        vec![
            ExecutionUid::new("exec-fail").unwrap(),
            ExecutionUid::new("exec-pass").unwrap()
        ],
        "an Unresolved status must name every conflicting execution so a human can audit them"
    );
}

#[test]
fn plan_engine_resolves_version_bound_evidence_and_missing_test_gaps() {
    let changes = vec![
        ChangeEvent {
            event_id: "checkout--base--head".to_string(),
            feature_id: "checkout".to_string(),
            feature_uid: None,
            feature_id_at_from: None,
            feature_id_at_to: None,
            from_milestone: "base".to_string(),
            to_milestone: "head".to_string(),
            from_tree_sha: Some("old".to_string()),
            to_tree_sha: Some("new".to_string()),
            impacted_testcases: vec!["tc-checkout".to_string()],
            impact_reason: markharness::changes::ImpactReason::default(),
            change_type: None,
            true_divergences: vec![],
            related_events: vec![],
        },
        ChangeEvent {
            event_id: "search--base--head".to_string(),
            feature_id: "search".to_string(),
            feature_uid: None,
            feature_id_at_from: None,
            feature_id_at_to: None,
            from_milestone: "base".to_string(),
            to_milestone: "head".to_string(),
            from_tree_sha: Some("old-search".to_string()),
            to_tree_sha: Some("new-search".to_string()),
            impacted_testcases: vec![],
            impact_reason: markharness::changes::ImpactReason::default(),
            change_type: None,
            true_divergences: vec![],
            related_events: vec![],
        },
    ];
    // Two applicable records that agree (both pass, at different times):
    // ADR 0017 §5 only forbids resolving *conflicting* results by timestamp
    // alone — agreeing records may coexist without ambiguity.
    let evidence = vec![
        // Recorded later, but agrees — deliberately listed first so a
        // naive "first in the input" pick would get this wrong.
        PlanEvidence {
            test_id: "tc-checkout".to_string(),
            result: EvidenceResult::Pass,
            executed_at: Some("2026-08-18T10:00:00Z".to_string()),
            execution_uid: Some(ExecutionUid::new("exec-later").unwrap()),
            bound_versions: bound_versions("case-checkout-1", "rev-new", "head", Some("ci")),
        },
        PlanEvidence {
            test_id: "tc-checkout".to_string(),
            result: EvidenceResult::Pass,
            executed_at: Some("2026-08-18T09:00:00Z".to_string()),
            execution_uid: Some(ExecutionUid::new("exec-earlier").unwrap()),
            bound_versions: bound_versions("case-checkout-1", "rev-new", "head", Some("ci")),
        },
    ];

    let plan = build_plan(PlanInput {
        base: "base".to_string(),
        head: "head".to_string(),
        changes,
        evidence,
        stored_traces: vec![],
        target_revision: TargetRevision::new("head").unwrap(),
        environment: Some(Environment::new("ci").unwrap()),
        case_versions: BTreeMap::from([(
            "tc-checkout".to_string(),
            markharness::plan::CaseVersion {
                case_uid: CaseUid::new("case-checkout-1").unwrap(),
                case_revision: CaseRevision::new("rev-new").unwrap(),
            },
        )]),
    });

    assert_eq!(plan.affected_existing_tests.len(), 1);
    assert_eq!(plan.affected_existing_tests[0].status, TestStatus::Passed);
    assert_eq!(
        plan.affected_existing_tests[0].execution_uids,
        vec![ExecutionUid::new("exec-earlier").unwrap()],
        "agreeing evidence must deterministically adopt the earliest-executed record, \
         regardless of input order"
    );
    assert_eq!(plan.new_required_tests.len(), 1);
    assert_eq!(plan.new_required_tests[0].feature_id, "search");
    assert_eq!(plan.summary.changed_features, 2);
    assert_eq!(plan.summary.passed, 1);
    assert_eq!(plan.summary.new_tests, 1);
}

/// ADR 0017 §5: "計画には採用する実行結果を明示的に関連付ける" — a passed
/// test's `AffectedExistingTest` must name the `execution_uid` of the record
/// that backs the judgement, not leave the reader to guess which of
/// potentially many recorded executions was used.
#[test]
fn plan_engine_exposes_the_execution_uid_that_backs_a_passed_status() {
    let change = ChangeEvent {
        event_id: "checkout--base--head".to_string(),
        feature_id: "checkout".to_string(),
        feature_uid: None,
        feature_id_at_from: None,
        feature_id_at_to: None,
        from_milestone: "base".to_string(),
        to_milestone: "head".to_string(),
        from_tree_sha: Some("old".to_string()),
        to_tree_sha: Some("new".to_string()),
        impacted_testcases: vec!["tc-checkout".to_string()],
        impact_reason: markharness::changes::ImpactReason::default(),
        change_type: None,
        true_divergences: vec![],
        related_events: vec![],
    };
    let evidence = vec![PlanEvidence {
        test_id: "tc-checkout".to_string(),
        result: EvidenceResult::Pass,
        executed_at: Some("2026-08-18T10:00:00Z".to_string()),
        execution_uid: Some(ExecutionUid::new("exec-1").unwrap()),
        bound_versions: bound_versions("case-checkout-1", "rev-new", "head", Some("ci")),
    }];

    let plan = build_plan(PlanInput {
        base: "base".to_string(),
        head: "head".to_string(),
        changes: vec![change],
        evidence,
        stored_traces: vec![],
        target_revision: TargetRevision::new("head").unwrap(),
        environment: Some(Environment::new("ci").unwrap()),
        case_versions: BTreeMap::from([(
            "tc-checkout".to_string(),
            markharness::plan::CaseVersion {
                case_uid: CaseUid::new("case-checkout-1").unwrap(),
                case_revision: CaseRevision::new("rev-new").unwrap(),
            },
        )]),
    });

    assert_eq!(plan.affected_existing_tests[0].status, TestStatus::Passed);
    assert_eq!(
        plan.affected_existing_tests[0].execution_uids,
        vec![ExecutionUid::new("exec-1").unwrap()]
    );
}

#[test]
fn historical_plan_evaluation_reports_precision_and_recall() {
    let predicted = vec!["new-test:search:missing-coverage".to_string()];
    let human_plan = vec![
        "new-test:search:missing-coverage".to_string(),
        "new-test:search:unicode".to_string(),
    ];

    let evaluation = evaluate_proposals(&predicted, &human_plan);

    assert_eq!(evaluation.true_positives, 1);
    assert_eq!(evaluation.false_positives, 0);
    assert_eq!(evaluation.false_negatives, 1);
    assert_eq!(evaluation.precision, 1.0);
    assert_eq!(evaluation.recall, 0.5);
}

#[test]
fn historical_pr_fixture_reproduces_the_golden_plan_and_evaluation() {
    let fixture: HistoricalFixture =
        serde_json::from_str(include_str!("fixtures/stage2/historical-pr.json")).unwrap();
    let plan = build_plan(fixture.input);
    let actual = format!("{}\n", serde_json::to_string_pretty(&plan).unwrap());

    assert_eq!(
        actual,
        include_str!("fixtures/stage2/verification-plan.golden.json")
    );
    let predicted: Vec<String> = plan
        .new_required_tests
        .iter()
        .map(|proposal| proposal.proposal_id.clone())
        .collect();
    let evaluation = evaluate_proposals(&predicted, &fixture.human_required_tests);
    assert_eq!(evaluation.precision, 1.0);
    assert_eq!(evaluation.recall, 0.5);
}

struct OptionalProposalAdapter;

impl ProposalAdapter for OptionalProposalAdapter {
    fn propose(&self, change: &ChangeEvent) -> Vec<NewRequiredTest> {
        vec![NewRequiredTest {
            proposal_id: format!("ai:{}:boundary", change.feature_id),
            feature_id: change.feature_id.clone(),
            behavior: "exercise an inferred boundary".to_string(),
            reason: "optional adapter suggestion".to_string(),
            confidence: 0.6,
            decision: ProposalDecision::Proposed,
        }]
    }
}

#[test]
fn optional_proposal_adapter_adds_reviewable_proposals_without_changing_the_baseline() {
    let change = ChangeEvent {
        event_id: "checkout--base--head".to_string(),
        feature_id: "checkout".to_string(),
        feature_uid: None,
        feature_id_at_from: None,
        feature_id_at_to: None,
        from_milestone: "base".to_string(),
        to_milestone: "head".to_string(),
        from_tree_sha: Some("old".to_string()),
        to_tree_sha: Some("new".to_string()),
        impacted_testcases: vec!["tc-checkout".to_string()],
        impact_reason: markharness::changes::ImpactReason::default(),
        change_type: None,
        true_divergences: vec![],
        related_events: vec![],
    };
    let input = PlanInput {
        base: "base".to_string(),
        head: "head".to_string(),
        changes: vec![change],
        evidence: vec![],
        stored_traces: vec![],
        target_revision: TargetRevision::new("head").unwrap(),
        environment: None,
        case_versions: BTreeMap::new(),
    };

    let plan = build_plan_with_adapter(input, Some(&OptionalProposalAdapter));

    assert_eq!(plan.new_required_tests.len(), 1);
    assert_eq!(plan.new_required_tests[0].confidence, 0.6);
    assert_eq!(
        plan.new_required_tests[0].decision,
        ProposalDecision::Proposed
    );
}

#[test]
fn plan_engine_uses_stored_traces_as_affected_existing_tests() {
    let change = ChangeEvent {
        event_id: "checkout--base--head".to_string(),
        feature_id: "checkout".to_string(),
        feature_uid: None,
        feature_id_at_from: None,
        feature_id_at_to: None,
        from_milestone: "base".to_string(),
        to_milestone: "head".to_string(),
        from_tree_sha: Some("old".to_string()),
        to_tree_sha: Some("new".to_string()),
        impacted_testcases: vec![],
        impact_reason: markharness::changes::ImpactReason::default(),
        change_type: None,
        true_divergences: vec![],
        related_events: vec![],
    };

    let plan = build_plan(PlanInput {
        base: "base".to_string(),
        head: "head".to_string(),
        changes: vec![change],
        evidence: vec![],
        stored_traces: vec![StoredTrace {
            test_id: "junit:checkout:pays".to_string(),
            feature_id: "checkout".to_string(),
        }],
        target_revision: TargetRevision::new("head").unwrap(),
        environment: None,
        case_versions: BTreeMap::new(),
    });

    assert_eq!(plan.affected_existing_tests.len(), 1);
    assert_eq!(
        plan.affected_existing_tests[0].origin,
        markharness::canonical::RelationOriginKind::Stored
    );
    assert!(plan.new_required_tests.is_empty());
}
