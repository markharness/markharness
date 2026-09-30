//! `knowledge intent-from-strictdoc`: turns a StrictDoc
//! `export --formats=json` file into a Knowledge Intent that
//! `knowledge reconcile` consumes. Read-only; nothing is written.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// Requirement MID → project-root-relative paths of the StrictDoc source
/// files (`.sdoc`, `.md`, `.markdown`) with a `MID:` line of that value.
pub type MidIndex = HashMap<String, Vec<String>>;

/// Indexes the MID lines of every StrictDoc source file under `sdoc_roots`.
/// The JSON export carries neither the source path nor, for Markdown
/// documents, a document MID, so the requirement's own MID is the only key
/// that leads back to its file.
pub fn index_source_mids(project_root: &Path, sdoc_roots: &[PathBuf]) -> Result<MidIndex, String> {
    let project_root = canonical(project_root)?;
    let mut roots = Vec::new();
    for sdoc_root in sdoc_roots {
        let sdoc_root = canonical(sdoc_root)?;
        if !sdoc_root.starts_with(&project_root) {
            return Err(format!(
                "--sdoc-root '{}' is outside the project '{}'",
                sdoc_root.display(),
                project_root.display()
            ));
        }
        roots.push(sdoc_root);
    }
    // A root inside another root (or listed twice) would be read again.
    roots.sort();
    let mut pending: Vec<PathBuf> = Vec::new();
    for root in roots {
        if !pending.last().is_some_and(|outer| root.starts_with(outer)) {
            pending.push(root);
        }
    }

    let mut index = MidIndex::new();
    while let Some(dir) = pending.pop() {
        let entries = fs::read_dir(&dir).map_err(|e| io_message(&dir, &e))?;
        for entry in entries {
            let entry = entry.map_err(|e| io_message(&dir, &e))?;
            let path = entry.path();
            let file_type = entry.file_type().map_err(|e| io_message(&path, &e))?;
            if file_type.is_dir() {
                if entry.file_name() != ".git" {
                    pending.push(path);
                }
            } else if file_type.is_file()
                && let Some(format) = SourceFormat::of(&path)
            {
                let bytes = fs::read(&path).map_err(|e| io_message(&path, &e))?;
                let text = String::from_utf8_lossy(&bytes);
                let locator = path
                    .strip_prefix(&project_root)
                    .expect("sdoc_root is inside project_root")
                    .to_string_lossy()
                    .replace('\\', "/");
                for mid in format.mids(&text) {
                    let locators = index.entry(mid.to_string()).or_default();
                    // One file's lines are visited together, so a MID
                    // declared twice in it is still one file.
                    if locators.last() != Some(&locator) {
                        locators.push(locator.clone());
                    }
                }
            }
        }
    }
    for locators in index.values_mut() {
        locators.sort();
    }
    Ok(index)
}

fn canonical(path: &Path) -> Result<PathBuf, String> {
    fs::canonicalize(path).map_err(|e| io_message(path, &e))
}

fn io_message(path: &Path, error: &std::io::Error) -> String {
    format!("cannot read '{}': {error}", path.display())
}

/// The two StrictDoc document syntaxes, which spell a node's MID
/// differently and are scanned by separate functions, since each syntax can
/// change independently of the other.
#[derive(Clone, Copy)]
enum SourceFormat {
    Sdoc,
    Markdown,
}

impl SourceFormat {
    fn of(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()? {
            "sdoc" => Some(Self::Sdoc),
            "md" | "markdown" => Some(Self::Markdown),
            _ => None,
        }
    }

    fn mids(self, text: &str) -> Vec<&str> {
        match self {
            Self::Sdoc => sdoc_mids(text),
            Self::Markdown => markdown_mids(text),
        }
    }
}

/// The MIDs a `.sdoc` file declares: `MID: <value>` at column 0, outside
/// multi-line strings. An indented line is a `[REQUIREMENT]` block quoted in
/// a text node (strictdoc's own user guide does this), and a multi-line
/// string (`FIELD: >>>` up to `<<<`) is text whose lines are not indented, so
/// a MID there is quoted, never declared.
fn sdoc_mids(text: &str) -> Vec<&str> {
    let mut mids = Vec::new();
    let mut in_string = false;
    for line in text.lines() {
        if in_string {
            in_string = line.trim() != "<<<";
        } else if opens_multiline_string(line) {
            in_string = true;
        } else if let Some(mid) = line
            .strip_prefix("MID:")
            .map(str::trim)
            .filter(|m| is_mid(m))
        {
            mids.push(mid);
        }
    }
    mids
}

/// `FIELD: >>>`, where FIELD is an upper-case field name. Anything else
/// ending in `>>>` (a title, say) is ordinary text.
fn opens_multiline_string(line: &str) -> bool {
    line.split_once(": ").is_some_and(|(name, value)| {
        !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
            && value.trim_end() == ">>>"
    })
}

/// The MIDs a Markdown file declares: `**MID**: <value>` at column 0,
/// outside fenced code blocks, where a MID is an example. A meta line ends
/// with ` \` unless it is the last one.
fn markdown_mids(text: &str) -> Vec<&str> {
    let mut mids = Vec::new();
    let mut fence: Option<(u8, usize)> = None;
    for line in text.lines() {
        match (fence, fence_of(line)) {
            (None, Some(opening)) => fence = Some(opening),
            (Some((marker, length)), Some((closing, count)))
                if closing == marker && count >= length && line.trim().len() == count =>
            {
                fence = None;
            }
            (Some(_), _) => {}
            (None, None) => {
                if let Some(mid) = line
                    .strip_prefix("**MID**:")
                    .map(|rest| rest.trim_end().trim_end_matches('\\').trim())
                    .filter(|m| is_mid(m))
                {
                    mids.push(mid);
                }
            }
        }
    }
    mids
}

/// A code fence line: up to three spaces, then three or more backticks or
/// tildes. Returns the fence character and its length.
fn fence_of(line: &str) -> Option<(u8, usize)> {
    let indent = line.bytes().take_while(|b| *b == b' ').count();
    let rest = line[indent..].trim_end();
    let marker = *rest.as_bytes().first()?;
    let length = rest.bytes().take_while(|b| *b == marker).count();
    (indent <= 3 && matches!(marker, b'`' | b'~') && length >= 3).then_some((marker, length))
}

/// A MID is lowercase hex; the length is not checked (StrictDoc's own
/// documents contain a 31-digit MID).
fn is_mid(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Builds the Intent YAML for every `REQUIREMENT` node in the export.
/// Fails as a whole on any inconsistency, so no partial Intent is emitted.
pub fn intent_from_strictdoc(export_json: &str, mid_index: &MidIndex) -> Result<String, String> {
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
        for requirement in requirements {
            let (name, mid) = requirement_mid(requirement)?;
            let locator = source_locator(name, mid, mid_index)?;
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

/// A requirement's display name and MID. The MID becomes both `source_key`
/// and `id: sd-<MID>`; only lowercase hex is accepted: that
/// is a valid slug, and the Intent is assembled by formatting,
/// so nothing else may reach it. The length is not checked: StrictDoc's
/// own docs carry a 31-digit MID.
fn requirement_mid(requirement: &Value) -> Result<(&str, &str), String> {
    let name = str_field(requirement, "UID")
        .or_else(|| str_field(requirement, "TITLE"))
        .unwrap_or("(unnamed)");
    let mid =
        str_field(requirement, "MID").ok_or_else(|| format!("requirement '{name}' has no MID"))?;
    if !is_mid(mid) {
        return Err(format!(
            "requirement '{name}' has MID '{mid}', which is not lowercase hex"
        ));
    }
    Ok((name, mid))
}

/// The source file a requirement is declared in.
fn source_locator<'a>(name: &str, mid: &str, mid_index: &'a MidIndex) -> Result<&'a str, String> {
    match mid_index.get(mid).map(Vec::as_slice) {
        Some([only]) => Ok(only),
        Some(several) => Err(format!(
            "requirement '{name}' (MID {mid}) is declared in several files: {} (remove the duplicate declaration, or narrow the scan with --sdoc-root)",
            several.join(", ")
        )),
        None => Err(format!(
            "requirement '{name}' (MID {mid}) is declared in no .sdoc/.md file under --sdoc-root"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sdoc_declares_only_column_0_lowercase_hex_mids() {
        let text = "[REQUIREMENT]\nMID: 00ff\n\n[REQUIREMENT]\nMID:\nMID: see below\n    MID: 0a\n";

        assert_eq!(sdoc_mids(text), ["00ff"]);
    }

    #[test]
    fn sdoc_multiline_strings_declare_nothing() {
        // `TITLE: a >>>` is a title, not a string opener.
        let text = "STATEMENT: >>>\nMID: 0a\n<<<\nMID: 0b\nTITLE: a >>>\nMID: 0c\n";

        assert_eq!(sdoc_mids(text), ["0b", "0c"]);
    }

    #[test]
    fn markdown_declares_only_column_0_lowercase_hex_mids() {
        let text = "**MID**: 00ff \\\n**UID**: X\n\n**MID**: see below\n  **MID**: 0a\n";

        assert_eq!(markdown_mids(text), ["00ff"]);
    }

    #[test]
    fn markdown_code_fences_declare_nothing() {
        assert_eq!(
            markdown_mids("```\n**MID**: 0a\n```\n**MID**: 0b\n"),
            ["0b"]
        );
        // A shorter fence does not close a longer one; a fence line with an
        // info string never closes.
        assert_eq!(
            markdown_mids("~~~~\n**MID**: 0a\n~~~\n**MID**: 0b\n~~~~\n**MID**: 0c\n"),
            ["0c"]
        );
        assert_eq!(
            markdown_mids("```md\n**MID**: 0a\n```md\n**MID**: 0b\n```\n**MID**: 0c\n"),
            ["0c"]
        );
        // Like CommonMark, an unclosed fence runs to the end of the file.
        assert!(markdown_mids("```\n**MID**: 0a\n").is_empty());
    }
}
