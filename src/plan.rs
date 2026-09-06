use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::canonical::{EvidenceResult, RelationOriginKind};
use crate::changes::ChangeEvent;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanEvidence {
    pub test_id: String,
    pub result: EvidenceResult,
    #[serde(default)]
    pub executed_at: Option<String>,
    /// ADR 0017 §5: the native execution record (`execution::ExecutionEntry`)
    /// this evidence was built from, so the plan can name exactly which
    /// record it adopted. `None` for evidence with no such record — e.g.
    /// canonical/imported evidence never sets this, since `application.rs`
    /// deliberately excludes that provenance from the evidence candidates a
    /// plan judges (out of scope for ADR 0017 §5; see the Case UID/revision
    /// model this evidence still requires via `bound_versions`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_uid: Option<String>,
    pub bound_versions: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredTrace {
    pub test_id: String,
    pub feature_id: String,
}

/// A TestCase's current identity (ADR 0017 §3), as of the plan's `head`.
/// `evidence_status` requires a `PlanEvidence`'s `bound_versions` to name
/// exactly this `case_uid`/`case_revision` pair before considering it for
/// applicability — a test absent from `PlanInput::case_versions` (an
/// unmigrated Scenario) can never be found `Passed`/`Failed`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CaseVersion {
    pub case_uid: String,
    pub case_revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanInput {
    pub base: String,
    pub head: String,
    pub changes: Vec<ChangeEvent>,
    pub evidence: Vec<PlanEvidence>,
    pub stored_traces: Vec<StoredTrace>,
    /// ADR 0017 §5: the opaque, non-empty build/commit identifier this plan
    /// asks "is currently verified" — typically `head` resolved to a commit
    /// OID. Evidence whose own `bound_versions["target_revision"]` doesn't
    /// match this exactly is inapplicable, however well its `case_uid`/
    /// `case_revision` matched.
    pub target_revision: String,
    /// The environment this plan requires evidence to have run in. `Some`
    /// matches only evidence recorded with exactly that environment.
    /// `None` means no specific environment is required, but this is not
    /// "match anything": evidence with no recorded `environment` at all
    /// (unknown) never satisfies any requirement, `None` included — ADR
    /// 0017 §5's "対象・環境が不明な記録は合格を満たさない" is unconditional.
    pub environment: Option<String>,
    /// Each affected test's current `(case_uid, case_revision)`, keyed by
    /// `test_id`/case_id.
    pub case_versions: BTreeMap<String, CaseVersion>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TestStatus {
    Passed,
    Failed,
    Pending,
    Stale,
    /// ADR 0017 §5: multiple applicable records (same case_uid, case_revision,
    /// target_revision, and environment) disagree on the result. The plan
    /// must not resolve this by timestamp alone ("日時だけで独立した結果を
    /// 上書き・優先しない") — an explicit human decision is required.
    Unresolved,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProposalDecision {
    Proposed,
    Accepted,
    Rejected,
    Deferred,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChangedFeature {
    pub id: String,
    pub from_tree_sha: Option<String>,
    pub to_tree_sha: Option<String>,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AffectedExistingTest {
    pub id: String,
    pub feature_id: String,
    pub reason: String,
    pub origin: RelationOriginKind,
    pub status: TestStatus,
    /// ADR 0017 §5: the `execution_uid`(s) of the native execution record(s)
    /// this `status` is based on. Exactly one entry for `Passed`/`Failed`
    /// (the adopted record); every conflicting record's `execution_uid` for
    /// `Unresolved`, so a human can audit the disagreement; empty for
    /// `Pending`/`Stale`. Evidence with no `execution_uid` of its own
    /// (canonical/imported evidence) contributes nothing to this list even
    /// when it was the evidence that decided `status`.
    #[serde(default)]
    pub execution_uids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NewRequiredTest {
    pub proposal_id: String,
    pub feature_id: String,
    pub behavior: String,
    pub reason: String,
    pub confidence: f64,
    pub decision: ProposalDecision,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanSummary {
    pub changed_features: usize,
    pub affected_tests: usize,
    pub new_tests: usize,
    pub obsolete_tests: usize,
    pub passed: usize,
    pub pending: usize,
    pub failed: usize,
    pub stale_evidence: usize,
    pub unresolved: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VerificationPlan {
    pub schema_version: u32,
    pub base: String,
    pub head: String,
    pub summary: PlanSummary,
    pub changed_features: Vec<ChangedFeature>,
    pub affected_existing_tests: Vec<AffectedExistingTest>,
    pub new_required_tests: Vec<NewRequiredTest>,
    pub obsolete_tests: Vec<serde_json::Value>,
}

/// Optional boundary for proposal generators such as AI-assisted adapters.
/// The deterministic rule-based baseline remains the default and adapters
/// only add reviewable proposals; they never modify canonical knowledge.
pub trait ProposalAdapter {
    fn propose(&self, change: &ChangeEvent) -> Vec<NewRequiredTest>;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanEvaluation {
    pub true_positives: usize,
    pub false_positives: usize,
    pub false_negatives: usize,
    pub precision: f64,
    pub recall: f64,
}

pub fn evaluate_proposals(predicted: &[String], expected: &[String]) -> PlanEvaluation {
    let predicted: BTreeSet<&String> = predicted.iter().collect();
    let expected: BTreeSet<&String> = expected.iter().collect();
    let true_positives = predicted.intersection(&expected).count();
    let false_positives = predicted.difference(&expected).count();
    let false_negatives = expected.difference(&predicted).count();
    let precision = if true_positives + false_positives == 0 {
        0.0
    } else {
        true_positives as f64 / (true_positives + false_positives) as f64
    };
    let recall = if true_positives + false_negatives == 0 {
        0.0
    } else {
        true_positives as f64 / (true_positives + false_negatives) as f64
    };
    PlanEvaluation {
        true_positives,
        false_positives,
        false_negatives,
        precision,
        recall,
    }
}

/// ADR 0017 §5: decides a test's status by requiring its evidence's
/// `bound_versions` to name the test's *current* `case_uid`/`case_revision`
/// (from `case_versions`) and the plan's required `target_revision`/
/// `environment` — never a Feature-tree-SHA proxy. A test with no entry in
/// `case_versions` (an unmigrated Scenario) has no identity to match
/// against and is always `Pending`. Evidence recorded with no `environment`
/// at all (unknown) is never applicable, even when the plan itself has no
/// specific environment requirement — "対象・環境が不明な記録は保存できて
/// も合格を満たさない" is unconditional, not merely "matches whatever the
/// plan happens to ask for".
fn evidence_status(
    test_id: &str,
    case_versions: &BTreeMap<String, CaseVersion>,
    target_revision: &str,
    environment: Option<&str>,
    evidence: &[PlanEvidence],
) -> (TestStatus, Vec<String>) {
    let matching_test: Vec<&PlanEvidence> = evidence
        .iter()
        .filter(|item| item.test_id == test_id)
        .collect();
    let Some(case_version) = case_versions.get(test_id) else {
        return (TestStatus::Pending, Vec::new());
    };
    let applicable: Vec<&PlanEvidence> = matching_test
        .iter()
        .copied()
        .filter(|item| {
            let case_uid_matches = item.bound_versions.get("case_uid").map(String::as_str)
                == Some(case_version.case_uid.as_str());
            let case_revision_matches =
                item.bound_versions.get("case_revision").map(String::as_str)
                    == Some(case_version.case_revision.as_str());
            let target_revision_matches = item
                .bound_versions
                .get("target_revision")
                .map(String::as_str)
                == Some(target_revision);
            // ADR 0017 §5: "対象・環境が不明な記録は保存できても合格を満た
            // さない" is unconditional — an execution record with no
            // recorded `environment` (unknown) must never be applicable,
            // even when the plan itself has no specific environment
            // requirement (`environment: None`). Only a plan with no
            // requirement paired with a record naming *some* known,
            // non-blank environment is treated as a match in that case;
            // `None == None` must never be read as "matches", and a blank
            // string is not a real environment identifier even if present
            // as a key — `execution::record_execution` already refuses to
            // store one, but this guards against a hand-edited record or
            // an externally imported evidence blob smuggling one in.
            let recorded_environment = item
                .bound_versions
                .get("environment")
                .map(String::as_str)
                .filter(|value| !value.trim().is_empty());
            let environment_matches = match (environment, recorded_environment) {
                (Some(required), Some(recorded)) => recorded == required,
                (None, Some(_)) => true,
                (_, None) => false,
            };
            case_uid_matches
                && case_revision_matches
                && target_revision_matches
                && environment_matches
        })
        .collect();
    // ADR 0017 §5: "計画には採用する実行結果を明示的に関連付ける" — when
    // several applicable records exist (agreeing or not), which one backs
    // the judgement must never be left implicit. Sort deterministically by
    // `executed_at` (earliest first), tie-broken by `execution_uid`, so the
    // choice depends only on the records themselves, never on the order
    // they happened to be collected in.
    let mut applicable = applicable;
    applicable.sort_by(|a, b| {
        a.executed_at
            .as_deref()
            .unwrap_or("")
            .cmp(b.executed_at.as_deref().unwrap_or(""))
            .then(
                a.execution_uid
                    .as_deref()
                    .unwrap_or("")
                    .cmp(b.execution_uid.as_deref().unwrap_or("")),
            )
    });
    let conflicting = applicable
        .split_first()
        .is_some_and(|(first, rest)| rest.iter().any(|item| item.result != first.result));
    if conflicting {
        // ADR 0017 §5: two applicable records disagree (e.g. a pass and a
        // fail both bound to the same case_uid/case_revision/target_revision/
        // environment). Picking whichever has the later `executed_at` would
        // let a timestamp alone settle a contradiction the ADR says must
        // never be resolved that way — instead every conflicting record's
        // `execution_uid` is surfaced so a human can audit and resolve it.
        let execution_uids = applicable
            .iter()
            .filter_map(|item| item.execution_uid.clone())
            .collect();
        (TestStatus::Unresolved, execution_uids)
    } else if let Some(first) = applicable.first() {
        let status = match first.result {
            EvidenceResult::Pass => TestStatus::Passed,
            EvidenceResult::Fail => TestStatus::Failed,
            EvidenceResult::Skip => TestStatus::Pending,
        };
        (status, first.execution_uid.clone().into_iter().collect())
    } else if matching_test.is_empty() {
        (TestStatus::Pending, Vec::new())
    } else {
        (TestStatus::Stale, Vec::new())
    }
}

pub fn build_plan(input: PlanInput) -> VerificationPlan {
    build_plan_with_adapter(input, None)
}

pub fn build_plan_with_adapter(
    input: PlanInput,
    adapter: Option<&dyn ProposalAdapter>,
) -> VerificationPlan {
    let mut changed_features = Vec::new();
    let mut affected: BTreeMap<(String, String), AffectedExistingTest> = BTreeMap::new();
    let mut proposals = Vec::new();

    for change in &input.changes {
        changed_features.push(ChangedFeature {
            id: change.feature_id.clone(),
            from_tree_sha: change.from_tree_sha.clone(),
            to_tree_sha: change.to_tree_sha.clone(),
            confidence: 1.0,
        });
        let mut test_ids: BTreeSet<(String, RelationOriginKind)> = change
            .impacted_testcases
            .iter()
            .cloned()
            .map(|id| (id, RelationOriginKind::Derived))
            .collect();
        test_ids.extend(
            input
                .stored_traces
                .iter()
                .filter(|trace| trace.feature_id == change.feature_id)
                .map(|trace| (trace.test_id.clone(), RelationOriginKind::Stored)),
        );
        if test_ids.is_empty() && change.to_tree_sha.is_some() {
            proposals.push(NewRequiredTest {
                proposal_id: format!("new-test:{}:missing-coverage", change.feature_id),
                feature_id: change.feature_id.clone(),
                behavior: format!("verify changed feature {}", change.feature_id),
                reason: "changed feature has no stored or derived test trace".to_string(),
                confidence: 1.0,
                decision: ProposalDecision::Proposed,
            });
        }
        if let Some(adapter) = adapter {
            proposals.extend(adapter.propose(change));
        }
        for (test_id, origin) in test_ids {
            let (status, execution_uids) = evidence_status(
                &test_id,
                &input.case_versions,
                &input.target_revision,
                input.environment.as_deref(),
                &input.evidence,
            );
            let item = AffectedExistingTest {
                id: test_id,
                feature_id: change.feature_id.clone(),
                reason: format!("affected by feature change {}", change.event_id),
                origin,
                status,
                execution_uids,
            };
            let key = (item.id.clone(), item.feature_id.clone());
            match affected.entry(key) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(item);
                }
                std::collections::btree_map::Entry::Occupied(mut entry)
                    if item.origin == RelationOriginKind::Stored =>
                {
                    entry.insert(item);
                }
                std::collections::btree_map::Entry::Occupied(_) => {}
            }
        }
    }

    changed_features.sort_by(|a, b| a.id.cmp(&b.id));
    proposals.sort_by(|a, b| a.proposal_id.cmp(&b.proposal_id));
    let affected_existing_tests: Vec<_> = affected.into_values().collect();
    let mut summary = PlanSummary {
        changed_features: changed_features.len(),
        affected_tests: affected_existing_tests.len(),
        new_tests: proposals.len(),
        ..PlanSummary::default()
    };
    for test in &affected_existing_tests {
        match test.status {
            TestStatus::Passed => summary.passed += 1,
            TestStatus::Failed => summary.failed += 1,
            TestStatus::Pending => summary.pending += 1,
            TestStatus::Stale => summary.stale_evidence += 1,
            TestStatus::Unresolved => summary.unresolved += 1,
        }
    }
    VerificationPlan {
        schema_version: 1,
        base: input.base,
        head: input.head,
        summary,
        changed_features,
        affected_existing_tests,
        new_required_tests: proposals,
        obsolete_tests: Vec::new(),
    }
}
