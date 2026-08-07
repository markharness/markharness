use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use crate::generate::{generate_testcases, serialize_testcase};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum DiffKind {
    Added,
    Removed,
    Changed,
}

#[derive(Debug, PartialEq, Eq)]
pub struct DiffEntry {
    pub file_name: String,
    pub kind: DiffKind,
}

fn read_existing_testcases(generated_dir: &Path) -> io::Result<BTreeMap<String, String>> {
    let mut existing = BTreeMap::new();
    if !generated_dir.is_dir() {
        return Ok(existing);
    }
    for entry in fs::read_dir(generated_dir)? {
        let path = entry?.path();
        if path.is_file()
            && let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned())
        {
            existing.insert(name, fs::read_to_string(&path)?);
        }
    }
    Ok(existing)
}

/// Regenerates testcases from `root/knowledge` and compares them against the
/// committed files in `root/generated/testcases/`, without writing anything.
pub fn diff_generated_testcases(root: &Path) -> io::Result<Vec<DiffEntry>> {
    let testcases = generate_testcases(&root.join("knowledge"))?;
    let mut expected: BTreeMap<String, String> = BTreeMap::new();
    for testcase in &testcases {
        let file_name = format!("{}.yml", testcase.file_stem());
        expected.insert(file_name, serialize_testcase(testcase));
    }

    let existing = read_existing_testcases(&root.join("generated").join("testcases"))?;

    let mut diffs = Vec::new();
    for (file_name, content) in &expected {
        match existing.get(file_name) {
            None => diffs.push(DiffEntry {
                file_name: file_name.clone(),
                kind: DiffKind::Added,
            }),
            Some(existing_content) if existing_content != content => diffs.push(DiffEntry {
                file_name: file_name.clone(),
                kind: DiffKind::Changed,
            }),
            Some(_) => {}
        }
    }
    for file_name in existing.keys() {
        if !expected.contains_key(file_name) {
            diffs.push(DiffEntry {
                file_name: file_name.clone(),
                kind: DiffKind::Removed,
            });
        }
    }

    diffs.sort_by(|a, b| a.file_name.cmp(&b.file_name));
    Ok(diffs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_knowledge_todo_add_task(root: &Path) {
        let dir = root.join("knowledge/req-todo/todo/todo-add-task/todo-add-task-empty-input");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            root.join("knowledge/req-todo/requirement.yml"),
            "id: req-todo\nlabel: req-todo\naxis: [ui]\n",
        )
        .unwrap();
        fs::write(
            root.join("knowledge/req-todo/todo/feature.yml"),
            "id: todo\nrequirement: req-todo\nlabel: todo\naxis: [ui]\n",
        )
        .unwrap();
        fs::write(
            root.join("knowledge/req-todo/todo/todo-add-task/behavior.yml"),
            "id: todo-add-task\nfeature: todo\nlabel: todo-add-task\naxis: [ui]\ndescription: |\n  User adds a task.\n",
        )
        .unwrap();
        fs::write(
            dir.join("condition.yml"),
            "id: todo-add-task-empty-input\nbehavior: todo-add-task\nlabel: todo-add-task-empty-input\ndescription: |\n  Title is empty.\n",
        )
        .unwrap();
        fs::create_dir_all(dir.join("expected")).unwrap();
        fs::write(
            dir.join("expected/001.yml"),
            "id: todo-add-task-empty-input-001\ncondition: todo-add-task-empty-input\ndescription: |\n  Shows a validation error.\n",
        )
        .unwrap();
    }

    #[test]
    fn reports_no_diff_when_generated_dir_missing_and_knowledge_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        let diffs = diff_generated_testcases(dir.path()).unwrap();

        assert!(diffs.is_empty());
    }

    #[test]
    fn reports_added_when_committed_file_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_knowledge_todo_add_task(dir.path());

        let diffs = diff_generated_testcases(dir.path()).unwrap();

        assert_eq!(
            diffs,
            vec![DiffEntry {
                file_name: "todo-add-task-empty-input.yml".to_string(),
                kind: DiffKind::Added,
            }]
        );
    }

    #[test]
    fn reports_no_diff_when_committed_file_matches_regeneration() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_knowledge_todo_add_task(dir.path());

        let testcases = generate_testcases(&dir.path().join("knowledge")).unwrap();
        let testcases_dir = dir.path().join("generated/testcases");
        fs::create_dir_all(&testcases_dir).unwrap();
        for testcase in &testcases {
            fs::write(
                testcases_dir.join(format!("{}.yml", testcase.file_stem())),
                serialize_testcase(testcase),
            )
            .unwrap();
        }

        let diffs = diff_generated_testcases(dir.path()).unwrap();

        assert!(diffs.is_empty());
    }

    #[test]
    fn reports_changed_when_committed_file_content_differs() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_knowledge_todo_add_task(dir.path());

        let testcases_dir = dir.path().join("generated/testcases");
        fs::create_dir_all(&testcases_dir).unwrap();
        fs::write(
            testcases_dir.join("todo-add-task-empty-input.yml"),
            "stale content\n",
        )
        .unwrap();

        let diffs = diff_generated_testcases(dir.path()).unwrap();

        assert_eq!(
            diffs,
            vec![DiffEntry {
                file_name: "todo-add-task-empty-input.yml".to_string(),
                kind: DiffKind::Changed,
            }]
        );
    }

    #[test]
    fn reports_removed_when_committed_file_no_longer_generated() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        let testcases_dir = dir.path().join("generated/testcases");
        fs::create_dir_all(&testcases_dir).unwrap();
        fs::write(testcases_dir.join("stale-condition.yml"), "stale content\n").unwrap();

        let diffs = diff_generated_testcases(dir.path()).unwrap();

        assert_eq!(
            diffs,
            vec![DiffEntry {
                file_name: "stale-condition.yml".to_string(),
                kind: DiffKind::Removed,
            }]
        );
    }
}
