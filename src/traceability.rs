use serde::Serialize;

use crate::generate::TestCase;

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct TraceabilityEntry {
    pub case_id: String,
    pub requirement_ids: Vec<String>,
    pub feature: String,
    pub behavior: String,
    pub scenario: String,
    pub axis: Vec<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct TraceabilityIndex {
    pub testcases: Vec<TraceabilityEntry>,
}

/// Builds the Requirement→Feature→Behavior→Scenario→TestCase index (§2番目の
/// 未実装項目, docs/ja/cli-manual.md §2)。`testcases` の順序をそのまま使うため、
/// `generate::generate_testcases` の決定的な出力に依存する。
pub fn build_index(testcases: &[TestCase]) -> TraceabilityIndex {
    TraceabilityIndex {
        testcases: testcases
            .iter()
            .map(|tc| TraceabilityEntry {
                case_id: tc.case_id.clone(),
                requirement_ids: tc.generated_from.requirement_ids.clone(),
                feature: tc.generated_from.feature.clone(),
                behavior: tc.generated_from.behavior.clone(),
                scenario: tc.generated_from.scenario.clone(),
                axis: tc.axis.clone(),
            })
            .collect(),
    }
}

pub fn serialize_index(index: &TraceabilityIndex) -> String {
    let mut json =
        serde_json::to_string_pretty(index).expect("TraceabilityIndex serialization is infallible");
    json.push('\n');
    json
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generate::{CaseFilePaths, GeneratedFrom, Phase};

    fn sample_testcase() -> TestCase {
        TestCase {
            case_id: "tc-todo-add-task-empty-input-001".to_string(),
            case_uid: None,
            case_revision: "test-revision".to_string(),
            case_files: CaseFilePaths::default(),
            generated_from: GeneratedFrom {
                requirement_ids: vec!["req-todo".to_string()],
                feature: "todo".to_string(),
                feature_uid: None,
                behavior: "todo-add-task".to_string(),
                scenario: "todo-add-task-empty-input".to_string(),
            },
            phases: vec![Phase {
                steps: vec!["Do it.".to_string()],
                results: vec!["Shows a validation error.".to_string()],
            }],
            axis: vec!["ui".to_string()],
        }
    }

    #[test]
    fn build_index_maps_generated_from_fields_and_axis() {
        let index = build_index(&[sample_testcase()]);

        assert_eq!(index.testcases.len(), 1);
        let entry = &index.testcases[0];
        assert_eq!(entry.case_id, "tc-todo-add-task-empty-input-001");
        assert_eq!(entry.requirement_ids, vec!["req-todo".to_string()]);
        assert_eq!(entry.feature, "todo");
        assert_eq!(entry.behavior, "todo-add-task");
        assert_eq!(entry.scenario, "todo-add-task-empty-input");
        assert_eq!(entry.axis, vec!["ui".to_string()]);
    }

    #[test]
    fn build_index_returns_empty_testcases_for_empty_input() {
        let index = build_index(&[]);

        assert!(index.testcases.is_empty());
    }

    #[test]
    fn serialize_index_produces_valid_json() {
        let index = build_index(&[sample_testcase()]);

        let json = serialize_index(&index);

        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            parsed["testcases"][0]["case_id"],
            "tc-todo-add-task-empty-input-001"
        );
    }

    #[test]
    fn serialize_index_is_deterministic() {
        let index = build_index(&[sample_testcase()]);

        let first = serialize_index(&index);
        let second = serialize_index(&index);

        assert_eq!(first, second);
    }
}
