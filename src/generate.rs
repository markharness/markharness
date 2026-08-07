use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::knowledge::{
    parse_behavior, parse_condition, parse_expected_result, parse_feature, parse_requirement,
};

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct GeneratedFrom {
    pub requirement: String,
    pub feature: String,
    pub behavior: String,
    pub condition: String,
    pub expected_results: Vec<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct TestCase {
    pub case_id: String,
    pub generated_from: GeneratedFrom,
    pub title: String,
    pub steps: Vec<String>,
    pub expected: Vec<String>,
    /// Requirement/Feature/Behavior の axis を合成(union)したもの。決定性のため
    /// 重複除去のうえソートする(§3.4 axisの継承)。
    pub axis: Vec<String>,
}

/// Deduplicates and sorts axis values from multiple hierarchy levels into one
/// deterministic list, independent of input order or duplication.
fn union_axis(sources: &[&[String]]) -> Vec<String> {
    let mut union: Vec<String> = sources
        .iter()
        .flat_map(|axis| axis.iter().cloned())
        .collect();
    union.sort();
    union.dedup();
    union
}

impl TestCase {
    /// The base name (without extension) used for `generated/testcases/<file_stem>.yml`.
    pub fn file_stem(&self) -> &str {
        &self.generated_from.condition
    }
}

fn sorted_subdirs(dir: &Path) -> io::Result<Vec<PathBuf>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut dirs: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    Ok(dirs)
}

/// Recursively searches `root` for directories directly containing `marker_file`,
/// stopping the search along a branch as soon as a match is found.
fn find_dirs_with_marker(root: &Path, marker_file: &str) -> io::Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if dir.join(marker_file).is_file() {
            found.push(dir);
            continue;
        }
        for child in sorted_subdirs(&dir)? {
            stack.push(child);
        }
    }
    found.sort();
    Ok(found)
}

pub fn generate_testcases(knowledge_root: &Path) -> io::Result<Vec<TestCase>> {
    let mut testcases = Vec::new();

    for requirement_dir in sorted_subdirs(knowledge_root)? {
        let requirement_path = requirement_dir.join("requirement.yml");
        if !requirement_path.is_file() {
            continue;
        }
        let requirement = parse_requirement(&fs::read_to_string(&requirement_path)?)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        for feature_dir in sorted_subdirs(&requirement_dir)? {
            let feature_path = feature_dir.join("feature.yml");
            if !feature_path.is_file() {
                continue;
            }
            let feature = parse_feature(&fs::read_to_string(&feature_path)?)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

            for behavior_dir in find_dirs_with_marker(&feature_dir, "behavior.yml")? {
                let behavior_path = behavior_dir.join("behavior.yml");
                let behavior = parse_behavior(&fs::read_to_string(&behavior_path)?)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

                for condition_dir in find_dirs_with_marker(&behavior_dir, "condition.yml")? {
                    let condition_path = condition_dir.join("condition.yml");
                    let condition = parse_condition(&fs::read_to_string(&condition_path)?)
                        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

                    let expected_dir = condition_dir.join("expected");
                    if !expected_dir.is_dir() {
                        continue;
                    }
                    let mut expected_paths: Vec<PathBuf> = fs::read_dir(&expected_dir)?
                        .filter_map(|entry| entry.ok())
                        .map(|entry| entry.path())
                        .filter(|path| path.is_file())
                        .collect();
                    expected_paths.sort();
                    if expected_paths.is_empty() {
                        continue;
                    }

                    let mut expected_results = Vec::new();
                    let mut expected_texts = Vec::new();
                    for expected_path in &expected_paths {
                        let expected =
                            parse_expected_result(&fs::read_to_string(expected_path)?)
                                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                        expected_results.push(expected.id);
                        expected_texts.push(expected.description);
                    }

                    let axis = union_axis(&[&requirement.axis, &feature.axis, &behavior.axis]);

                    testcases.push(TestCase {
                        case_id: format!("tc-{}-001", condition.id),
                        generated_from: GeneratedFrom {
                            requirement: requirement.id.clone(),
                            feature: feature.id.clone(),
                            behavior: behavior.id.clone(),
                            condition: condition.id.clone(),
                            expected_results,
                        },
                        title: condition.description,
                        steps: vec![behavior.description.clone()],
                        expected: expected_texts,
                        axis,
                    });
                }
            }
        }
    }

    testcases.sort_by(|a, b| a.case_id.cmp(&b.case_id));
    Ok(testcases)
}

pub fn serialize_testcase(testcase: &TestCase) -> String {
    serde_yaml_ng::to_string(testcase).expect("TestCase serialization is infallible")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_requirement(root: &std::path::Path, requirement: &str, axis: &[&str]) {
        let dir = root.join("knowledge").join(requirement);
        fs::create_dir_all(&dir).unwrap();
        let axis_line = axis.join(", ");
        fs::write(
            dir.join("requirement.yml"),
            format!("id: {requirement}\nlabel: {requirement}\naxis: [{axis_line}]\n"),
        )
        .unwrap();
    }

    fn write_feature(root: &std::path::Path, requirement: &str, feature: &str, axis: &[&str]) {
        let dir = root.join("knowledge").join(requirement).join(feature);
        fs::create_dir_all(&dir).unwrap();
        let axis_line = axis.join(", ");
        fs::write(
            dir.join("feature.yml"),
            format!(
                "id: {feature}\nrequirement: {requirement}\nlabel: {feature}\naxis: [{axis_line}]\n"
            ),
        )
        .unwrap();
    }

    fn write_behavior(
        root: &std::path::Path,
        requirement: &str,
        feature: &str,
        behavior: &str,
        description: &str,
    ) {
        let dir = root
            .join("knowledge")
            .join(requirement)
            .join(feature)
            .join(behavior);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("behavior.yml"),
            format!(
                "id: {behavior}\nfeature: {feature}\nlabel: {behavior}\naxis: [ui]\ndescription: |\n  {description}\n"
            ),
        )
        .unwrap();
    }

    fn write_condition(
        root: &std::path::Path,
        requirement: &str,
        feature: &str,
        behavior: &str,
        condition: &str,
        description: &str,
    ) {
        let dir = root
            .join("knowledge")
            .join(requirement)
            .join(feature)
            .join(behavior)
            .join(condition);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("condition.yml"),
            format!(
                "id: {condition}\nbehavior: {behavior}\nlabel: {condition}\ndescription: |\n  {description}\n"
            ),
        )
        .unwrap();
    }

    #[allow(clippy::too_many_arguments)]
    fn write_expected(
        root: &std::path::Path,
        requirement: &str,
        feature: &str,
        behavior: &str,
        condition: &str,
        seq: &str,
        id: &str,
        description: &str,
    ) {
        let dir = root
            .join("knowledge")
            .join(requirement)
            .join(feature)
            .join(behavior)
            .join(condition)
            .join("expected");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(format!("{seq}.yml")),
            format!("id: {id}\ncondition: {condition}\ndescription: |\n  {description}\n"),
        )
        .unwrap();
    }

    #[test]
    fn generates_empty_list_for_empty_knowledge_dir() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        let testcases = generate_testcases(&dir.path().join("knowledge")).unwrap();

        assert!(testcases.is_empty());
    }

    #[test]
    fn generates_single_testcase_aggregating_all_expected_files_under_one_condition() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-todo", &["security"]);
        write_feature(dir.path(), "req-todo", "todo", &["ui", "data"]);
        write_behavior(
            dir.path(),
            "req-todo",
            "todo",
            "todo-add-task",
            "User adds a task.",
        );
        write_condition(
            dir.path(),
            "req-todo",
            "todo",
            "todo-add-task",
            "todo-add-task-empty-input",
            "Title is empty.",
        );
        write_expected(
            dir.path(),
            "req-todo",
            "todo",
            "todo-add-task",
            "todo-add-task-empty-input",
            "001",
            "todo-add-task-empty-input-001",
            "Shows a validation error.",
        );

        let testcases = generate_testcases(&dir.path().join("knowledge")).unwrap();

        assert_eq!(testcases.len(), 1);
        let tc = &testcases[0];
        assert_eq!(tc.case_id, "tc-todo-add-task-empty-input-001");
        assert_eq!(tc.generated_from.requirement, "req-todo");
        assert_eq!(tc.generated_from.feature, "todo");
        assert_eq!(tc.generated_from.behavior, "todo-add-task");
        assert_eq!(tc.generated_from.condition, "todo-add-task-empty-input");
        assert_eq!(
            tc.generated_from.expected_results,
            vec!["todo-add-task-empty-input-001".to_string()]
        );
        assert_eq!(tc.title, "Title is empty.\n");
        assert_eq!(tc.steps, vec!["User adds a task.\n".to_string()]);
        assert_eq!(tc.expected, vec!["Shows a validation error.\n".to_string()]);
    }

    #[test]
    fn aggregates_multiple_expected_files_into_a_single_testcase() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-todo", &["security"]);
        write_feature(dir.path(), "req-todo", "todo", &["ui"]);
        write_behavior(
            dir.path(),
            "req-todo",
            "todo",
            "todo-complete-task",
            "User checks a task.",
        );
        write_condition(
            dir.path(),
            "req-todo",
            "todo",
            "todo-complete-task",
            "todo-complete-task-toggle-done",
            "Task is unchecked.",
        );
        write_expected(
            dir.path(),
            "req-todo",
            "todo",
            "todo-complete-task",
            "todo-complete-task-toggle-done",
            "001",
            "todo-complete-task-toggle-done-001",
            "Task becomes done.",
        );
        write_expected(
            dir.path(),
            "req-todo",
            "todo",
            "todo-complete-task",
            "todo-complete-task-toggle-done",
            "002",
            "todo-complete-task-toggle-done-002",
            "completedAt is recorded.",
        );

        let testcases = generate_testcases(&dir.path().join("knowledge")).unwrap();

        assert_eq!(testcases.len(), 1);
        let tc = &testcases[0];
        assert_eq!(tc.case_id, "tc-todo-complete-task-toggle-done-001");
        assert_eq!(
            tc.generated_from.expected_results,
            vec![
                "todo-complete-task-toggle-done-001".to_string(),
                "todo-complete-task-toggle-done-002".to_string(),
            ]
        );
        assert_eq!(
            tc.expected,
            vec![
                "Task becomes done.\n".to_string(),
                "completedAt is recorded.\n".to_string(),
            ]
        );
    }

    #[test]
    fn sorts_testcases_by_case_id_across_multiple_features() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-todo", &["security"]);
        write_feature(dir.path(), "req-todo", "todo", &["ui"]);
        write_behavior(
            dir.path(),
            "req-todo",
            "todo",
            "todo-add-task",
            "User adds a task.",
        );
        write_condition(
            dir.path(),
            "req-todo",
            "todo",
            "todo-add-task",
            "todo-add-task-empty-input",
            "Title is empty.",
        );
        write_expected(
            dir.path(),
            "req-todo",
            "todo",
            "todo-add-task",
            "todo-add-task-empty-input",
            "001",
            "todo-add-task-empty-input-001",
            "Shows a validation error.",
        );

        write_requirement(dir.path(), "req-enemy", &["combat"]);
        write_feature(dir.path(), "req-enemy", "enemy", &["combat"]);
        write_behavior(
            dir.path(),
            "req-enemy",
            "enemy",
            "enemy-attack",
            "Enemy attacks.",
        );
        write_condition(
            dir.path(),
            "req-enemy",
            "enemy",
            "enemy-attack",
            "enemy-attack-melee-range",
            "Enemy is in melee range.",
        );
        write_expected(
            dir.path(),
            "req-enemy",
            "enemy",
            "enemy-attack",
            "enemy-attack-melee-range",
            "001",
            "enemy-attack-melee-range-001",
            "Deals damage.",
        );

        let testcases = generate_testcases(&dir.path().join("knowledge")).unwrap();

        assert_eq!(testcases.len(), 2);
        assert_eq!(testcases[0].case_id, "tc-enemy-attack-melee-range-001");
        assert_eq!(testcases[1].case_id, "tc-todo-add-task-empty-input-001");
    }

    #[test]
    fn produces_no_testcase_for_condition_without_expected_files() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-todo", &["security"]);
        write_feature(dir.path(), "req-todo", "todo", &["ui"]);
        write_behavior(
            dir.path(),
            "req-todo",
            "todo",
            "todo-add-task",
            "User adds a task.",
        );
        write_condition(
            dir.path(),
            "req-todo",
            "todo",
            "todo-add-task",
            "todo-add-task-empty-input",
            "Title is empty.",
        );

        let testcases = generate_testcases(&dir.path().join("knowledge")).unwrap();

        assert!(testcases.is_empty());
    }

    #[test]
    fn produces_no_testcase_for_feature_without_behavior() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-todo", &["security"]);
        write_feature(dir.path(), "req-todo", "todo", &["ui"]);

        let testcases = generate_testcases(&dir.path().join("knowledge")).unwrap();

        assert!(testcases.is_empty());
    }

    #[test]
    fn generate_is_deterministic_across_repeated_runs() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-todo", &["security"]);
        write_feature(dir.path(), "req-todo", "todo", &["ui"]);
        write_behavior(
            dir.path(),
            "req-todo",
            "todo",
            "todo-add-task",
            "User adds a task.",
        );
        write_condition(
            dir.path(),
            "req-todo",
            "todo",
            "todo-add-task",
            "todo-add-task-empty-input",
            "Title is empty.",
        );
        write_expected(
            dir.path(),
            "req-todo",
            "todo",
            "todo-add-task",
            "todo-add-task-empty-input",
            "001",
            "todo-add-task-empty-input-001",
            "Shows a validation error.",
        );

        let first: Vec<String> = generate_testcases(&dir.path().join("knowledge"))
            .unwrap()
            .iter()
            .map(serialize_testcase)
            .collect();
        let second: Vec<String> = generate_testcases(&dir.path().join("knowledge"))
            .unwrap()
            .iter()
            .map(serialize_testcase)
            .collect();

        assert_eq!(first, second);
    }

    #[test]
    fn serialized_testcase_contains_no_leading_comment() {
        let testcase = TestCase {
            case_id: "tc-todo-add-task-empty-input-001".to_string(),
            generated_from: GeneratedFrom {
                requirement: "req-todo".to_string(),
                feature: "todo".to_string(),
                behavior: "todo-add-task".to_string(),
                condition: "todo-add-task-empty-input".to_string(),
                expected_results: vec!["todo-add-task-empty-input-001".to_string()],
            },
            title: "Title is empty.".to_string(),
            steps: vec!["User adds a task.".to_string()],
            expected: vec!["Shows a validation error.".to_string()],
            axis: vec!["ui".to_string()],
        };

        let yaml = serialize_testcase(&testcase);

        assert!(!yaml.starts_with('#'));
        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(
            parsed["case_id"].as_str(),
            Some("tc-todo-add-task-empty-input-001")
        );
        assert_eq!(parsed["generated_from"]["feature"].as_str(), Some("todo"));
    }

    #[test]
    fn testcase_axis_is_union_of_requirement_feature_and_behavior_axis() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_requirement(dir.path(), "req-todo", &["security", "ui"]);
        write_feature(dir.path(), "req-todo", "todo", &["ui", "data"]);
        write_behavior(
            dir.path(),
            "req-todo",
            "todo",
            "todo-add-task",
            "User adds a task.",
        );
        write_condition(
            dir.path(),
            "req-todo",
            "todo",
            "todo-add-task",
            "todo-add-task-empty-input",
            "Title is empty.",
        );
        write_expected(
            dir.path(),
            "req-todo",
            "todo",
            "todo-add-task",
            "todo-add-task-empty-input",
            "001",
            "todo-add-task-empty-input-001",
            "Shows a validation error.",
        );

        let testcases = generate_testcases(&dir.path().join("knowledge")).unwrap();

        // write_behavior always uses axis [ui]; combined with requirement's
        // [security, ui] and feature's [ui, data], duplicates must collapse.
        assert_eq!(
            testcases[0].axis,
            vec!["data".to_string(), "security".to_string(), "ui".to_string()]
        );
    }

    #[test]
    fn file_stem_matches_condition_id() {
        let testcase = TestCase {
            case_id: "tc-todo-add-task-empty-input-001".to_string(),
            generated_from: GeneratedFrom {
                requirement: "req-todo".to_string(),
                feature: "todo".to_string(),
                behavior: "todo-add-task".to_string(),
                condition: "todo-add-task-empty-input".to_string(),
                expected_results: vec!["todo-add-task-empty-input-001".to_string()],
            },
            title: "Title is empty.".to_string(),
            steps: vec!["User adds a task.".to_string()],
            expected: vec!["Shows a validation error.".to_string()],
            axis: vec!["ui".to_string()],
        };

        assert_eq!(testcase.file_stem(), "todo-add-task-empty-input");
    }
}
