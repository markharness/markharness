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

fn write_chain(root: &Path, description: &str) {
    let base = root.join("knowledge/shop/checkout/pay/card");
    std::fs::create_dir_all(base.join("expected")).unwrap();
    std::fs::write(
        root.join("knowledge/shop/requirement.yml"),
        "id: shop\nlabel: Shop\naxis: []\n",
    )
    .unwrap();
    std::fs::write(
        root.join("knowledge/shop/checkout/feature.yml"),
        "id: checkout\nrequirement: shop\nlabel: Checkout\naxis: []\n",
    )
    .unwrap();
    std::fs::write(
        root.join("knowledge/shop/checkout/pay/behavior.yml"),
        "id: pay\nfeature: checkout\nlabel: Pay\naxis: []\ndescription: Pay.\n",
    )
    .unwrap();
    std::fs::write(
        base.join("condition.yml"),
        format!("id: card\nbehavior: pay\nlabel: Card\ndescription: {description}\n"),
    )
    .unwrap();
    std::fs::write(
        base.join("expected/001.yml"),
        "id: accepted\ncondition: card\ndescription: Accepted.\n",
    )
    .unwrap();
}

#[test]
fn working_tree_and_git_tree_adapters_load_the_same_snapshot_interface() {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-q", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "Test"]);
    write_chain(repo.path(), "Committed card.");
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-qm", "base"]);
    write_chain(repo.path(), "Working tree card.");

    let working = WorkingTreeKnowledgeSource::new(repo.path().join("knowledge"))
        .load_snapshot()
        .unwrap();
    let historical = GitTreeKnowledgeSource::new(repo.path(), "HEAD")
        .load_snapshot()
        .unwrap();

    assert_eq!(working.cases[0].condition_description, "Working tree card.");
    assert_eq!(historical.cases[0].condition_description, "Committed card.");
}
