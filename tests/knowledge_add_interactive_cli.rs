// Integration test fixtures write directly to a scratch repo before
// invoking the CLI binary; that's outside fs_safety's managed-root scope
// (see clippy.toml / src/lib.rs).
#![allow(clippy::disallowed_methods)]

use std::io::Write;
use std::process::{Command, Output, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_markharness")
}

fn run(args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .output()
        .expect("failed to run markharness binary")
}

fn run_with_stdin(args: &[&str], input: &str) -> Output {
    let mut child = Command::new(bin())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn markharness binary");
    child
        .stdin
        .take()
        .expect("stdin was not piped")
        .write_all(input.as_bytes())
        .expect("failed to write to stdin");
    child.wait_with_output().expect("failed to wait on child")
}

const FULL_INPUT: &str = "controls\ngameplay\nplayer-jump\ngameplay, animation\njump\ngameplay\nPlayer presses jump.\nground\nJump from the ground and land\nlands safely\n";

#[test]
fn knowledge_add_writes_full_chain_from_stdin_prompts() {
    let dir = tempfile::tempdir().unwrap();
    let init_output = run(&["init", "--dir", dir.path().to_str().unwrap()]);
    assert!(init_output.status.success());

    let output = run_with_stdin(
        &["knowledge", "add", "--dir", dir.path().to_str().unwrap()],
        FULL_INPUT,
    );

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let expected_path = dir
        .path()
        .join(".markharness/knowledge/controls/player-jump/jump/ground/expected/001.yml");
    assert_eq!(
        std::fs::read_to_string(expected_path).unwrap(),
        "id: ground-001\ncondition: ground\ndescription: |\n  lands safely\n"
    );
}
