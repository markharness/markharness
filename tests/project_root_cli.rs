// Integration test fixtures write directly to a scratch repo before
// invoking the CLI binary; that's outside fs_safety's managed-root scope
// (see clippy.toml / src/lib.rs).
#![allow(clippy::disallowed_methods)]

use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_markharness")
}

fn run_in(cwd: &std::path::Path, args: &[&str]) -> Output {
    Command::new(bin())
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("failed to run markharness binary")
}

#[test]
fn resolves_the_project_root_from_a_nested_subdirectory_without_dir_flag() {
    let dir = tempfile::tempdir().unwrap();
    let init_output = run_in(dir.path(), &["init", "--dir", "."]);
    assert!(init_output.status.success());

    let nested = dir.path().join("knowledge/some/nested/place");
    std::fs::create_dir_all(&nested).unwrap();

    let output = run_in(&nested, &["axes", "list", "--json"]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn errors_with_guidance_when_no_project_is_found_anywhere_upward() {
    let dir = tempfile::tempdir().unwrap();

    let output = run_in(dir.path(), &["axes", "list", "--json"]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("markharness init"),
        "expected guidance to run `markharness init`, got: {stderr}"
    );
}
