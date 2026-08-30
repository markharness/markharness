use markharness::changes::CommitRef;
use markharness::generate::{
    CaseFilePaths, ExpectedSnapshot, KnowledgeCaseSnapshot, KnowledgeSnapshot, compile_testcases,
};
use markharness::verify::{
    PendingCandidate, ReflectedChange, VerificationStatus, evaluate_pending_candidate,
};

#[test]
fn testcase_compiler_compiles_a_snapshot_without_filesystem_access() {
    let snapshot = KnowledgeSnapshot {
        cases: vec![KnowledgeCaseSnapshot {
            requirement_id: "req".to_string(),
            requirement_uid: None,
            requirement_axis: vec!["ui".to_string()],
            feature_id: "feature".to_string(),
            feature_uid: None,
            feature_axis: vec!["workflow".to_string()],
            behavior_id: "behavior".to_string(),
            behavior_uid: None,
            behavior_description: "perform action".to_string(),
            behavior_steps: vec!["click the button".to_string()],
            behavior_axis: vec!["ui".to_string()],
            condition_id: "condition".to_string(),
            condition_uid: None,
            condition_description: "given state".to_string(),
            expected: vec![ExpectedSnapshot {
                id: "expected-1".to_string(),
                description: "result".to_string(),
                uid: None,
            }],
            case_files: CaseFilePaths::default(),
        }],
    };

    let testcases = compile_testcases(&snapshot);

    assert_eq!(testcases.len(), 1);
    assert_eq!(testcases[0].case_id, "tc-req-feature-behavior-condition");
    assert_eq!(testcases[0].axis, vec!["ui", "workflow"]);
    assert_eq!(testcases[0].expected, vec!["result"]);
    assert_eq!(testcases[0].steps, vec!["click the button"]);
}

#[test]
fn verification_engine_classifies_loaded_candidates_without_io() {
    let candidate = PendingCandidate {
        case_id: "tc-1".to_string(),
        feature_id: "feature-1".to_string(),
        original_event_id: "event-1".to_string(),
        target_tree_sha: Some("target".to_string()),
        current_tree_sha: Some("current".to_string()),
        current_event: Some(ReflectedChange {
            event_id: "event-2".to_string(),
            from_milestone: "v2".to_string(),
            to_milestone: "v3".to_string(),
        }),
        reexecuted: false,
    };

    assert_eq!(
        evaluate_pending_candidate(&candidate),
        VerificationStatus::Stale
    );

    let pending = PendingCandidate {
        current_tree_sha: Some("target".to_string()),
        ..candidate.clone()
    };
    assert_eq!(
        evaluate_pending_candidate(&pending),
        VerificationStatus::Pending
    );

    let current = PendingCandidate {
        reexecuted: true,
        ..candidate.clone()
    };
    assert_eq!(
        evaluate_pending_candidate(&current),
        VerificationStatus::Current
    );

    let unknown = PendingCandidate {
        current_tree_sha: None,
        current_event: None,
        ..candidate
    };
    assert_eq!(
        evaluate_pending_candidate(&unknown),
        VerificationStatus::Unknown
    );
}

#[test]
fn commit_ref_preserves_the_git_reference_kind_and_value() {
    let milestone = CommitRef::milestone("v1");
    let commit = CommitRef::commit("HEAD~1");

    assert_eq!(milestone.as_git_ref(), "v1");
    assert_eq!(commit.as_git_ref(), "HEAD~1");
    assert!(milestone.is_milestone());
    assert!(!commit.is_milestone());
}
