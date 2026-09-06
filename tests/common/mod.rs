//! Shared fixture builders for identity-model integration tests. Not a
//! test binary itself (`tests/common/` is excluded from Cargo's automatic
//! `tests/*.rs` discovery) — pulled in via `mod common;` by the tests that
//! need it.
#![allow(clippy::disallowed_methods, dead_code)]

use std::path::Path;

/// Writes a full req-todo -> `feature_id` -> todo-add-task ->
/// todo-add-task-empty-input scenario tree, none of it migrated yet (no
/// `uid:` anywhere).
pub fn write_full_tree(root: &Path, feature_id: &str) {
    let knowledge = root
        .join(".markharness/knowledge/features")
        .join(feature_id);
    let base = knowledge.join("todo-add-task/todo-add-task-empty-input");
    std::fs::create_dir_all(&base).unwrap();
    std::fs::create_dir_all(root.join(".markharness/knowledge/requirements/req-todo")).unwrap();
    std::fs::write(
        root.join(".markharness/knowledge/requirements/req-todo/requirement.yml"),
        "id: req-todo\nlabel: req-todo\naxis: []\n",
    )
    .unwrap();
    std::fs::write(
        knowledge.join("feature.yml"),
        format!("id: {feature_id}\nrequirement_ids: [req-todo]\nlabel: todo\naxis: []\n"),
    )
    .unwrap();
    std::fs::write(
        base.parent().unwrap().join("behavior.yml"),
        format!(
            "id: todo-add-task\nfeature: {feature_id}\nlabel: todo-add-task\naxis: []\ndescription: |\n  User adds a task.\nprocedures: {{}}\n"
        ),
    )
    .unwrap();
    std::fs::write(
        base.join("scenario.yml"),
        "id: todo-add-task-empty-input\nbehavior: todo-add-task\nlabel: todo-add-task-empty-input\ndescription: |\n  Title is empty.\nphases:\n  - steps:\n      - action: \"Press the add button.\"\n    results:\n      - \"Shows a validation error.\"\n",
    )
    .unwrap();
}
