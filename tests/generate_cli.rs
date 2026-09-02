// Integration test fixtures write directly to a scratch repo before
// invoking the CLI binary; that's outside fs_safety's managed-root scope
// (see clippy.toml / src/lib.rs).
#![allow(clippy::disallowed_methods)]

use std::path::Path;
use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_markharness")
}

fn run_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .current_dir(dir)
        .output()
        .expect("failed to run markharness binary")
}

fn run(args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .output()
        .expect("failed to run markharness binary")
}

#[cfg(unix)]
fn link_dir(link: &Path, target: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[cfg(windows)]
fn link_dir(link: &Path, target: &Path) {
    let status = Command::new("cmd")
        .args(["/c", "mklink", "/j"])
        .arg(link)
        .arg(target)
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "mklink /j failed");
}

fn write_chain(root: &Path, requirement: &str, feature: &str, behavior: &str, condition: &str) {
    let dir = root
        .join(markharness::project_root::MARKHARNESS_DIR)
        .join("knowledge")
        .join(requirement)
        .join(feature)
        .join(behavior)
        .join(condition);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        root.join(markharness::project_root::MARKHARNESS_DIR)
            .join("knowledge")
            .join(requirement)
            .join("requirement.yml"),
        format!("id: {requirement}\nlabel: {requirement}\naxis: [ui]\n"),
    )
    .unwrap();
    std::fs::write(
        root.join(markharness::project_root::MARKHARNESS_DIR)
            .join("knowledge")
            .join(requirement)
            .join(feature)
            .join("feature.yml"),
        format!("id: {feature}\nrequirement: {requirement}\nlabel: {feature}\naxis: [ui]\n"),
    )
    .unwrap();
    std::fs::write(
        dir.parent().unwrap().join("behavior.yml"),
        format!(
            "id: {behavior}\nfeature: {feature}\nlabel: {behavior}\naxis: [ui]\ndescription: |\n  Behavior.\npreconditions:\n  - \"Do it.\"\n"
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("condition.yml"),
        format!(
            "id: {condition}\nbehavior: {behavior}\nlabel: {condition}\ndescription: |\n  Condition.\nsteps:\n  - \"Do it.\"\nadditional_preconditions: []\n"
        ),
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("expected")).unwrap();
    std::fs::write(
        dir.join("expected/001.yml"),
        format!("id: {condition}-001\ncondition: {condition}\ndescription: |\n  Expected.\nresults:\n  - \"Confirmed.\"\n"),
    )
    .unwrap();
}

/// Step B: unlike every other subcommand, `generate` used to only accept
/// `env::current_dir()` and had no `--dir`, forcing callers to `cd` into the
/// target project first.
#[test]
fn generate_accepts_a_dir_option_targeting_a_directory_other_than_cwd() {
    let root = tempfile::tempdir().unwrap();
    let init_output = run_in(root.path(), &["init"]);
    assert!(init_output.status.success());
    write_chain(root.path(), "req-one", "feature-a", "behavior-a", "ground");

    // Run from an unrelated cwd, targeting `root` only via --dir.
    let unrelated_cwd = tempfile::tempdir().unwrap();
    let output = run_in(
        unrelated_cwd.path(),
        &["generate", "--dir", root.path().to_str().unwrap()],
    );

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        root.path()
            .join(".markharness/generated/testcases/req-one/feature-a/behavior-a/ground.yml")
            .is_file()
    );
}

/// Step B: `--json` should report the generated count and every written
/// path, so callers don't have to reconcile a human-readable count string
/// against the actual file count (the original reported bug was exactly
/// this kind of silent mismatch).
#[test]
fn generate_json_reports_generated_count_and_written_paths() {
    let root = tempfile::tempdir().unwrap();
    let init_output = run_in(root.path(), &["init"]);
    assert!(init_output.status.success());
    write_chain(root.path(), "req-one", "feature-a", "behavior-a", "ground");

    let output = run(&["generate", "--dir", root.path().to_str().unwrap(), "--json"]);

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["ok"], serde_json::json!(true));
    assert_eq!(parsed["generated"], serde_json::json!(1));
    let written = parsed["written"].as_array().unwrap();
    assert!(
        written.iter().any(|p| p
            .as_str()
            .unwrap()
            .ends_with("generated/testcases/req-one/feature-a/behavior-a/ground.yml")),
        "unexpected written list: {written:?}"
    );
    assert!(
        written
            .iter()
            .any(|p| p.as_str().unwrap().ends_with("traceability-index.json")),
        "unexpected written list: {written:?}"
    );
}

/// Regression test for the reported bug: two Conditions with the same
/// `condition.id` under different Behaviors used to be written to the same
/// flat `generated/testcases/<condition.id>.yml` path, silently overwriting
/// one another. Since Step A, output mirrors `knowledge/`'s own hierarchy,
/// so both must now survive as distinct files.
#[test]
fn generate_does_not_lose_testcases_when_the_same_condition_id_is_reused_under_different_behaviors()
{
    let root = tempfile::tempdir().unwrap();
    let init_output = run_in(root.path(), &["init"]);
    assert!(init_output.status.success());

    write_chain(
        root.path(),
        "req-one",
        "feature-a",
        "behavior-a",
        "shared-id",
    );
    write_chain(
        root.path(),
        "req-two",
        "feature-b",
        "behavior-b",
        "shared-id",
    );

    let output = run_in(root.path(), &["generate"]);
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let first = root
        .path()
        .join(".markharness/generated/testcases/req-one/feature-a/behavior-a/shared-id.yml");
    let second = root
        .path()
        .join(".markharness/generated/testcases/req-two/feature-b/behavior-b/shared-id.yml");
    assert!(first.is_file(), "expected {} to exist", first.display());
    assert!(second.is_file(), "expected {} to exist", second.display());
}

#[test]
fn generate_refuses_to_follow_a_symlinked_generated_dir() {
    let root = tempfile::tempdir().unwrap();
    let init_output = run_in(root.path(), &["init"]);
    assert!(init_output.status.success());

    let outside = tempfile::tempdir().unwrap();
    let victim_dir = outside.path().join("testcases");
    std::fs::create_dir_all(&victim_dir).unwrap();
    std::fs::write(victim_dir.join("do-not-delete.txt"), "victim").unwrap();

    let generated_dir = root
        .path()
        .join(markharness::project_root::MARKHARNESS_DIR)
        .join("generated");
    if generated_dir.is_dir() {
        std::fs::remove_dir_all(&generated_dir).unwrap();
    }
    link_dir(&generated_dir, outside.path());

    let output = run_in(root.path(), &["generate"]);

    assert!(
        !output.status.success(),
        "expected generate to refuse a symlinked generated/ dir, stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        victim_dir.join("do-not-delete.txt").is_file(),
        "generate must not delete through the symlinked ancestor"
    );
}
