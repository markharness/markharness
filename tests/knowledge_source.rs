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
    let base = root.join(".markharness/knowledge/shop/checkout/pay/card");
    std::fs::create_dir_all(base.join("expected")).unwrap();
    std::fs::write(
        root.join(".markharness/knowledge/shop/requirement.yml"),
        "id: shop\nlabel: Shop\naxis: []\n",
    )
    .unwrap();
    std::fs::write(
        root.join(".markharness/knowledge/shop/checkout/feature.yml"),
        "id: checkout\nrequirement: shop\nlabel: Checkout\naxis: []\n",
    )
    .unwrap();
    std::fs::write(
        root.join(".markharness/knowledge/shop/checkout/pay/behavior.yml"),
        "id: pay\nfeature: checkout\nlabel: Pay\naxis: []\ndescription: Pay.\npreconditions:\n  - \"Enter the card number.\"\n",
    )
    .unwrap();
    std::fs::write(
        base.join("condition.yml"),
        format!("id: card\nbehavior: pay\nlabel: Card\ndescription: A valid card.\nsteps:\n  - \"{step}\"\nadditional_preconditions: []\n"),
    )
    .unwrap();
    std::fs::write(
        base.join("expected/001.yml"),
        "id: accepted\ncondition: card\ndescription: Accepted.\nresults:\n  - \"Confirmed.\"\n",
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
        working.cases[0].condition_steps,
        vec!["Working tree step.".to_string()]
    );
    assert_eq!(
        historical.cases[0].condition_steps,
        vec!["Committed step.".to_string()]
    );
}
