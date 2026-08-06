use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::knowledge::{parse_condition, parse_expected_result, parse_feature};

#[derive(Debug, PartialEq, Eq)]
pub struct TestCase {
    pub id: String,
    pub feature_id: String,
    pub condition_id: String,
    pub axis: Vec<String>,
    pub title: String,
    pub expected_result: String,
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

fn find_condition_dirs(feature_dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    let mut stack = vec![feature_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if dir.join("condition.yaml").is_file() {
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

    for feature_dir in sorted_subdirs(knowledge_root)? {
        let feature_path = feature_dir.join("feature.yaml");
        if !feature_path.is_file() {
            continue;
        }
        let feature = parse_feature(&fs::read_to_string(&feature_path)?)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        for condition_dir in find_condition_dirs(&feature_dir)? {
            let condition_path = condition_dir.join("condition.yaml");
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

            for (index, expected_path) in expected_paths.into_iter().enumerate() {
                let seq = index + 1;
                let expected = parse_expected_result(&fs::read_to_string(&expected_path)?)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

                testcases.push(TestCase {
                    id: format!("{}-{}-{:03}", feature.id, condition.id, seq),
                    feature_id: feature.id.clone(),
                    condition_id: condition.id.clone(),
                    axis: feature.axis.clone(),
                    title: format!("{} (#{})", condition.summary, seq),
                    expected_result: format!(
                        "{} (condition: {})",
                        expected.result, condition.summary
                    ),
                });
            }
        }
    }

    testcases.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(testcases)
}

pub fn serialize_testcases(testcases: &[TestCase]) -> String {
    if testcases.is_empty() {
        return "[]\n".to_string();
    }
    let mut out = String::new();
    for tc in testcases {
        out.push_str(&format!("- id: {}\n", tc.id));
        out.push_str(&format!("  feature_id: {}\n", tc.feature_id));
        out.push_str(&format!("  condition_id: {}\n", tc.condition_id));
        out.push_str("  axis:\n");
        for a in &tc.axis {
            out.push_str(&format!("    - {a}\n"));
        }
        out.push_str(&format!("  title: {}\n", tc.title));
        out.push_str(&format!("  expected_result: {}\n", tc.expected_result));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_feature(root: &std::path::Path, feature: &str, axis: &[&str]) {
        let dir = root.join("knowledge").join(feature);
        fs::create_dir_all(&dir).unwrap();
        let axis_lines: String = axis.iter().map(|a| format!("  - {a}\n")).collect();
        fs::write(
            dir.join("feature.yaml"),
            format!("id: {feature}\nkind: feature\naxis:\n{axis_lines}"),
        )
        .unwrap();
    }

    fn write_condition(root: &std::path::Path, feature: &str, condition: &str, summary: &str) {
        let dir = root.join("knowledge").join(feature).join(condition);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("condition.yaml"),
            format!("id: {condition}\nkind: condition\nsummary: {summary}\n"),
        )
        .unwrap();
    }

    fn write_expected(
        root: &std::path::Path,
        feature: &str,
        condition: &str,
        seq: &str,
        result: &str,
    ) {
        let dir = root
            .join("knowledge")
            .join(feature)
            .join(condition)
            .join("expected");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(format!("{seq}.yaml")),
            format!("id: placeholder\nkind: expected-result\nresult: {result}\n"),
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
    fn generates_single_testcase_from_one_feature_condition_expected() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_feature(dir.path(), "player-jump", &["gameplay", "animation"]);
        write_condition(
            dir.path(),
            "player-jump",
            "jump-ground",
            "Jump from the ground and land",
        );
        write_expected(
            dir.path(),
            "player-jump",
            "jump-ground",
            "001",
            "lands safely",
        );

        let testcases = generate_testcases(&dir.path().join("knowledge")).unwrap();

        assert_eq!(testcases.len(), 1);
        let tc = &testcases[0];
        assert_eq!(tc.id, "player-jump-jump-ground-001");
        assert_eq!(tc.feature_id, "player-jump");
        assert_eq!(tc.condition_id, "jump-ground");
        assert_eq!(
            tc.axis,
            vec!["gameplay".to_string(), "animation".to_string()]
        );
        assert_eq!(tc.title, "Jump from the ground and land (#1)");
        assert_eq!(
            tc.expected_result,
            "lands safely (condition: Jump from the ground and land)"
        );
    }

    #[test]
    fn generates_multiple_testcases_in_seq_order_for_multiple_expected_files() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_feature(dir.path(), "player-jump", &["gameplay"]);
        write_condition(dir.path(), "player-jump", "jump-ground", "Jump and land");
        write_expected(
            dir.path(),
            "player-jump",
            "jump-ground",
            "001",
            "lands safely",
        );
        write_expected(
            dir.path(),
            "player-jump",
            "jump-ground",
            "002",
            "falls over",
        );

        let testcases = generate_testcases(&dir.path().join("knowledge")).unwrap();

        assert_eq!(testcases.len(), 2);
        assert_eq!(testcases[0].id, "player-jump-jump-ground-001");
        assert_eq!(testcases[1].id, "player-jump-jump-ground-002");
    }

    #[test]
    fn sorts_testcases_by_id_across_multiple_features() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_feature(dir.path(), "player-jump", &["gameplay"]);
        write_condition(dir.path(), "player-jump", "jump-ground", "Jump and land");
        write_expected(
            dir.path(),
            "player-jump",
            "jump-ground",
            "001",
            "lands safely",
        );

        write_feature(dir.path(), "enemy-attack", &["combat"]);
        write_condition(
            dir.path(),
            "enemy-attack",
            "melee-range",
            "Attack in melee range",
        );
        write_expected(
            dir.path(),
            "enemy-attack",
            "melee-range",
            "001",
            "deals damage",
        );

        let testcases = generate_testcases(&dir.path().join("knowledge")).unwrap();

        assert_eq!(testcases.len(), 2);
        assert_eq!(testcases[0].id, "enemy-attack-melee-range-001");
        assert_eq!(testcases[1].id, "player-jump-jump-ground-001");
    }

    #[test]
    fn produces_no_testcase_for_condition_without_expected_files() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_feature(dir.path(), "player-jump", &["gameplay"]);
        write_condition(dir.path(), "player-jump", "jump-ground", "Jump and land");

        let testcases = generate_testcases(&dir.path().join("knowledge")).unwrap();

        assert!(testcases.is_empty());
    }

    #[test]
    fn generate_is_deterministic_across_repeated_runs() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_feature(dir.path(), "player-jump", &["gameplay"]);
        write_condition(dir.path(), "player-jump", "jump-ground", "Jump and land");
        write_expected(
            dir.path(),
            "player-jump",
            "jump-ground",
            "001",
            "lands safely",
        );
        write_expected(
            dir.path(),
            "player-jump",
            "jump-ground",
            "002",
            "falls over",
        );

        let first =
            serialize_testcases(&generate_testcases(&dir.path().join("knowledge")).unwrap());
        let second =
            serialize_testcases(&generate_testcases(&dir.path().join("knowledge")).unwrap());

        assert_eq!(first, second);
    }

    #[test]
    fn serializes_empty_testcase_list_as_empty_yaml_array() {
        assert_eq!(serialize_testcases(&[]), "[]\n");
    }
}
