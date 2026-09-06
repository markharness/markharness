use crate::canonical::CanonicalSnapshot;
use crate::plan::VerificationPlan;
use std::io::{self, Write};
use std::path::PathBuf;

#[derive(Debug, PartialEq)]
pub enum CommandOutcome {
    CanonicalImported(CanonicalSnapshot),
    PlanBuilt(VerificationPlan),
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

fn plan_exit_code(plan: &VerificationPlan) -> i32 {
    // ADR 0017 §5: unresolved (mutually conflicting) evidence must never be
    // treated as a clean plan — it needs the same human attention as an
    // outright failure, not silent success.
    if plan.summary.failed > 0 || plan.summary.unresolved > 0 {
        1
    } else if plan.summary.pending > 0
        || plan.summary.stale_evidence > 0
        || plan
            .new_required_tests
            .iter()
            .any(|proposal| proposal.decision == crate::plan::ProposalDecision::Proposed)
    {
        2
    } else {
        0
    }
}

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
            CommandOutcome::PlanBuilt(plan) => PresentedResult {
                stdout: format!(
                    "verification plan: {} changed feature(s), {} affected test(s), {} proposal(s)\n",
                    plan.summary.changed_features,
                    plan.summary.affected_tests,
                    plan.summary.new_tests
                ),
                stderr: String::new(),
                exit_code: plan_exit_code(plan),
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
            CommandOutcome::PlanBuilt(plan) => PresentedResult {
                stdout: format!(
                    "{}\n",
                    serde_json::to_string_pretty(plan)
                        .expect("verification plan serialization is infallible")
                ),
                stderr: String::new(),
                exit_code: plan_exit_code(plan),
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
