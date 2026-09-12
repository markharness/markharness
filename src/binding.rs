//! `ExecutionBinding` — the declaration that a TestCase is verified by some
//! means, and where that means lives (ADR 0020, ADR 0025 §1).
//!
//! A binding is **not** an execution fact. It carries no timestamp, result,
//! Case revision, build, environment, attempt, or evidence, and its presence
//! must never be read as "executed" or "passed" (ADR 0025 §2). Anything
//! richer belongs to a future `ExecutionFact` — a separate type, never an
//! extension of this one.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::fs_safety::replace_file;
use crate::identity::CaseUid;

/// Fixed at 1 and never bumped (ADR 0026 §7). It exists so a future record
/// kind cannot be mistaken for this one, not to support reading older forms.
const SCHEMA_VERSION: u32 = 1;
const RECORD_KIND: &str = "execution_binding";

/// How a TestCase is verified. The distinction matters at review time: an
/// automated case can be read directly as test code, a manual one needs the
/// separate material `reference` points at (ADR 0020 background).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingMode {
    Automated,
    Manual,
}

impl BindingMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            BindingMode::Automated => "automated",
            BindingMode::Manual => "manual",
        }
    }
}

/// `deny_unknown_fields` is what makes AC32 hold: a file carrying `result`,
/// `executed_at`, `build`, or `environment` fails to parse instead of being
/// quietly accepted with those fields dropped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionBinding {
    pub schema_version: u32,
    pub record_kind: String,
    pub case_uid: CaseUid,
    pub mode: BindingMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
}

impl ExecutionBinding {
    pub fn new(case_uid: CaseUid, mode: BindingMode, reference: Option<String>) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            record_kind: RECORD_KIND.to_string(),
            case_uid,
            mode,
            reference,
        }
    }
}

#[derive(Debug)]
pub enum BindingError {
    /// The Case UID is empty, or shaped so that it would not stay inside
    /// `.markharness/bindings/` as a filename.
    UnsafeCaseUid(String),
    /// A stored binding could not be parsed — including a file carrying
    /// execution-fact fields this schema does not have (ADR 0025 §2).
    Malformed {
        path: PathBuf,
        message: String,
    },
    /// The file's `case_uid` disagrees with the Case UID its name encodes.
    /// One file per Case UID is the identity invariant `set_binding` writes
    /// under; accepting a mismatch would attach a verification declaration
    /// to a Case that never had it.
    CaseUidMismatch {
        path: PathBuf,
        file_name_uid: String,
        content_uid: String,
    },
    Io(io::Error),
}

impl From<io::Error> for BindingError {
    fn from(e: io::Error) -> Self {
        BindingError::Io(e)
    }
}

impl std::fmt::Display for BindingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BindingError::UnsafeCaseUid(value) => write!(
                f,
                "case uid \"{value}\" is not usable as a file name (no path separators, no \".\" or \"..\", not empty)"
            ),
            BindingError::Malformed { path, message } => {
                write!(f, "{}: {message}", path.display())
            }
            BindingError::CaseUidMismatch {
                path,
                file_name_uid,
                content_uid,
            } => write!(
                f,
                "{}: file name says case_uid \"{file_name_uid}\" but the record says \"{content_uid}\"",
                path.display()
            ),
            BindingError::Io(e) => write!(f, "filesystem error: {e}"),
        }
    }
}

pub fn bindings_dir(root: &Path) -> PathBuf {
    root.join(crate::project_root::MARKHARNESS_DIR)
        .join("bindings")
}

/// A Case UID is the sole component of the binding's path, so it is checked
/// before anything is created — the same reasoning `generate` applies to
/// `id:` and ADR 0024 §3 applies to `release_id`.
fn require_path_safe(case_uid: &str) -> Result<(), BindingError> {
    let unsafe_value = case_uid.trim().is_empty()
        || case_uid == "."
        || case_uid == ".."
        || case_uid.starts_with('.')
        || case_uid.contains('/')
        || case_uid.contains('\\')
        || case_uid.contains(':');
    if unsafe_value {
        return Err(BindingError::UnsafeCaseUid(case_uid.to_string()));
    }
    Ok(())
}

fn binding_path(root: &Path, case_uid: &str) -> PathBuf {
    bindings_dir(root).join(format!("{case_uid}.yml"))
}

/// Writes the binding for `case_uid`, replacing any existing one. A binding
/// is a current declaration, not an append-only log, so there is one file
/// per Case UID and `set` overwrites it.
pub fn set_binding(
    root: &Path,
    case_uid: &str,
    mode: BindingMode,
    reference: Option<String>,
) -> Result<ExecutionBinding, BindingError> {
    require_path_safe(case_uid)?;
    let case_uid = CaseUid::new(case_uid.to_string())
        .map_err(|e| BindingError::UnsafeCaseUid(e.type_name.to_string()))?;
    let binding = ExecutionBinding::new(case_uid.clone(), mode, reference);
    let content =
        serde_yaml_ng::to_string(&binding).expect("execution binding serialization is infallible");
    replace_file(
        root,
        &binding_path(root, case_uid.as_str()),
        content.as_bytes(),
    )?;
    Ok(binding)
}

/// Every stored binding, ordered by Case UID. A malformed file is an error,
/// never a silently skipped entry: a binding that cannot be read is not the
/// same as a TestCase with no binding.
pub fn read_all(root: &Path) -> Result<Vec<ExecutionBinding>, BindingError> {
    let dir = bindings_dir(root);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("yml"))
        .collect();
    paths.sort();

    let mut bindings = Vec::with_capacity(paths.len());
    for path in paths {
        let content = fs::read_to_string(&path)?;
        let binding: ExecutionBinding =
            serde_yaml_ng::from_str(&content).map_err(|e| BindingError::Malformed {
                path: path.clone(),
                message: e.to_string(),
            })?;
        if binding.record_kind != RECORD_KIND {
            return Err(BindingError::Malformed {
                path,
                message: format!(
                    "record_kind is \"{}\", expected \"{RECORD_KIND}\"",
                    binding.record_kind
                ),
            });
        }
        // Fixed at 1 and never bumped (ADR 0026 §7), so any other value is a
        // hand-edit or a record from a type this reader does not know —
        // never something to interpret optimistically.
        if binding.schema_version != SCHEMA_VERSION {
            return Err(BindingError::Malformed {
                path,
                message: format!(
                    "schema_version is {}, expected {SCHEMA_VERSION}",
                    binding.schema_version
                ),
            });
        }
        let file_name_uid = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default();
        if file_name_uid != binding.case_uid.as_str() {
            return Err(BindingError::CaseUidMismatch {
                file_name_uid: file_name_uid.to_string(),
                content_uid: binding.case_uid.as_str().to_string(),
                path,
            });
        }
        bindings.push(binding);
    }
    Ok(bindings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        dir
    }

    #[test]
    fn set_binding_writes_one_file_per_case_uid() {
        let dir = project();
        set_binding(dir.path(), "case-a", BindingMode::Manual, None).unwrap();
        set_binding(
            dir.path(),
            "case-b",
            BindingMode::Automated,
            Some("tests/b.spec.ts".to_string()),
        )
        .unwrap();

        let bindings = read_all(dir.path()).unwrap();
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0].case_uid.as_str(), "case-a");
        assert_eq!(bindings[1].mode, BindingMode::Automated);
    }

    #[test]
    fn set_binding_replaces_rather_than_appends() {
        let dir = project();
        set_binding(dir.path(), "case-a", BindingMode::Manual, None).unwrap();
        set_binding(
            dir.path(),
            "case-a",
            BindingMode::Automated,
            Some("tests/a.spec.ts".to_string()),
        )
        .unwrap();

        let bindings = read_all(dir.path()).unwrap();
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].mode, BindingMode::Automated);
        assert_eq!(bindings[0].reference.as_deref(), Some("tests/a.spec.ts"));
    }

    #[test]
    fn read_all_is_empty_when_nothing_is_bound() {
        let dir = project();
        assert!(read_all(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn a_binding_carrying_an_execution_fact_field_is_malformed() {
        let dir = project();
        let path = bindings_dir(dir.path()).join("case-a.yml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            "schema_version: 1\nrecord_kind: execution_binding\ncase_uid: case-a\nmode: manual\nresult: pass\n",
        )
        .unwrap();

        match read_all(dir.path()) {
            Err(BindingError::Malformed { message, .. }) => {
                assert!(message.contains("result"), "{message}");
            }
            other => panic!("expected a malformed-binding error, got {other:?}"),
        }
    }

    #[test]
    fn a_binding_whose_case_uid_disagrees_with_its_file_name_is_rejected() {
        let dir = project();
        set_binding(dir.path(), "case-a", BindingMode::Manual, None).unwrap();
        let path = bindings_dir(dir.path()).join("case-a.yml");
        let content = fs::read_to_string(&path)
            .unwrap()
            .replace("case_uid: case-a", "case_uid: case-b");
        fs::write(&path, content).unwrap();

        match read_all(dir.path()) {
            Err(BindingError::CaseUidMismatch {
                file_name_uid,
                content_uid,
                ..
            }) => {
                assert_eq!(file_name_uid, "case-a");
                assert_eq!(content_uid, "case-b");
            }
            other => panic!("expected a case-uid mismatch error, got {other:?}"),
        }
    }

    #[test]
    fn a_binding_with_an_unexpected_schema_version_is_rejected() {
        let dir = project();
        set_binding(dir.path(), "case-a", BindingMode::Manual, None).unwrap();
        let path = bindings_dir(dir.path()).join("case-a.yml");
        let content = fs::read_to_string(&path)
            .unwrap()
            .replace("schema_version: 1", "schema_version: 2");
        fs::write(&path, content).unwrap();

        match read_all(dir.path()) {
            Err(BindingError::Malformed { message, .. }) => {
                assert!(message.contains("schema_version"), "{message}");
            }
            other => panic!("expected a schema_version error, got {other:?}"),
        }
    }

    #[test]
    fn a_path_shaped_case_uid_is_refused_before_anything_is_written() {
        let dir = project();
        for hostile in ["..", ".", "a/b", "a\\b", "  ", ".hidden"] {
            assert!(
                matches!(
                    set_binding(dir.path(), hostile, BindingMode::Manual, None),
                    Err(BindingError::UnsafeCaseUid(_))
                ),
                "{hostile} must be refused"
            );
        }
        assert!(!bindings_dir(dir.path()).exists());
    }
}
