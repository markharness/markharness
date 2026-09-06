use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use crate::generate::{generate_testcases, list_files_recursive, serialize_testcase};
use crate::traceability::{build_index, serialize_index};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum DiffKind {
    Added,
    Removed,
    Changed,
}

#[derive(Debug, PartialEq, Eq)]
pub struct DiffEntry {
    /// Path relative to `generated/` (not `generated/testcases/`), so a
    /// caller can print `generated/{file_name}` uniformly for every entry.
    /// Testcase files are prefixed `testcases/...` (mirroring `knowledge/`'s
    /// nesting since Step A); the traceability index is the bare
    /// `traceability-index.json`, which actually lives directly under
    /// `generated/`, not under `generated/testcases/`. Forward-slash
    /// separated regardless of platform.
    pub file_name: String,
    pub kind: DiffKind,
}

/// Forward-slash-normalizes a path relative to `generated/testcases/`, then
/// prefixes it with `testcases/` so it is relative to `generated/` like
/// every other `DiffEntry::file_name` (see that field's doc comment).
fn to_diff_key(relative_path: &Path) -> String {
    format!(
        "testcases/{}",
        relative_path.to_string_lossy().replace('\\', "/")
    )
}

fn read_existing_testcases(generated_dir: &Path) -> io::Result<BTreeMap<String, String>> {
    let mut existing = BTreeMap::new();
    for relative_path in list_files_recursive(generated_dir)? {
        let content = fs::read_to_string(generated_dir.join(&relative_path))?;
        existing.insert(to_diff_key(&relative_path), content);
    }
    Ok(existing)
}

/// Regenerates testcases from `root/knowledge` and compares them against the
/// committed files in `root/generated/testcases/`, without writing anything.
pub fn diff_generated_testcases(root: &Path) -> io::Result<Vec<DiffEntry>> {
    let testcases = generate_testcases(
        &root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge"),
    )?;
    let mut expected: BTreeMap<String, String> = BTreeMap::new();
    for testcase in &testcases {
        let key = to_diff_key(&testcase.relative_path());
        expected.insert(key, serialize_testcase(testcase));
    }

    let existing = read_existing_testcases(
        &root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("generated")
            .join("testcases"),
    )?;

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

    let index = build_index(&testcases);
    let expected_index_json = serialize_index(&index);
    let index_path = root
        .join(crate::project_root::MARKHARNESS_DIR)
        .join("generated")
        .join("traceability-index.json");
    match fs::read_to_string(&index_path) {
        Err(_) => diffs.push(DiffEntry {
            file_name: "traceability-index.json".to_string(),
            kind: DiffKind::Added,
        }),
        Ok(existing_json) if existing_json != expected_index_json => diffs.push(DiffEntry {
            file_name: "traceability-index.json".to_string(),
            kind: DiffKind::Changed,
        }),
        Ok(_) => {}
    }

    diffs.sort_by(|a, b| a.file_name.cmp(&b.file_name));
    Ok(diffs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_knowledge_todo_add_task(root: &Path) {
        let dir = root
            .join(".markharness/knowledge/features/todo/todo-add-task/todo-add-task-empty-input");
        fs::create_dir_all(&dir).unwrap();
        fs::create_dir_all(root.join(".markharness/knowledge/requirements/req-todo")).unwrap();
        fs::write(
            root.join(".markharness/knowledge/requirements/req-todo/requirement.yml"),
            "id: req-todo\nlabel: req-todo\naxis: [ui]\n",
        )
        .unwrap();
        fs::write(
            root.join(".markharness/knowledge/features/todo/feature.yml"),
            "id: todo\nrequirement_ids: [req-todo]\nlabel: todo\naxis: [ui]\n",
        )
        .unwrap();
        fs::write(
            root.join(".markharness/knowledge/features/todo/todo-add-task/behavior.yml"),
            "id: todo-add-task\nfeature: todo\nlabel: todo-add-task\naxis: [ui]\ndescription: |\n  User adds a task.\nprocedures: {}\n",
        )
        .unwrap();
        fs::write(
            dir.join("scenario.yml"),
            "id: todo-add-task-empty-input\nbehavior: todo-add-task\nlabel: todo-add-task-empty-input\ndescription: |\n  Title is empty.\nphases:\n  - steps:\n      - action: \"Press the add button.\"\n    results:\n      - \"Shows a validation error.\"\n",
        )
        .unwrap();
    }

    /// Writes `generated/traceability-index.json` matching a fresh
    /// regeneration from `root/knowledge`, so tests can isolate the
    /// testcases-file diff behavior from the index-file diff behavior.
    fn write_matching_index(root: &Path) {
        let testcases = generate_testcases(
            &root
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge"),
        )
        .unwrap();
        let index = build_index(&testcases);
        fs::write(
            root.join(".markharness/generated/traceability-index.json"),
            serialize_index(&index),
        )
        .unwrap();
    }

    #[test]
    fn reports_no_diff_when_generated_dir_missing_and_knowledge_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_matching_index(dir.path());

        let diffs = diff_generated_testcases(dir.path()).unwrap();

        assert!(diffs.is_empty());
    }

    #[test]
    fn reports_added_when_committed_file_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_knowledge_todo_add_task(dir.path());
        write_matching_index(dir.path());

        let diffs = diff_generated_testcases(dir.path()).unwrap();

        assert_eq!(
            diffs,
            vec![DiffEntry {
                file_name: "testcases/todo/todo-add-task/todo-add-task-empty-input.yml".to_string(),
                kind: DiffKind::Added,
            }]
        );
    }

    #[test]
    fn reports_no_diff_when_committed_file_matches_regeneration() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_knowledge_todo_add_task(dir.path());

        let testcases = generate_testcases(
            &dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge"),
        )
        .unwrap();
        let testcases_dir = dir.path().join(".markharness/generated/testcases");
        for testcase in &testcases {
            let path = testcases_dir.join(testcase.relative_path());
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, serialize_testcase(testcase)).unwrap();
        }
        write_matching_index(dir.path());

        let diffs = diff_generated_testcases(dir.path()).unwrap();

        assert!(diffs.is_empty());
    }

    #[test]
    fn reports_changed_when_committed_file_content_differs() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_knowledge_todo_add_task(dir.path());

        let testcases_dir = dir
            .path()
            .join(".markharness/generated/testcases/todo/todo-add-task");
        fs::create_dir_all(&testcases_dir).unwrap();
        fs::write(
            testcases_dir.join("todo-add-task-empty-input.yml"),
            "stale content\n",
        )
        .unwrap();
        write_matching_index(dir.path());

        let diffs = diff_generated_testcases(dir.path()).unwrap();

        assert_eq!(
            diffs,
            vec![DiffEntry {
                file_name: "testcases/todo/todo-add-task/todo-add-task-empty-input.yml".to_string(),
                kind: DiffKind::Changed,
            }]
        );
    }

    #[test]
    fn reports_removed_when_committed_file_no_longer_generated() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        let testcases_dir = dir
            .path()
            .join(".markharness/generated/testcases/todo/todo-add-task");
        fs::create_dir_all(&testcases_dir).unwrap();
        fs::write(testcases_dir.join("stale-condition.yml"), "stale content\n").unwrap();
        write_matching_index(dir.path());

        let diffs = diff_generated_testcases(dir.path()).unwrap();

        assert_eq!(
            diffs,
            vec![DiffEntry {
                file_name: "testcases/todo/todo-add-task/stale-condition.yml".to_string(),
                kind: DiffKind::Removed,
            }]
        );
    }

    #[test]
    fn reports_added_for_traceability_index_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_knowledge_todo_add_task(dir.path());
        let testcases = generate_testcases(
            &dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge"),
        )
        .unwrap();
        let testcases_dir = dir.path().join(".markharness/generated/testcases");
        for testcase in &testcases {
            let path = testcases_dir.join(testcase.relative_path());
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, serialize_testcase(testcase)).unwrap();
        }

        let diffs = diff_generated_testcases(dir.path()).unwrap();

        assert_eq!(
            diffs,
            vec![DiffEntry {
                file_name: "traceability-index.json".to_string(),
                kind: DiffKind::Added,
            }]
        );
    }

    #[test]
    fn reports_changed_for_traceability_index_when_stale() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_knowledge_todo_add_task(dir.path());
        let testcases = generate_testcases(
            &dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("knowledge"),
        )
        .unwrap();
        let testcases_dir = dir.path().join(".markharness/generated/testcases");
        for testcase in &testcases {
            let path = testcases_dir.join(testcase.relative_path());
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, serialize_testcase(testcase)).unwrap();
        }
        fs::write(
            dir.path()
                .join(".markharness/generated/traceability-index.json"),
            "{\"testcases\":[]}",
        )
        .unwrap();

        let diffs = diff_generated_testcases(dir.path()).unwrap();

        assert_eq!(
            diffs,
            vec![DiffEntry {
                file_name: "traceability-index.json".to_string(),
                kind: DiffKind::Changed,
            }]
        );
    }
}
