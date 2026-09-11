//! ADR 0023: a Requirement's content is owned either by markharness
//! (`source: native`) or by an external `.sdoc` (`source: external`).
//!
//! Covers the v2 design's acceptance criteria AC02, AC02b, AC09, AC09b, and
//! AC09c. Fixtures write directly to a scratch repo before invoking the CLI
//! binary; that's outside fs_safety's managed-root scope (see clippy.toml).
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

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let output = run(&["init", "--dir", dir.path().to_str().unwrap()]);
    assert!(output.status.success(), "init failed: {output:?}");
    std::fs::write(
        dir.path().join(".markharness/axes/gameplay.yml"),
        "id: gameplay\nlabel: Gameplay\n",
    )
    .unwrap();
    dir
}

fn write_requirement(root: &Path, id: &str, body: &str) {
    let dir = root.join(".markharness/knowledge/requirements").join(id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("requirement.yml"), body).unwrap();
}

fn validate(root: &Path) -> Output {
    run(&["validate", "--dir", root.to_str().unwrap()])
}

/// AC09b (revised by ADR 0026 §7): mode is never decided by an implicit
/// default, so omitting `source` is refused rather than assumed native.
#[test]
fn a_requirement_without_source_is_rejected() {
    let dir = project();
    write_requirement(
        dir.path(),
        "controls",
        "id: controls\nlabel: controls\naxis: [gameplay]\n",
    );

    let output = validate(dir.path());
    assert!(!output.status.success(), "{output:?}");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(combined.contains("source"), "{combined}");
}

/// AC02b: native keeps its content in markharness.
#[test]
fn a_native_requirement_with_a_label_is_valid() {
    let dir = project();
    write_requirement(
        dir.path(),
        "controls",
        "id: controls\nsource: native\nlabel: controls\naxis: [gameplay]\ndescription: |\n  free text\n",
    );

    let output = validate(dir.path());
    assert!(output.status.success(), "{output:?}");
}

/// AC09: external must carry both halves of the fixed reference.
#[test]
fn an_external_requirement_without_a_locator_is_rejected() {
    let dir = project();
    write_requirement(
        dir.path(),
        "controls",
        "id: controls\nsource: external\naxis: [gameplay]\n",
    );

    let output = validate(dir.path());
    assert!(!output.status.success(), "{output:?}");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(combined.contains("source_locator"), "{combined}");
}

#[test]
fn an_external_requirement_with_a_locator_and_revision_is_valid() {
    let dir = project();
    write_requirement(
        dir.path(),
        "controls",
        "id: controls\nsource: external\nsource_locator: docs/requirements.sdoc\nsource_revision: 0123456789abcdef0123456789abcdef01234567\naxis: [gameplay]\n",
    );

    let output = validate(dir.path());
    assert!(output.status.success(), "{output:?}");
}

/// AC02: external never duplicates the external owner's content.
#[test]
fn an_external_requirement_carrying_a_label_is_rejected() {
    let dir = project();
    write_requirement(
        dir.path(),
        "controls",
        "id: controls\nsource: external\nsource_locator: docs/requirements.sdoc\nsource_revision: 0123456789abcdef0123456789abcdef01234567\nlabel: controls\naxis: [gameplay]\n",
    );

    let output = validate(dir.path());
    assert!(!output.status.success(), "{output:?}");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(combined.contains("label"), "{combined}");
}

/// AC09c: a file that is neither cleanly native nor cleanly external is
/// refused, so no rule is ever needed to decide which half wins.
#[test]
fn a_native_requirement_carrying_a_source_locator_is_rejected() {
    let dir = project();
    write_requirement(
        dir.path(),
        "controls",
        "id: controls\nsource: native\nlabel: controls\nsource_locator: docs/requirements.sdoc\naxis: [gameplay]\n",
    );

    let output = validate(dir.path());
    assert!(!output.status.success(), "{output:?}");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(combined.contains("source_locator"), "{combined}");
}

#[test]
fn a_native_requirement_without_a_label_is_rejected() {
    let dir = project();
    write_requirement(
        dir.path(),
        "controls",
        "id: controls\nsource: native\naxis: [gameplay]\n",
    );

    let output = validate(dir.path());
    assert!(!output.status.success(), "{output:?}");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(combined.contains("label"), "{combined}");
}

/// `axis` is markharness's own classification, not a copy of the external
/// owner's content, so it is kept in both modes (ADR 0023 §5).
#[test]
fn an_unregistered_axis_is_still_caught_on_an_external_requirement() {
    let dir = project();
    write_requirement(
        dir.path(),
        "controls",
        "id: controls\nsource: external\nsource_locator: docs/requirements.sdoc\nsource_revision: 0123456789abcdef0123456789abcdef01234567\naxis: [not-registered]\n",
    );

    let output = validate(dir.path());
    assert!(!output.status.success(), "{output:?}");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(combined.contains("not-registered"), "{combined}");
}
