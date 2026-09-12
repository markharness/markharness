#![allow(clippy::disallowed_methods)]

use std::path::Path;
use std::process::Command;

use markharness::knowledge_source::{
    GitTreeKnowledgeSource, KnowledgeSource, WorkingTreeKnowledgeSource,
};

fn git(root: &Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .unwrap()
            .success()
    );
}

fn write_chain(root: &Path, step: &str) {
    let base = root.join(".markharness/knowledge/features/checkout/pay/card");
    std::fs::create_dir_all(&base).unwrap();
    std::fs::create_dir_all(root.join(".markharness/knowledge/requirements/shop")).unwrap();
    std::fs::write(
        root.join(".markharness/knowledge/requirements/shop/requirement.yml"),
        "id: shop\nsource: native\nlabel: Shop\naxis: []\nuid: 01ARZ3NDEKTSV4RRFFQ69G5FAV\n",
    )
    .unwrap();
    std::fs::write(
        root.join(".markharness/knowledge/features/checkout/feature.yml"),
        "id: checkout\nrequirement_uids: [01ARZ3NDEKTSV4RRFFQ69G5FAV]\nlabel: Checkout\naxis: []\n",
    )
    .unwrap();
    std::fs::write(
        root.join(".markharness/knowledge/features/checkout/pay/behavior.yml"),
        "id: pay\nfeature: checkout\nlabel: Pay\naxis: []\ndescription: Pay.\nprocedures: {}\n",
    )
    .unwrap();
    std::fs::write(
        base.join("scenario.yml"),
        format!("id: card\nbehavior: pay\nlabel: Card\ndescription: A valid card.\nphases:\n  - steps:\n      - action: \"{step}\"\n    results:\n      - \"Confirmed.\"\n"),
    )
    .unwrap();
}

#[test]
fn working_tree_and_git_tree_adapters_load_the_same_snapshot_interface() {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-q", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "Test"]);
    git(repo.path(), &["config", "core.autocrlf", "false"]);
    write_chain(repo.path(), "Committed step.");
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-qm", "base"]);
    write_chain(repo.path(), "Working tree step.");

    let working = WorkingTreeKnowledgeSource::new(
        repo.path()
            .join(markharness::project_root::MARKHARNESS_DIR)
            .join("knowledge"),
    )
    .load_snapshot()
    .unwrap();
    let historical = GitTreeKnowledgeSource::new(repo.path(), "HEAD")
        .load_snapshot()
        .unwrap();

    assert_eq!(
        working.cases[0].phases[0].steps,
        vec!["Working tree step.".to_string()]
    );
    assert_eq!(
        historical.cases[0].phases[0].steps,
        vec!["Committed step.".to_string()]
    );
}
