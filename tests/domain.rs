use markharness::changes::CommitRef;
use markharness::generate::{
    CaseFilePaths, KnowledgeCaseSnapshot, KnowledgeSnapshot, Phase, compile_testcases,
};

#[test]
fn testcase_compiler_compiles_a_snapshot_without_filesystem_access() {
    let snapshot = KnowledgeSnapshot {
        cases: vec![KnowledgeCaseSnapshot {
            requirement_ids: vec!["req".to_string()],
            feature_id: "feature".to_string(),
            feature_uid: None,
            feature_axis: vec!["workflow".to_string()],
            behavior_id: "behavior".to_string(),
            behavior_axis: vec!["ui".to_string()],
            scenario_id: "scenario".to_string(),
            scenario_uid: None,
            phases: vec![Phase {
                steps: vec!["confirm the state".to_string()],
                results: vec!["result".to_string()],
            }],
            case_files: CaseFilePaths::default(),
        }],
    };

    let testcases = compile_testcases(&snapshot);

    assert_eq!(testcases.len(), 1);
    assert_eq!(testcases[0].case_id, "tc-feature-behavior-scenario");
    assert_eq!(testcases[0].axis, vec!["ui", "workflow"]);
    assert_eq!(
        testcases[0].phases,
        vec![Phase {
            steps: vec!["confirm the state".to_string()],
            results: vec!["result".to_string()],
        }]
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
