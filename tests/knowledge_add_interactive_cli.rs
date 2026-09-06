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

// ADR 0017 §1・§3: a brand-new Requirement always has no `uid` yet, and a
// Feature can never be created referencing one in the same `knowledge add`
// invocation (`identity migrate` must run first — see `interactive::run_add`,
// which now stops right after writing a new Requirement rather than going on
// to prompt for Feature/Behavior/Scenario). So this now takes two invocations:
// create the Requirement, migrate, then create the rest reusing it.
const REQUIREMENT_ONLY_INPUT: &str = "controls\ngameplay\n";
const FULL_INPUT_REUSING_MIGRATED_REQUIREMENT: &str = "controls\nplayer-jump\ngameplay, animation\njump\ngameplay\nPlayer presses jump.\n\nground\nJump from the ground and land\nDo it.\n\nlands safely\n\n\n";

#[test]
fn knowledge_add_writes_full_chain_from_stdin_prompts() {
    let dir = tempfile::tempdir().unwrap();
    let init_output = run(&["init", "--dir", dir.path().to_str().unwrap()]);
    assert!(init_output.status.success());

    let requirement_output = run_with_stdin(
        &["knowledge", "add", "--dir", dir.path().to_str().unwrap()],
        REQUIREMENT_ONLY_INPUT,
    );
    assert!(
        requirement_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&requirement_output.stderr)
    );

    let migrate_output = run(&["identity", "migrate", "--dir", dir.path().to_str().unwrap()]);
    assert!(
        migrate_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&migrate_output.stderr)
    );

    let output = run_with_stdin(
        &["knowledge", "add", "--dir", dir.path().to_str().unwrap()],
        FULL_INPUT_REUSING_MIGRATED_REQUIREMENT,
    );

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let scenario_path = dir
        .path()
        .join(".markharness/knowledge/features/player-jump/jump/ground/scenario.yml");
    assert_eq!(
        std::fs::read_to_string(scenario_path).unwrap(),
        "id: ground\nbehavior: jump\nlabel: ground\ndescription: |\n  Jump from the ground and land\nphases:\n  - steps:\n      - action: \"Do it.\"\n    results:\n      - \"lands safely\"\n"
    );
}
