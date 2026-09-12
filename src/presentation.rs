use crate::canonical::CanonicalSnapshot;
use std::io::{self, Write};
use std::path::PathBuf;

#[derive(Debug, PartialEq)]
pub enum CommandOutcome {
    CanonicalImported(CanonicalSnapshot),
    Generated {
        count: usize,
        written: Vec<PathBuf>,
    },
    ChangesComputed {
        count: usize,
        to: String,
        /// Non-fatal issues surfaced alongside a successful computation
        /// (issue #29 §6: a legacy Knowledge schema version assumed for a
        /// ref that predates `[knowledge].schema_version`). Empty when
        /// nothing needs the user's attention.
        warnings: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentedResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

pub trait Presenter {
    fn present(&self, outcome: &CommandOutcome) -> PresentedResult;
}

pub fn emit(result: PresentedResult) -> io::Result<()> {
    io::stdout().write_all(result.stdout.as_bytes())?;
    io::stderr().write_all(result.stderr.as_bytes())?;
    if result.exit_code != 0 {
        std::process::exit(result.exit_code);
    }
    Ok(())
}

pub fn error(message: String, exit_code: i32) -> io::Result<()> {
    emit(PresentedResult {
        stdout: String::new(),
        stderr: message,
        exit_code,
    })
}

pub struct HumanPresenter;
pub struct JsonPresenter;

impl Presenter for HumanPresenter {
    fn present(&self, outcome: &CommandOutcome) -> PresentedResult {
        match outcome {
            CommandOutcome::CanonicalImported(snapshot) => PresentedResult {
                stdout: format!(
                    "imported {} artifact(s), {} relation(s), and {} evidence record(s)\n",
                    snapshot.artifacts.len(),
                    snapshot.relations.len(),
                    snapshot.evidence.len()
                ),
                stderr: String::new(),
                exit_code: 0,
            },
            CommandOutcome::Generated { count, .. } => PresentedResult {
                stdout: format!(
                    "generated {count} testcase(s) into .markharness/generated/testcases/\n"
                ),
                stderr: String::new(),
                exit_code: 0,
            },
            CommandOutcome::ChangesComputed {
                count,
                to,
                warnings,
            } => {
                let mut stdout = format!(
                    "computed {count} change event(s) into .markharness/changes/{to}.yaml\n"
                );
                for warning in warnings {
                    stdout.push_str(&format!("warning: {warning}\n"));
                }
                PresentedResult {
                    stdout,
                    stderr: String::new(),
                    exit_code: 0,
                }
            }
        }
    }
}

impl Presenter for JsonPresenter {
    fn present(&self, outcome: &CommandOutcome) -> PresentedResult {
        match outcome {
            CommandOutcome::CanonicalImported(snapshot) => PresentedResult {
                stdout: format!(
                    "{}\n",
                    serde_json::to_string_pretty(snapshot)
                        .expect("canonical snapshot serialization is infallible")
                ),
                stderr: String::new(),
                exit_code: 0,
            },
            CommandOutcome::Generated { count, written } => {
                let written: Vec<String> = written
                    .iter()
                    .map(|path| path.to_string_lossy().replace('\\', "/"))
                    .collect();
                let stdout = serde_json::json!({
                    "schema_version": 1,
                    "outcome": "generated",
                    "ok": true,
                    "generated": count,
                    "written": written,
                });
                PresentedResult {
                    stdout: format!("{stdout}\n"),
                    stderr: String::new(),
                    exit_code: 0,
                }
            }
            CommandOutcome::ChangesComputed {
                count,
                to,
                warnings,
            } => {
                let mut stdout = serde_json::json!({
                    "schema_version": 1,
                    "outcome": "changes_computed",
                    "audit_scope": crate::audit_scope::AuditScope::TwoSnapshot,
                    "changes": count,
                    "to": to,
                });
                // Optional field (design doc §5: only optional fields may be
                // added within one schema_version) — omitted, not `[]`, when
                // there's nothing to report, so existing v1 consumers see an
                // unchanged shape.
                if !warnings.is_empty() {
                    stdout["warnings"] = serde_json::json!(warnings);
                }
                PresentedResult {
                    stdout: format!("{stdout}\n"),
                    stderr: String::new(),
                    exit_code: 0,
                }
            }
        }
    }
}
