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
    assert!(
        root.path()
            .join(".markharness/generated/testcases")
            .is_dir()
    );
    assert!(
        root.path()
            .join(".markharness/generated/traceability-index.json")
            .is_file()
    );
}

#[test]
fn generate_use_case_preserves_non_owned_siblings_in_generated_dir() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join(".markharness/generated/extra/nested")).unwrap();
    std::fs::write(root.path().join(".markharness/generated/.gitkeep"), "").unwrap();
    std::fs::write(
        root.path()
            .join(".markharness/generated/extra/nested/note.txt"),
        "keep me\n",
    )
    .unwrap();

    application::generate_testcases(root.path()).unwrap();

    assert!(
        root.path()
            .join(".markharness/generated/.gitkeep")
            .is_file(),
        "generate must not delete the .gitkeep placeholder init left in generated/"
    );
    assert_eq!(
        std::fs::read_to_string(
            root.path()
                .join(".markharness/generated/extra/nested/note.txt")
        )
        .unwrap(),
        "keep me\n",
        "generate must not delete files/directories it does not own"
    );
}

// Windows and default-configuration macOS filesystems are case-insensitive,
// so "testcases" and "TestCases" name the very same directory at the OS
// level there: the setup below can't even create them as two distinct
// entries, let alone exercise the alias-rejection this test targets. Linux
// (this crate's only case-sensitive supported target, and what CI runs) is
// the one platform where this scenario is actually constructible.
#[cfg(target_os = "linux")]
#[test]
fn generate_use_case_fails_without_swapping_when_a_sibling_name_is_a_case_insensitive_alias_of_an_owned_name()
 {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join(".markharness/generated/testcases")).unwrap();
    std::fs::write(
        root.path()
            .join(".markharness/generated/testcases/existing.yml"),
        "existing\n",
    )
    .unwrap();
    std::fs::write(
        root.path()
            .join(".markharness/generated/traceability-index.json"),
        "existing index\n",
    )
    .unwrap();
    std::fs::write(root.path().join(".markharness/generated/.gitkeep"), "").unwrap();
    // Case-insensitive alias of the generator-owned "testcases" name.
    std::fs::create_dir(root.path().join(".markharness/generated/TestCases")).unwrap();

    let result = application::generate_testcases(root.path());

    assert!(
        result.is_err(),
        "expected an error for an owned-name alias, got: {result:?}"
    );
    assert_eq!(
        std::fs::read_to_string(
            root.path()
                .join(".markharness/generated/testcases/existing.yml")
        )
        .unwrap(),
        "existing\n",
        "an alias-name error must not swap in any staged content"
    );
    assert_eq!(
        std::fs::read_to_string(
            root.path()
                .join(".markharness/generated/traceability-index.json")
        )
        .unwrap(),
        "existing index\n"
    );
    assert!(
        root.path()
            .join(".markharness/generated/.gitkeep")
            .is_file()
    );
}

#[test]
fn generate_use_case_preserves_existing_artifacts_when_staging_fails() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join(".markharness/generated/testcases")).unwrap();
    std::fs::write(
        root.path()
            .join(".markharness/generated/testcases/existing.yml"),
        "existing\n",
    )
    .unwrap();
    std::fs::write(root.path().join(".markharness/generated/.gitkeep"), "").unwrap();
    std::fs::create_dir(
        root.path()
            .join(".markharness/generated/traceability-index.json"),
    )
    .unwrap();

    let result = application::generate_testcases(root.path());

    assert!(result.is_err());
    assert_eq!(
        std::fs::read_to_string(
            root.path()
                .join(".markharness/generated/testcases/existing.yml")
        )
        .unwrap(),
        "existing\n"
    );
    assert!(
        root.path()
            .join(".markharness/generated/.gitkeep")
            .is_file(),
        "a staging failure must leave non-owned siblings untouched too"
    );
}

#[test]
fn compute_changes_use_case_writes_change_events_and_returns_an_outcome() {
    let root = tempfile::tempdir().unwrap();
    run_git(root.path(), &["init", "-q"]);
    run_git(root.path(), &["config", "user.email", "test@example.com"]);
    run_git(root.path(), &["config", "user.name", "Test"]);
    run_git(root.path(), &["config", "core.autocrlf", "false"]);
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
    assert!(root.path().join(".markharness/changes/m2.yaml").is_file());
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
