use crate::canonical::CanonicalSnapshot;
use crate::plan::VerificationPlan;
use crate::verify::PendingReport;
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
    },
    Pending {
        report: PendingReport,
        fail_on_pending: bool,
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
    if plan.summary.failed > 0 {
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
                stdout: format!("generated {count} testcase(s) into generated/testcases/\n"),
                stderr: String::new(),
                exit_code: 0,
            },
            CommandOutcome::ChangesComputed { count, to } => PresentedResult {
                stdout: format!("computed {count} change event(s) into changes/{to}.yaml\n"),
                stderr: String::new(),
                exit_code: 0,
            },
            CommandOutcome::Pending {
                report,
                fail_on_pending,
            } => {
                let mut stdout = String::from("pending (再実行なし):\n");
                if report.pending.is_empty() {
                    stdout.push_str("  (なし)\n");
                } else {
                    for entry in &report.pending {
                        stdout.push_str(&format!(
                            "  - {}  ({} の変更 {} の影響、未実行)\n",
                            entry.case_id, entry.feature_id, entry.event_id
                        ));
                    }
                }
                stdout.push_str("\nstale (影響範囲がさらに変更済み):\n");
                if report.stale.is_empty() {
                    stdout.push_str("  (なし)\n");
                } else {
                    for entry in &report.stale {
                        let current = entry
                            .current_event
                            .as_ref()
                            .map(|event| event.event_id.as_str())
                            .unwrap_or("(不明)");
                        stdout.push_str(&format!(
                            "  - {}  ({} の変更 {} は陳腐化、現在の確認対象は {})\n",
                            entry.case_id, entry.feature_id, entry.original_event_id, current
                        ));
                    }
                }
                PresentedResult {
                    stdout,
                    stderr: String::new(),
                    exit_code: if *fail_on_pending && !report.pending.is_empty() {
                        1
                    } else {
                        0
                    },
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
            CommandOutcome::ChangesComputed { count, to } => PresentedResult {
                stdout: format!(
                    "{{\"schema_version\":1,\"outcome\":\"changes_computed\",\"changes\":{count},\"to\":{}}}\n",
                    serde_json::to_string(to).expect("milestone serialization is infallible")
                ),
                stderr: String::new(),
                exit_code: 0,
            },
            CommandOutcome::Pending {
                report,
                fail_on_pending,
            } => {
                let stdout = serde_json::json!({
                    "schema_version": 1,
                    "outcome": "pending",
                    "pending": report.pending,
                    "stale": report.stale,
                });
                PresentedResult {
                    stdout: format!("{stdout}\n"),
                    stderr: String::new(),
                    exit_code: if *fail_on_pending && !report.pending.is_empty() {
                        1
                    } else {
                        0
                    },
                }
            }
        }
    }
}
