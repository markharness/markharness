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
