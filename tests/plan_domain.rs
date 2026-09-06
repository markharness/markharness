use std::collections::BTreeMap;

use markharness::canonical::EvidenceResult;
use markharness::changes::ChangeEvent;
use markharness::plan::{
    NewRequiredTest, PlanEvidence, PlanInput, ProposalAdapter, ProposalDecision, StoredTrace,
    TestStatus, build_plan, build_plan_with_adapter, evaluate_proposals,
};

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
    let bound_versions = BTreeMap::from([
        ("case_uid".to_string(), "case-checkout-1".to_string()),
        ("case_revision".to_string(), "rev-new".to_string()),
        ("target_revision".to_string(), "head".to_string()),
    ]);
    let evidence = vec![PlanEvidence {
        test_id: "tc-checkout".to_string(),
        result: EvidenceResult::Pass,
        executed_at: Some("2026-08-18T10:00:00Z".to_string()),
        bound_versions,
    }];

    let plan = build_plan(PlanInput {
        base: "base".to_string(),
        head: "head".to_string(),
        changes: vec![change],
        evidence,
        stored_traces: vec![],
        target_revision: "head".to_string(),
        // No environment requirement — must NOT be read as "match only
        // evidence with no environment", which would let the unknown-
        // environment record above count as Passed.
        environment: None,
        case_versions: BTreeMap::from([(
            "tc-checkout".to_string(),
            markharness::plan::CaseVersion {
                case_uid: "case-checkout-1".to_string(),
                case_revision: "rev-new".to_string(),
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
/// blank-environment rejection: even if a blank `environment` value reaches
/// `evidence_status` (a hand-edited record, or an externally imported
/// evidence blob via `--bind`), it must be treated the same as no
/// `environment` key at all — never as a "known" environment that could
/// satisfy a plan with no specific requirement.
#[test]
fn plan_engine_treats_a_blank_environment_value_the_same_as_no_environment_key() {
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
    let bound_versions = BTreeMap::from([
        ("case_uid".to_string(), "case-checkout-1".to_string()),
        ("case_revision".to_string(), "rev-new".to_string()),
        ("target_revision".to_string(), "head".to_string()),
        ("environment".to_string(), "   ".to_string()),
    ]);
    let evidence = vec![PlanEvidence {
        test_id: "tc-checkout".to_string(),
        result: EvidenceResult::Pass,
        executed_at: Some("2026-08-18T10:00:00Z".to_string()),
        bound_versions,
    }];

    let plan = build_plan(PlanInput {
        base: "base".to_string(),
        head: "head".to_string(),
        changes: vec![change],
        evidence,
        stored_traces: vec![],
        target_revision: "head".to_string(),
        environment: None,
        case_versions: BTreeMap::from([(
            "tc-checkout".to_string(),
            markharness::plan::CaseVersion {
                case_uid: "case-checkout-1".to_string(),
                case_revision: "rev-new".to_string(),
            },
        )]),
    });

    assert_eq!(
        plan.affected_existing_tests[0].status,
        TestStatus::Stale,
        "a blank environment value must never count as a known environment"
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
    let bound_versions = BTreeMap::from([
        ("case_uid".to_string(), "case-checkout-1".to_string()),
        ("case_revision".to_string(), "rev-new".to_string()),
        ("target_revision".to_string(), "head".to_string()),
        ("environment".to_string(), "ci".to_string()),
    ]);
    let evidence = vec![
        PlanEvidence {
            test_id: "tc-checkout".to_string(),
            result: EvidenceResult::Fail,
            executed_at: Some("2026-08-18T09:00:00Z".to_string()),
            bound_versions: bound_versions.clone(),
        },
        PlanEvidence {
            test_id: "tc-checkout".to_string(),
            result: EvidenceResult::Pass,
            executed_at: Some("2026-08-18T10:00:00Z".to_string()),
            bound_versions,
        },
    ];

    let plan = build_plan(PlanInput {
        base: "base".to_string(),
        head: "head".to_string(),
        changes,
        evidence,
        stored_traces: vec![],
        target_revision: "head".to_string(),
        environment: Some("ci".to_string()),
        case_versions: BTreeMap::from([(
            "tc-checkout".to_string(),
            markharness::plan::CaseVersion {
                case_uid: "case-checkout-1".to_string(),
                case_revision: "rev-new".to_string(),
            },
        )]),
    });

    assert_eq!(plan.affected_existing_tests.len(), 1);
    assert_eq!(plan.affected_existing_tests[0].status, TestStatus::Passed);
    assert_eq!(plan.new_required_tests.len(), 1);
    assert_eq!(plan.new_required_tests[0].feature_id, "search");
    assert_eq!(plan.summary.changed_features, 2);
    assert_eq!(plan.summary.passed, 1);
    assert_eq!(plan.summary.new_tests, 1);
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
        target_revision: "head".to_string(),
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
        target_revision: "head".to_string(),
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
