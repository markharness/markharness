//! `markharness gui` — launches the separate GUI executable (ADR 0038).
//!
//! The launched-GUI path (finding it, passing `--dir`/`MARKHARNESS_BIN`,
//! returning its exit code) is covered by `src/gui.rs`'s unit tests; these
//! tests cover what the CLI binary does before and without a GUI.
#![allow(clippy::disallowed_methods)]

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

#[test]
fn gui_in_a_directory_without_a_project_tells_the_user_to_initialize() {
    let dir = tempfile::tempdir().unwrap();

    let output = run(&["gui", "--dir", dir.path().to_str().unwrap()]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("markharness init"), "{stderr}");
}

#[test]
fn gui_without_a_bundled_executable_reports_what_is_missing() {
    let project = tempfile::tempdir().unwrap();
    assert!(
        run(&["init", "--dir", project.path().to_str().unwrap()])
            .status
            .success()
    );
    let empty_path = tempfile::tempdir().unwrap();

    let output = Command::new(bin())
        .args(["gui", "--dir", project.path().to_str().unwrap()])
        .env("PATH", empty_path.path())
        .output()
        .expect("failed to run markharness binary");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("markharness-gui"), "{stderr}");
    assert!(stderr.contains("GUI-bundled"), "{stderr}");
}
