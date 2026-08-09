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

fn write_valid_tree(root: &Path) {
    let base = root.join("knowledge/controls/player-jump/jump/ground");
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(
        root.join("axes/gameplay.yml"),
        "id: gameplay\nlabel: Gameplay\n",
    )
    .unwrap();
    std::fs::write(
        root.join("knowledge/controls/requirement.yml"),
        "id: controls\nlabel: controls\naxis: [gameplay]\n",
    )
    .unwrap();
    std::fs::write(
        root.join("knowledge/controls/player-jump/feature.yml"),
        "id: player-jump\nrequirement: controls\nlabel: player-jump\naxis: [gameplay]\n",
    )
    .unwrap();
    std::fs::write(
        root.join("knowledge/controls/player-jump/jump/behavior.yml"),
        "id: jump\nfeature: player-jump\nlabel: jump\naxis: [gameplay]\ndescription: |\n  Player presses jump.\n",
    )
    .unwrap();
    std::fs::write(
        base.join("condition.yml"),
        "id: ground\nbehavior: jump\nlabel: ground\ndescription: |\n  Jump from the ground.\n",
    )
    .unwrap();
    std::fs::create_dir_all(base.join("expected")).unwrap();
    std::fs::write(
        base.join("expected/001.yml"),
        "id: ground-001\ncondition: ground\ndescription: |\n  lands safely\n",
    )
    .unwrap();
}

#[test]
fn validate_exits_zero_and_reports_ok_for_a_valid_tree() {
    let dir = tempfile::tempdir().unwrap();
    let init_output = run(&["init", "--dir", dir.path().to_str().unwrap()]);
    assert!(init_output.status.success());
    write_valid_tree(dir.path());

    let output = run(&["validate", "--dir", dir.path().to_str().unwrap()]);

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn validate_exits_one_and_lists_issues_for_an_invalid_feature() {
    let dir = tempfile::tempdir().unwrap();
    let init_output = run(&["init", "--dir", dir.path().to_str().unwrap()]);
    assert!(init_output.status.success());
    write_valid_tree(dir.path());
    std::fs::write(
        dir.path()
            .join("knowledge/controls/player-jump/feature.yml"),
        "id: player-jump\nrequirement: controls\nlabel: player-jump\naxis: [not-registered]\n",
    )
    .unwrap();

    let output = run(&["validate", "--dir", dir.path().to_str().unwrap()]);

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("not-registered"),
        "unexpected stdout: {stdout}"
    );
}
