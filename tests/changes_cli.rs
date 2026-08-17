// Integration test fixtures write directly to a scratch repo before
// invoking the CLI binary; that's outside fs_safety's managed-root scope
// (see clippy.toml / src/lib.rs).
#![allow(clippy::disallowed_methods)]

use std::path::Path;
use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_markharness")
}

fn run(args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .output()
        .expect("failed to run markharness binary")
}

fn run_git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

fn write_feature(root: &Path, label: &str) {
    let dir = root.join("knowledge/controls/player-jump");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        root.join("knowledge/controls/requirement.yml"),
        "id: controls\nlabel: controls\naxis: [gameplay]\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("feature.yml"),
        format!("id: player-jump\nrequirement: controls\nlabel: {label}\naxis: [gameplay]\n"),
    )
    .unwrap();
}

fn init_project_with_two_milestones() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let output = run(&["init", "--dir", dir.path().to_str().unwrap()]);
    assert!(output.status.success());
    run_git(dir.path(), &["init", "-q"]);
    run_git(dir.path(), &["config", "user.email", "test@example.com"]);
    run_git(dir.path(), &["config", "user.name", "Test"]);

    write_feature(dir.path(), "v1");
    run_git(dir.path(), &["add", "-A"]);
    run_git(dir.path(), &["commit", "-q", "-m", "v1"]);
    run_git(dir.path(), &["tag", "m1"]);

    write_feature(dir.path(), "v2");
    run_git(dir.path(), &["add", "-A"]);
    run_git(dir.path(), &["commit", "-q", "-m", "v2"]);
    run_git(dir.path(), &["tag", "m2"]);

    let output = run(&[
        "changes",
        "compute",
        "m1",
        "m2",
        "--dir",
        dir.path().to_str().unwrap(),
        "--no-cache",
    ]);
    assert!(
        output.status.success(),
        "changes compute failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    dir
}

fn normalize_tree_shas(yaml: &str) -> String {
    yaml.lines()
        .map(|line| {
            if line.trim_start().starts_with("from_tree_sha:")
                || line.trim_start().starts_with("to_tree_sha:")
            {
                let indent = &line[..line.len() - line.trim_start().len()];
                format!("{indent}{}", line.trim_start().split(':').next().unwrap()) + ": <tree-sha>"
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

#[test]
fn changes_compute_matches_the_stage0_golden_contract() {
    let dir = init_project_with_two_milestones();
    let output = run(&[
        "changes",
        "compute",
        "m1",
        "m2",
        "--dir",
        dir.path().to_str().unwrap(),
        "--no-cache",
    ]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "computed 1 change event(s) into changes/m2.yaml\n"
    );
    let yaml = std::fs::read_to_string(dir.path().join("changes/m2.yaml")).unwrap();

    assert_eq!(
        normalize_tree_shas(&yaml),
        include_str!("fixtures/stage0/changes-m1-m2.golden.yml")
    );
}

#[test]
fn changes_annotate_sets_change_type_on_the_matching_event() {
    let dir = init_project_with_two_milestones();

    let output = run(&[
        "changes",
        "annotate",
        "player-jump--m1--m2",
        "--type",
        "spec-change",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);

    assert!(
        output.status.success(),
        "changes annotate failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let yaml = std::fs::read_to_string(dir.path().join("changes/m2.yaml")).unwrap();
    assert!(
        yaml.contains("change_type: spec_change"),
        "expected change_type in: {yaml}"
    );
}

#[test]
fn changes_annotate_exits_three_when_event_id_does_not_exist() {
    let dir = init_project_with_two_milestones();

    let output = run(&[
        "changes",
        "annotate",
        "no-such-event",
        "--type",
        "bug-fix",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);

    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no-such-event"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn changes_annotate_related_sets_related_events_on_the_matching_event() {
    let dir = init_project_with_two_milestones();

    write_feature(dir.path(), "v3");
    run_git(dir.path(), &["add", "-A"]);
    run_git(dir.path(), &["commit", "-q", "-m", "v3"]);
    run_git(dir.path(), &["tag", "m3"]);
    let output = run(&[
        "changes",
        "compute",
        "m2",
        "m3",
        "--dir",
        dir.path().to_str().unwrap(),
        "--no-cache",
    ]);
    assert!(output.status.success());

    let output = run(&[
        "changes",
        "annotate",
        "player-jump--m2--m3",
        "--type",
        "spec-change",
        "--related",
        "player-jump--m1--m2",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);

    assert!(
        output.status.success(),
        "changes annotate --related failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let yaml = std::fs::read_to_string(dir.path().join("changes/m3.yaml")).unwrap();
    assert!(
        yaml.contains("related_events:\n  - player-jump--m1--m2"),
        "expected related_events in: {yaml}"
    );
}

#[test]
fn changes_annotate_related_sets_related_events_without_requiring_type() {
    let dir = init_project_with_two_milestones();

    write_feature(dir.path(), "v3");
    run_git(dir.path(), &["add", "-A"]);
    run_git(dir.path(), &["commit", "-q", "-m", "v3"]);
    run_git(dir.path(), &["tag", "m3"]);
    let output = run(&[
        "changes",
        "compute",
        "m2",
        "m3",
        "--dir",
        dir.path().to_str().unwrap(),
        "--no-cache",
    ]);
    assert!(output.status.success());

    let output = run(&[
        "changes",
        "annotate",
        "player-jump--m2--m3",
        "--related",
        "player-jump--m1--m2",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);

    assert!(
        output.status.success(),
        "changes annotate --related (no --type) failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let yaml = std::fs::read_to_string(dir.path().join("changes/m3.yaml")).unwrap();
    assert!(
        yaml.contains("related_events:\n  - player-jump--m1--m2"),
        "expected related_events in: {yaml}"
    );
    assert!(
        !yaml.contains("change_type: spec_change"),
        "change_type should not have been touched: {yaml}"
    );
}

#[test]
fn changes_annotate_requires_type_or_related() {
    let dir = init_project_with_two_milestones();

    let output = run(&[
        "changes",
        "annotate",
        "player-jump--m1--m2",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);

    assert!(!output.status.success());
}

#[test]
fn changes_annotate_related_exits_three_when_a_related_event_id_does_not_exist() {
    let dir = init_project_with_two_milestones();

    let output = run(&[
        "changes",
        "annotate",
        "player-jump--m1--m2",
        "--type",
        "spec-change",
        "--related",
        "no-such-event",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);

    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no-such-event"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn changes_annotate_does_not_write_change_type_when_related_event_id_does_not_exist() {
    let dir = init_project_with_two_milestones();

    let output = run(&[
        "changes",
        "annotate",
        "player-jump--m1--m2",
        "--type",
        "spec-change",
        "--related",
        "no-such-event",
        "--dir",
        dir.path().to_str().unwrap(),
    ]);

    assert_eq!(output.status.code(), Some(3));
    let yaml = std::fs::read_to_string(dir.path().join("changes/m2.yaml")).unwrap();
    assert!(
        !yaml.contains("change_type: spec_change"),
        "change_type should not have been written on failure: {yaml}"
    );
}

#[test]
fn changes_compute_records_both_parent_tree_shas_when_to_milestone_is_a_true_divergence_merge() {
    let dir = tempfile::tempdir().unwrap();
    let output = run(&["init", "--dir", dir.path().to_str().unwrap()]);
    assert!(output.status.success());
    run_git(dir.path(), &["init", "-q", "-b", "main"]);
    run_git(dir.path(), &["config", "user.email", "test@example.com"]);
    run_git(dir.path(), &["config", "user.name", "Test"]);

    write_feature(dir.path(), "base");
    run_git(dir.path(), &["add", "-A"]);
    run_git(dir.path(), &["commit", "-q", "-m", "base"]);
    run_git(dir.path(), &["tag", "m1"]);
    run_git(dir.path(), &["branch", "feature"]);

    write_feature(dir.path(), "changed-on-main");
    run_git(dir.path(), &["add", "-A"]);
    run_git(dir.path(), &["commit", "-q", "-m", "on main"]);

    run_git(dir.path(), &["checkout", "-q", "feature"]);
    write_feature(dir.path(), "changed-on-feature");
    run_git(dir.path(), &["add", "-A"]);
    run_git(dir.path(), &["commit", "-q", "-m", "on feature"]);

    run_git(dir.path(), &["checkout", "-q", "main"]);
    run_git(
        dir.path(),
        &[
            "merge", "-q", "-m", "merge", "-X", "ours", "--no-ff", "feature",
        ],
    );
    run_git(dir.path(), &["tag", "m2"]);

    let output = run(&[
        "changes",
        "compute",
        "m1",
        "m2",
        "--dir",
        dir.path().to_str().unwrap(),
        "--no-cache",
    ]);
    assert!(
        output.status.success(),
        "changes compute failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let yaml = std::fs::read_to_string(dir.path().join("changes/m2.yaml")).unwrap();
    assert!(
        yaml.contains("true_divergences:"),
        "expected true_divergences in: {yaml}"
    );
    assert!(
        yaml.contains("merge_commit:"),
        "expected merge_commit in: {yaml}"
    );
}

#[test]
fn changes_compute_records_both_parent_tree_shas_when_merge_occurs_within_the_milestone_interval() {
    let dir = tempfile::tempdir().unwrap();
    let output = run(&["init", "--dir", dir.path().to_str().unwrap()]);
    assert!(output.status.success());
    run_git(dir.path(), &["init", "-q", "-b", "main"]);
    run_git(dir.path(), &["config", "user.email", "test@example.com"]);
    run_git(dir.path(), &["config", "user.name", "Test"]);

    write_feature(dir.path(), "base");
    run_git(dir.path(), &["add", "-A"]);
    run_git(dir.path(), &["commit", "-q", "-m", "base"]);
    run_git(dir.path(), &["tag", "m1"]);
    run_git(dir.path(), &["branch", "feature"]);

    write_feature(dir.path(), "changed-on-main");
    run_git(dir.path(), &["add", "-A"]);
    run_git(dir.path(), &["commit", "-q", "-m", "on main"]);

    run_git(dir.path(), &["checkout", "-q", "feature"]);
    write_feature(dir.path(), "changed-on-feature");
    run_git(dir.path(), &["add", "-A"]);
    run_git(dir.path(), &["commit", "-q", "-m", "on feature"]);

    run_git(dir.path(), &["checkout", "-q", "main"]);
    run_git(
        dir.path(),
        &[
            "merge", "-q", "-m", "merge", "-X", "ours", "--no-ff", "feature",
        ],
    );

    std::fs::write(dir.path().join("post-merge.txt"), "done\n").unwrap();
    run_git(dir.path(), &["add", "-A"]);
    run_git(dir.path(), &["commit", "-q", "-m", "after merge"]);
    run_git(dir.path(), &["tag", "m2"]);

    let output = run(&[
        "changes",
        "compute",
        "m1",
        "m2",
        "--dir",
        dir.path().to_str().unwrap(),
        "--no-cache",
    ]);
    assert!(
        output.status.success(),
        "changes compute failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let yaml = std::fs::read_to_string(dir.path().join("changes/m2.yaml")).unwrap();
    assert!(
        yaml.contains("true_divergences:"),
        "expected true_divergences in: {yaml}"
    );
    assert!(
        !yaml.contains("true_divergences: []"),
        "expected populated true_divergences in: {yaml}"
    );
}

#[test]
fn changes_compute_records_a_true_divergence_entry_for_each_merge_when_the_interval_contains_two_merges()
 {
    let dir = tempfile::tempdir().unwrap();
    let output = run(&["init", "--dir", dir.path().to_str().unwrap()]);
    assert!(output.status.success());
    run_git(dir.path(), &["init", "-q", "-b", "main"]);
    run_git(dir.path(), &["config", "user.email", "test@example.com"]);
    run_git(dir.path(), &["config", "user.name", "Test"]);

    write_feature(dir.path(), "base");
    run_git(dir.path(), &["add", "-A"]);
    run_git(dir.path(), &["commit", "-q", "-m", "base"]);
    run_git(dir.path(), &["tag", "m1"]);

    // First divergence: branch off, diverge on both sides, merge back.
    run_git(dir.path(), &["branch", "feature1"]);
    write_feature(dir.path(), "main-1");
    run_git(dir.path(), &["add", "-A"]);
    run_git(dir.path(), &["commit", "-q", "-m", "main 1"]);
    run_git(dir.path(), &["checkout", "-q", "feature1"]);
    write_feature(dir.path(), "feature-1");
    run_git(dir.path(), &["add", "-A"]);
    run_git(dir.path(), &["commit", "-q", "-m", "feature 1"]);
    run_git(dir.path(), &["checkout", "-q", "main"]);
    run_git(
        dir.path(),
        &[
            "merge", "-q", "-m", "merge 1", "-X", "ours", "--no-ff", "feature1",
        ],
    );

    // Second divergence: branch off the post-merge state, diverge again, merge back.
    run_git(dir.path(), &["branch", "feature2"]);
    write_feature(dir.path(), "main-2");
    run_git(dir.path(), &["add", "-A"]);
    run_git(dir.path(), &["commit", "-q", "-m", "main 2"]);
    run_git(dir.path(), &["checkout", "-q", "feature2"]);
    write_feature(dir.path(), "feature-2");
    run_git(dir.path(), &["add", "-A"]);
    run_git(dir.path(), &["commit", "-q", "-m", "feature 2"]);
    run_git(dir.path(), &["checkout", "-q", "main"]);
    run_git(
        dir.path(),
        &[
            "merge", "-q", "-m", "merge 2", "-X", "ours", "--no-ff", "feature2",
        ],
    );
    run_git(dir.path(), &["tag", "m2"]);

    let output = run(&[
        "changes",
        "compute",
        "m1",
        "m2",
        "--dir",
        dir.path().to_str().unwrap(),
        "--no-cache",
    ]);
    assert!(
        output.status.success(),
        "changes compute failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let yaml = std::fs::read_to_string(dir.path().join("changes/m2.yaml")).unwrap();
    let merge_commit_count = yaml.matches("merge_commit:").count();
    assert_eq!(
        merge_commit_count, 2,
        "expected two true_divergences entries (one per merge) in: {yaml}"
    );
}
