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
    pub bound_versions: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredTrace {
    pub test_id: String,
    pub feature_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanInput {
    pub base: String,
    pub head: String,
    pub changes: Vec<ChangeEvent>,
    pub evidence: Vec<PlanEvidence>,
    pub stored_traces: Vec<StoredTrace>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TestStatus {
    Passed,
    Failed,
    Pending,
    Stale,
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

fn evidence_status(
    test_id: &str,
    feature_id: &str,
    target_version: Option<&str>,
    evidence: &[PlanEvidence],
) -> TestStatus {
    let matching_test: Vec<&PlanEvidence> = evidence
        .iter()
        .filter(|item| item.test_id == test_id)
        .collect();
    let matching_version: Vec<&PlanEvidence> = matching_test
        .iter()
        .copied()
        .filter(|item| {
            item.bound_versions.get(feature_id).map(String::as_str) == target_version
                && target_version.is_some()
        })
        .collect();
    if let Some(latest) = matching_version
        .iter()
        .max_by_key(|item| item.executed_at.as_deref().unwrap_or(""))
    {
        match latest.result {
            EvidenceResult::Pass => TestStatus::Passed,
            EvidenceResult::Fail => TestStatus::Failed,
            EvidenceResult::Skip => TestStatus::Pending,
        }
    } else if matching_test.is_empty() {
        TestStatus::Pending
    } else {
        TestStatus::Stale
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
            let status = evidence_status(
                &test_id,
                &change.feature_id,
                change.to_tree_sha.as_deref(),
                &input.evidence,
            );
            let item = AffectedExistingTest {
                id: test_id,
                feature_id: change.feature_id.clone(),
                reason: format!("affected by feature change {}", change.event_id),
                origin,
                status,
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
