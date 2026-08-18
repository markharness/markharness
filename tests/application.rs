// Application integration tests build fixtures in temporary workspaces.
#![allow(clippy::disallowed_methods)]

use markharness::application;
use markharness::changes::{CachePolicy, ChangeOptions, ImpactSource};
use markharness::presentation::CommandOutcome;
use std::path::Path;
use std::process::Command;

fn run_git(root: &Path, args: &[&str]) {
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

#[test]
fn generate_use_case_writes_artifacts_and_returns_an_outcome() {
    let root = tempfile::tempdir().unwrap();

    let outcome = application::generate_testcases(root.path()).unwrap();

    assert!(matches!(
        outcome,
        CommandOutcome::Generated { count: 0, .. }
    ));
    assert!(root.path().join("generated/testcases").is_dir());
    assert!(
        root.path()
            .join("generated/traceability-index.json")
            .is_file()
    );
}

#[test]
fn compute_changes_use_case_writes_change_events_and_returns_an_outcome() {
    let root = tempfile::tempdir().unwrap();
    run_git(root.path(), &["init", "-q"]);
    run_git(root.path(), &["config", "user.email", "test@example.com"]);
    run_git(root.path(), &["config", "user.name", "Test"]);
    std::fs::write(root.path().join("README.md"), "one\n").unwrap();
    run_git(root.path(), &["add", "-A"]);
    run_git(root.path(), &["commit", "-q", "-m", "m1"]);
    run_git(root.path(), &["tag", "m1"]);
    std::fs::write(root.path().join("README.md"), "two\n").unwrap();
    run_git(root.path(), &["add", "-A"]);
    run_git(root.path(), &["commit", "-q", "-m", "m2"]);
    run_git(root.path(), &["tag", "m2"]);

    let outcome = application::compute_changes(
        root.path(),
        "m1",
        "m2",
        ChangeOptions {
            cache: CachePolicy::Bypass,
            impact_source: ImpactSource::HistoricalTree,
        },
    )
    .unwrap();

    assert_eq!(
        outcome,
        CommandOutcome::ChangesComputed {
            count: 0,
            to: "m2".to_string(),
        }
    );
    assert!(root.path().join("changes/m2.yaml").is_file());
}

#[test]
fn verify_pending_use_case_preserves_the_domain_error_when_no_pair_exists() {
    let root = tempfile::tempdir().unwrap();

    let result = application::verify_pending(root.path(), None, true, false);

    assert!(matches!(
        result,
        Err(markharness::verify::PendingError::NoMilestonePair)
    ));
}
