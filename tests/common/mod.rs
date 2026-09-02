//! Shared fixture builders for identity-model integration tests. Not a
//! test binary itself (`tests/common/` is excluded from Cargo's automatic
//! `tests/*.rs` discovery) — pulled in via `mod common;` by the tests that
//! need it.
#![allow(clippy::disallowed_methods, dead_code)]

use std::path::Path;

/// Writes a full req-todo -> `feature_id` -> todo-add-task ->
/// todo-add-task-empty-input -> expected/001.yml tree, none of it
/// migrated yet (no `uid:` anywhere).
pub fn write_full_tree(root: &Path, feature_id: &str) {
    let knowledge = root.join(".markharness/knowledge/req-todo");
    let base = knowledge
        .join(feature_id)
        .join("todo-add-task/todo-add-task-empty-input");
    std::fs::create_dir_all(base.join("expected")).unwrap();
    std::fs::write(
        knowledge.join("requirement.yml"),
        "id: req-todo\nlabel: req-todo\naxis: []\n",
    )
    .unwrap();
    std::fs::write(
        knowledge.join(feature_id).join("feature.yml"),
        format!("id: {feature_id}\nrequirement: req-todo\nlabel: todo\naxis: []\n"),
    )
    .unwrap();
    std::fs::write(
        base.parent().unwrap().join("behavior.yml"),
        format!(
            "id: todo-add-task\nfeature: {feature_id}\nlabel: todo-add-task\naxis: []\ndescription: |\n  User adds a task.\npreconditions:\n  - \"Press the add button.\"\n"
        ),
    )
    .unwrap();
    std::fs::write(
        base.join("condition.yml"),
        "id: todo-add-task-empty-input\nbehavior: todo-add-task\nlabel: todo-add-task-empty-input\ndescription: |\n  Title is empty.\nsteps:\n  - \"Do it.\"\nadditional_preconditions: []\n",
    )
    .unwrap();
    std::fs::write(
        base.join("expected/001.yml"),
        "id: todo-add-task-empty-input-001\ncondition: todo-add-task-empty-input\ndescription: |\n  Shows a validation error.\nresults:\n  - \"Confirmed.\"\n",
    )
    .unwrap();
}
