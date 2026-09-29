//! `knowledge intent-from-strictdoc` (ADR 0036): turns a StrictDoc
//! `export --formats=json` file into a Knowledge Intent that
//! `knowledge reconcile` consumes. Read-only; nothing is written.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde_json::Value;

/// Document MID → project-root-relative paths of the `.sdoc` files whose
/// header carries that MID.
pub type SdocIndex = HashMap<String, Vec<String>>;

/// Indexes every `*.sdoc` under `sdoc_root` by the `MID:` line in its
/// header. The JSON export carries no `.sdoc` path, so this is the only
/// way to learn which file a document came from (ADR 0036 §4).
pub fn index_sdoc_headers(project_root: &Path, sdoc_root: &Path) -> Result<SdocIndex, String> {
    let project_root = canonical(project_root)?;
    let sdoc_root = canonical(sdoc_root)?;
    if !sdoc_root.starts_with(&project_root) {
        return Err(format!(
            "--sdoc-root '{}' is outside the project '{}'",
            sdoc_root.display(),
            project_root.display()
        ));
    }

    let mut index = SdocIndex::new();
    let mut pending = vec![sdoc_root];
    while let Some(dir) = pending.pop() {
        for entry in read_dir(&dir)? {
            let entry = entry.map_err(|e| io_message(&dir, &e))?;
            let path = entry.path();
            let file_type = entry.file_type().map_err(|e| io_message(&path, &e))?;
            if file_type.is_dir() {
                if entry.file_name() != ".git" {
                    pending.push(path);
                }
            } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == "sdoc") {
                let text = fs::read_to_string(&path).map_err(|e| io_message(&path, &e))?;
                if let Some(mid) = header_mid(&text) {
                    let locator = path
                        .strip_prefix(&project_root)
                        .expect("sdoc_root is inside project_root")
                        .to_string_lossy()
                        .replace('\\', "/");
                    index.entry(mid).or_default().push(locator);
                }
            }
        }
    }
    for locators in index.values_mut() {
        locators.sort();
    }
    Ok(index)
}

fn canonical(path: &Path) -> Result<std::path::PathBuf, String> {
    fs::canonicalize(path).map_err(|e| io_message(path, &e))
}

fn read_dir(dir: &Path) -> Result<fs::ReadDir, String> {
    fs::read_dir(dir).map_err(|e| io_message(dir, &e))
}

fn io_message(path: &Path, error: &std::io::Error) -> String {
    format!("cannot read '{}': {error}", path.display())
}

/// The document-level `MID:` of a `.sdoc` file: the header is everything
/// between `[DOCUMENT]` and the next `[...]` block.
fn header_mid(sdoc: &str) -> Option<String> {
    let mut lines = sdoc.lines().skip_while(|line| line.trim().is_empty());
    if lines.next()?.trim() != "[DOCUMENT]" {
        return None;
    }
    lines
        .take_while(|line| !line.starts_with('['))
        .find_map(|line| line.strip_prefix("MID:"))
        .map(|mid| mid.trim().to_string())
}

/// Builds the Intent YAML for every `REQUIREMENT` node in the export.
/// Fails as a whole on any inconsistency (ADR 0036 §5).
pub fn intent_from_strictdoc(export_json: &str, sdoc_index: &SdocIndex) -> Result<String, String> {
    let export: Value =
        serde_json::from_str(export_json).map_err(|e| format!("invalid StrictDoc JSON: {e}"))?;
    let documents = export
        .get("DOCUMENTS")
        .and_then(Value::as_array)
        .ok_or("StrictDoc JSON has no DOCUMENTS array")?;

    let mut intent =
        String::from("format: markharness/knowledge-intent/v1\nmode: merge\n\nrequirements:\n");
    for document in documents {
        let mut requirements = Vec::new();
        collect_requirements(document, &mut requirements);
        if requirements.is_empty() {
            continue;
        }
        let locator = sdoc_locator(document, sdoc_index)?;
        for requirement in requirements {
            let mid = requirement_mid(requirement)?;
            intent.push_str(&format!(
                "  - id: sd-{mid}\n    source: external\n    axis: []\n    source_locator: {locator}\n    source_revision: current\n    source_key: {mid}\n"
            ));
        }
    }
    Ok(intent)
}

fn collect_requirements<'a>(node: &'a Value, out: &mut Vec<&'a Value>) {
    let Some(children) = node.get("NODES").and_then(Value::as_array) else {
        return;
    };
    for child in children {
        if str_field(child, "_NODE_TYPE") == Some("REQUIREMENT") {
            out.push(child);
        }
        collect_requirements(child, out);
    }
}

fn str_field<'a>(node: &'a Value, name: &str) -> Option<&'a str> {
    node.get(name).and_then(Value::as_str)
}

/// The `.sdoc` a document came from, found through its MID (ADR 0036 §4).
fn sdoc_locator<'a>(document: &Value, sdoc_index: &'a SdocIndex) -> Result<&'a str, String> {
    let title = str_field(document, "TITLE").unwrap_or("(untitled)");
    let mid = str_field(document, "MID")
        .ok_or_else(|| format!("document '{title}' has requirements but no MID"))?;
    match sdoc_index.get(mid).map(Vec::as_slice) {
        Some([only]) => Ok(only),
        Some(several) => Err(format!(
            "document '{title}' (MID {mid}) matches several .sdoc files: {}",
            several.join(", ")
        )),
        None => Err(format!(
            "document '{title}' (MID {mid}) matches no .sdoc file under --sdoc-root"
        )),
    }
}

/// A requirement's MID, which becomes both `source_key` and `id: sd-<MID>`.
/// Only 32 lowercase hex digits are accepted (ADR 0036 §3): that is a valid
/// slug (ADR 0030), and the Intent is assembled by formatting, so nothing
/// else may reach it.
fn requirement_mid(requirement: &Value) -> Result<&str, String> {
    let name = str_field(requirement, "UID")
        .or_else(|| str_field(requirement, "TITLE"))
        .unwrap_or("(unnamed)");
    let mid =
        str_field(requirement, "MID").ok_or_else(|| format!("requirement '{name}' has no MID"))?;
    if mid.len() != 32 || !mid.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return Err(format!(
            "requirement '{name}' has MID '{mid}', which is not 32 lowercase hex digits"
        ));
    }
    Ok(mid)
}
