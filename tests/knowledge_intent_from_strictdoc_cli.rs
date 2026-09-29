// Integration test fixtures write directly to a scratch repo before
// invoking the CLI binary; that's outside fs_safety's managed-root scope
// (see clippy.toml / src/lib.rs).
#![allow(clippy::disallowed_methods)]

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

const DOC_MID: &str = "d0c0d0c0d0c0d0c0d0c0d0c0d0c0d0c0";
const REQ_MID_1: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa1";
const REQ_MID_2: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa2";

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_markharness")
}

fn run(args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .output()
        .expect("failed to run markharness binary")
}

fn run_with_stdin(args: &[&str], stdin: &str) -> Output {
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
        .expect("child stdin")
        .write_all(stdin.as_bytes())
        .expect("failed to write to child stdin");
    child
        .wait_with_output()
        .expect("failed to wait on markharness binary")
}

fn git(root: &Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .unwrap()
            .success()
    );
}

/// An initialized markharness project that is also a Git repository with
/// `docs/a.sdoc` committed, the way a real StrictDoc project would be.
fn project_with_sdoc() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        run(&["init", "--dir", dir.path().to_str().unwrap()])
            .status
            .success()
    );
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["config", "user.email", "test@example.com"]);
    git(dir.path(), &["config", "user.name", "Test"]);
    fs::create_dir_all(dir.path().join("docs")).unwrap();
    fs::write(
        dir.path().join("docs/a.sdoc"),
        sdoc_with(&[REQ_MID_1, REQ_MID_2]),
    )
    .unwrap();
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-q", "-m", "init"]);
    dir
}

/// The `.sdoc` text of a document holding requirements with these MIDs.
fn sdoc_with(mids: &[&str]) -> String {
    let mut text = String::from("[DOCUMENT]\nTITLE: Doc\n");
    for mid in mids {
        text.push_str(&format!("\n[REQUIREMENT]\nMID: {mid}\nSTATEMENT: s\n"));
    }
    text
}

fn write_export(dir: &Path, json: &str) -> String {
    let path = dir.join("strictdoc-export.json");
    fs::write(&path, json).unwrap();
    path.to_str().unwrap().to_string()
}

fn requirement(mid: &str, uid: &str) -> String {
    format!(r#"{{"_NODE_TYPE":"REQUIREMENT","MID":"{mid}","UID":"{uid}","STATEMENT":"s"}}"#)
}

#[test]
fn requirements_nested_in_sections_become_external_requirement_intents_reconcile_accepts() {
    let dir = project_with_sdoc();
    let export = write_export(
        dir.path(),
        &format!(
            r#"{{"DOCUMENTS":[{{"_NODE_TYPE":"DOCUMENT","MID":"{DOC_MID}","TITLE":"Doc A","NODES":[
                {{"_NODE_TYPE":"SECTION","TITLE":"S","NODES":[
                    {},
                    {{"_NODE_TYPE":"SECTION","TITLE":"Inner","NODES":[{}]}}
                ]}}
            ]}}]}}"#,
            requirement(REQ_MID_1, "REQ-1"),
            requirement(REQ_MID_2, "REQ-2"),
        ),
    );

    let output = run(&[
        "knowledge",
        "intent-from-strictdoc",
        "--input",
        &export,
        "--dir",
        dir.path().to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");

    let intent = String::from_utf8(output.stdout).unwrap();
    let expected = format!(
        "format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - id: sd-{REQ_MID_1}
    source: external
    axis: []
    source_locator: docs/a.sdoc
    source_revision: current
    source_key: {REQ_MID_1}
  - id: sd-{REQ_MID_2}
    source: external
    axis: []
    source_locator: docs/a.sdoc
    source_revision: current
    source_key: {REQ_MID_2}
"
    );
    assert_eq!(intent, expected);

    // The Intent is exactly what `knowledge reconcile` consumes from stdin:
    // `--check` exits 4 when it would create the two Requirements.
    let reconcile = run_with_stdin(
        &[
            "knowledge",
            "reconcile",
            "-",
            "--check",
            "--dir",
            dir.path().to_str().unwrap(),
        ],
        &intent,
    );
    assert_eq!(reconcile.status.code(), Some(4), "{reconcile:?}");
}

fn document(mid: Option<&str>, title: &str, nodes: &[String]) -> String {
    let mid = mid.map_or_else(String::new, |mid| format!(r#""MID":"{mid}","#));
    format!(
        r#"{{"_NODE_TYPE":"DOCUMENT",{mid}"TITLE":"{title}","NODES":[{}]}}"#,
        nodes.join(",")
    )
}

fn export_of(documents: &[String]) -> String {
    format!(r#"{{"DOCUMENTS":[{}]}}"#, documents.join(","))
}

fn intent_from(dir: &Path, export: &str, extra: &[&str]) -> Output {
    let export = write_export(dir, export);
    let mut args = vec![
        "knowledge",
        "intent-from-strictdoc",
        "--input",
        &export,
        "--dir",
        dir.to_str().unwrap(),
    ];
    args.extend_from_slice(extra);
    run(&args)
}

/// A rejected run must emit no Intent at all (ADR 0036 §5), so a caller
/// piping into `knowledge reconcile -` never applies a partial one.
fn assert_rejected(output: &Output, stderr_contains: &str) {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(stderr_contains), "{stderr}");
}

#[test]
fn only_requirement_nodes_are_imported() {
    let dir = project_with_sdoc();
    let text = format!(r#"{{"_NODE_TYPE":"TEXT","MID":"{REQ_MID_2}","STATEMENT":"prose"}}"#);
    let custom = format!(r#"{{"_NODE_TYPE":"DESIGN_NOTE","MID":"{REQ_MID_2}"}}"#);
    let export = export_of(&[
        document(
            Some(DOC_MID),
            "Doc A",
            &[requirement(REQ_MID_1, "REQ-1"), text, custom],
        ),
        // A document without requirements needs neither a MID nor a `.sdoc`.
        document(None, "Empty", &[]),
    ]);

    let output = intent_from(dir.path(), &export, &[]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let intent = String::from_utf8(output.stdout).unwrap();
    assert!(
        intent.contains(&format!("source_key: {REQ_MID_1}\n")),
        "{intent}"
    );
    assert!(!intent.contains(REQ_MID_2), "{intent}");
}

#[test]
fn a_requirement_without_a_mid_rejects_the_whole_run_naming_it() {
    let dir = project_with_sdoc();
    let no_mid = r#"{"_NODE_TYPE":"REQUIREMENT","UID":"REQ-9","STATEMENT":"s"}"#.to_string();
    let export = export_of(&[document(
        Some(DOC_MID),
        "Doc A",
        &[requirement(REQ_MID_1, "REQ-1"), no_mid],
    )]);

    assert_rejected(&intent_from(dir.path(), &export, &[]), "REQ-9");
}

#[test]
fn a_document_without_a_mid_is_fine_because_requirement_mids_locate_the_file() {
    // Markdown documents have no document-level MID at all.
    let dir = project_with_sdoc();
    let export = export_of(&[document(
        None,
        "Unnumbered Doc",
        &[requirement(REQ_MID_1, "REQ-1")],
    )]);

    let output = intent_from(dir.path(), &export, &[]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let intent = String::from_utf8(output.stdout).unwrap();
    assert!(intent.contains("source_locator: docs/a.sdoc\n"), "{intent}");
}

#[test]
fn a_requirement_mid_found_in_no_source_file_rejects_the_run() {
    let dir = project_with_sdoc();
    let unknown = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let export = export_of(&[document(
        Some(DOC_MID),
        "Doc A",
        &[requirement(unknown, "REQ-1")],
    )]);

    assert_rejected(&intent_from(dir.path(), &export, &[]), unknown);
}

#[test]
fn an_indented_mid_line_in_a_code_example_is_not_a_requirement_mid() {
    // strictdoc's own user guide quotes a `[REQUIREMENT]` block, indented,
    // inside a text node.
    let dir = project_with_sdoc();
    fs::write(
        dir.path().join("docs/guide.sdoc"),
        format!(
            "[DOCUMENT]\nTITLE: Guide\n\n[TEXT]\nSTATEMENT: >>>\n.. code:: strictdoc\n\n    [REQUIREMENT]\n    MID: {REQ_MID_1}\n<<<\n"
        ),
    )
    .unwrap();
    let export = export_of(&[document(
        Some(DOC_MID),
        "Doc A",
        &[requirement(REQ_MID_1, "REQ-1")],
    )]);

    let output = intent_from(dir.path(), &export, &[]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let intent = String::from_utf8(output.stdout).unwrap();
    assert!(intent.contains("source_locator: docs/a.sdoc\n"), "{intent}");
}

#[test]
fn requirements_of_a_markdown_document_locate_their_md_file() {
    let dir = project_with_sdoc();
    let md_mid = "cccccccccccccccccccccccccccccccc";
    fs::create_dir_all(dir.path().join("spec")).unwrap();
    fs::write(
        dir.path().join("spec/spec.md"),
        format!(
            "# Spec\n\n### Naming\n\n**MID**: {md_mid} \\\n**UID**: MD-1\n\n**Statement**: s\n"
        ),
    )
    .unwrap();
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-q", "-m", "md"]);
    let export = export_of(&[document(None, "Spec", &[requirement(md_mid, "MD-1")])]);

    let output = intent_from(dir.path(), &export, &[]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let intent = String::from_utf8(output.stdout).unwrap();
    assert!(
        intent.contains("source_locator: spec/spec.md\n"),
        "{intent}"
    );
    let reconcile = run_with_stdin(
        &[
            "knowledge",
            "reconcile",
            "-",
            "--check",
            "--dir",
            dir.path().to_str().unwrap(),
        ],
        &intent,
    );
    assert_eq!(reconcile.status.code(), Some(4), "{reconcile:?}");
}

#[test]
fn a_requirement_mid_found_in_several_files_rejects_the_run_listing_them() {
    let dir = project_with_sdoc();
    fs::write(dir.path().join("docs/copy.sdoc"), sdoc_with(&[REQ_MID_1])).unwrap();
    let export = export_of(&[document(
        Some(DOC_MID),
        "Doc A",
        &[requirement(REQ_MID_1, "REQ-1")],
    )]);

    let output = intent_from(dir.path(), &export, &[]);

    assert_rejected(&output, "docs/a.sdoc");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("docs/copy.sdoc"), "{output:?}");
    // Two copies under one root cannot be told apart by `--sdoc-root`.
    assert!(stderr.contains("remove the duplicate"), "{output:?}");
}

#[test]
fn sdoc_root_limits_the_scan_and_locators_stay_relative_to_the_project() {
    let dir = project_with_sdoc();
    // Same MID outside the scanned root: must not count as a second match.
    fs::create_dir_all(dir.path().join("other")).unwrap();
    fs::write(dir.path().join("other/dup.sdoc"), sdoc_with(&[REQ_MID_1])).unwrap();
    let export = export_of(&[document(
        Some(DOC_MID),
        "Doc A",
        &[requirement(REQ_MID_1, "REQ-1")],
    )]);
    let sdoc_root = dir.path().join("docs");

    let output = intent_from(
        dir.path(),
        &export,
        &["--sdoc-root", sdoc_root.to_str().unwrap()],
    );

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let intent = String::from_utf8(output.stdout).unwrap();
    assert!(intent.contains("source_locator: docs/a.sdoc\n"), "{intent}");
}

#[test]
fn an_sdoc_root_outside_the_project_rejects_the_run() {
    let dir = project_with_sdoc();
    let outside = tempfile::tempdir().unwrap();
    let export = export_of(&[document(
        Some(DOC_MID),
        "Doc A",
        &[requirement(REQ_MID_1, "REQ-1")],
    )]);

    let output = intent_from(
        dir.path(),
        &export,
        &["--sdoc-root", outside.path().to_str().unwrap()],
    );

    assert_rejected(&output, "outside the project");
}

#[test]
fn a_mid_that_is_not_lowercase_hex_rejects_the_run() {
    // `id: sd-<MID>` must be a valid slug and the YAML is built by
    // formatting, so anything but lowercase hex is refused up front.
    let dir = project_with_sdoc();
    let export = export_of(&[document(
        Some(DOC_MID),
        "Doc A",
        &[requirement("ABC: def", "REQ-1")],
    )]);

    assert_rejected(&intent_from(dir.path(), &export, &[]), "REQ-1");
}

#[test]
fn a_mid_of_fewer_than_32_hex_digits_is_accepted() {
    // A real StrictDoc project carries a 31-digit MID
    // (SDOC-SRS-148 in strictdoc's own docs), so length is not validated.
    let dir = project_with_sdoc();
    let short = "9183080f1358485f9bacdcda0635109";
    fs::write(dir.path().join("docs/short.sdoc"), sdoc_with(&[short])).unwrap();
    let export = export_of(&[document(
        Some(DOC_MID),
        "Doc A",
        &[requirement(short, "SDOC-SRS-148")],
    )]);

    let output = intent_from(dir.path(), &export, &[]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let intent = String::from_utf8(output.stdout).unwrap();
    assert!(
        intent.contains(&format!("  - id: sd-{short}\n")),
        "{intent}"
    );
    assert!(
        intent.contains(&format!("    source_key: {short}\n")),
        "{intent}"
    );
}

#[test]
fn an_empty_mid_rejects_the_run() {
    let dir = project_with_sdoc();
    let export = export_of(&[document(
        Some(DOC_MID),
        "Doc A",
        &[requirement("", "REQ-1")],
    )]);

    assert_rejected(&intent_from(dir.path(), &export, &[]), "REQ-1");
}

#[test]
fn several_sdoc_roots_scan_only_those_directories() {
    // strictdoc's own repository copies its spec into `tests/` as fixtures;
    // listing the document directories (its `include_doc_paths`) keeps
    // those copies out of the scan.
    let dir = project_with_sdoc();
    let md_mid = "cccccccccccccccccccccccccccccccc";
    let md = format!("# Spec\n\n### Naming\n\n**MID**: {md_mid} \\\n**UID**: MD-1\n");
    for path in ["spec/spec.md", "tests/fixture/input.md"] {
        fs::create_dir_all(dir.path().join(path).parent().unwrap()).unwrap();
        fs::write(dir.path().join(path), &md).unwrap();
    }
    let export = export_of(&[
        document(Some(DOC_MID), "Doc A", &[requirement(REQ_MID_1, "REQ-1")]),
        document(None, "Spec", &[requirement(md_mid, "MD-1")]),
    ]);

    assert_rejected(
        &intent_from(dir.path(), &export, &[]),
        "tests/fixture/input.md",
    );

    let docs = dir.path().join("docs");
    let spec = dir.path().join("spec");
    let output = intent_from(
        dir.path(),
        &export,
        &[
            "--sdoc-root",
            docs.to_str().unwrap(),
            "--sdoc-root",
            spec.to_str().unwrap(),
        ],
    );

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let intent = String::from_utf8(output.stdout).unwrap();
    assert!(intent.contains("source_locator: docs/a.sdoc\n"), "{intent}");
    assert!(
        intent.contains("source_locator: spec/spec.md\n"),
        "{intent}"
    );
}

#[test]
fn a_mid_quoted_in_an_sdoc_multiline_string_is_not_a_declaration() {
    // Multi-line field values are not indented, so a quoted requirement
    // block puts `MID:` at column 0.
    let dir = project_with_sdoc();
    fs::write(
        dir.path().join("docs/guide.sdoc"),
        format!(
            "[DOCUMENT]\nTITLE: Guide\n\n[TEXT]\nSTATEMENT: >>>\n[REQUIREMENT]\nMID: {REQ_MID_1}\n<<<\n"
        ),
    )
    .unwrap();
    let export = export_of(&[document(
        Some(DOC_MID),
        "Doc A",
        &[requirement(REQ_MID_1, "REQ-1")],
    )]);

    let output = intent_from(dir.path(), &export, &[]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let intent = String::from_utf8(output.stdout).unwrap();
    assert!(intent.contains("source_locator: docs/a.sdoc\n"), "{intent}");
}

#[test]
fn a_mid_quoted_in_a_markdown_code_fence_is_not_a_declaration() {
    let dir = project_with_sdoc();
    let md_mid = "cccccccccccccccccccccccccccccccc";
    fs::create_dir_all(dir.path().join("spec")).unwrap();
    fs::write(
        dir.path().join("spec/spec.md"),
        format!("# Spec\n\n### Naming\n\n**MID**: {md_mid}\n**UID**: MD-1\n"),
    )
    .unwrap();
    fs::write(
        dir.path().join("spec/howto.md"),
        format!("# How to\n\n```markdown\n**MID**: {md_mid}\n```\n"),
    )
    .unwrap();
    let export = export_of(&[document(None, "Spec", &[requirement(md_mid, "MD-1")])]);

    let output = intent_from(dir.path(), &export, &[]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let intent = String::from_utf8(output.stdout).unwrap();
    assert!(
        intent.contains("source_locator: spec/spec.md\n"),
        "{intent}"
    );
}
