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
        .join("knowledge")
        .join(requirement)
        .join(feature)
        .join(behavior)
        .join(condition);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        root.join("knowledge")
            .join(requirement)
            .join("requirement.yml"),
        format!("id: {requirement}\nlabel: {requirement}\naxis: [ui]\n"),
    )
    .unwrap();
    std::fs::write(
        root.join("knowledge")
            .join(requirement)
            .join(feature)
            .join("feature.yml"),
        format!("id: {feature}\nrequirement: {requirement}\nlabel: {feature}\naxis: [ui]\n"),
    )
    .unwrap();
    std::fs::write(
        dir.parent().unwrap().join("behavior.yml"),
        format!(
            "id: {behavior}\nfeature: {feature}\nlabel: {behavior}\naxis: [ui]\ndescription: |\n  Behavior.\n"
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("condition.yml"),
        format!(
            "id: {condition}\nbehavior: {behavior}\nlabel: {condition}\ndescription: |\n  Condition.\n"
        ),
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("expected")).unwrap();
    std::fs::write(
        dir.join("expected/001.yml"),
        format!("id: {condition}-001\ncondition: {condition}\ndescription: |\n  Expected.\n"),
    )
    .unwrap();
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
        .join("generated/testcases/req-one/feature-a/behavior-a/shared-id.yml");
    let second = root
        .path()
        .join("generated/testcases/req-two/feature-b/behavior-b/shared-id.yml");
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

    let generated_dir = root.path().join("generated");
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
