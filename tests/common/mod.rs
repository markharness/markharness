//! Shared fixture builders for identity-model integration tests. Not a
//! test binary itself (`tests/common/` is excluded from Cargo's automatic
//! `tests/*.rs` discovery) — pulled in via `mod common;` by the tests that
//! need it.
#![allow(clippy::disallowed_methods, dead_code)]

use std::path::Path;

/// Writes a full req-todo -> `feature_id` -> todo-add-task ->
/// todo-add-task-empty-input scenario tree, none of it migrated yet (no
/// `uid:` anywhere). The Feature's `requirement_uids` still holds
/// `req-todo`'s *display id* (schema requires `minItems: 1`, so it can't be
/// left empty) — exactly the pre-migration placeholder Issue #44 is about.
/// `generate::load_knowledge_snapshot`'s requirement-uid resolution falls
/// back to the raw value when it doesn't match a real Requirement `uid`
/// (informational field only, ADR 0017 §1・§3), and `identity migrate`
/// resolves it to `req-todo`'s real `uid` once that Requirement is migrated
/// too (see `feature_ops::build_migration_plan`'s Pass 3).
pub fn write_full_tree(root: &Path, feature_id: &str) {
    let knowledge = root
        .join(".markharness/knowledge/features")
        .join(feature_id);
    let base = knowledge.join("todo-add-task/todo-add-task-empty-input");
    std::fs::create_dir_all(&base).unwrap();
    std::fs::create_dir_all(root.join(".markharness/knowledge/requirements/req-todo")).unwrap();
    std::fs::write(
        root.join(".markharness/knowledge/requirements/req-todo/requirement.yml"),
        "id: req-todo\nsource: native\nlabel: req-todo\naxis: []\n",
    )
    .unwrap();
    std::fs::write(
        knowledge.join("feature.yml"),
        format!("id: {feature_id}\nrequirement_uids: [req-todo]\nlabel: todo\naxis: []\n"),
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
