//! Reconciliation Module for ADR 0027 (`knowledge reconcile`). CLI-agnostic:
//! `src/cli.rs` calls into this module the way it calls into
//! `identity::feature_ops`, keeping the Interface reusable by a future GUI.

pub mod diagnostics;
pub mod execute;
pub mod intent;
pub mod paths;
pub mod plan;
pub mod validate;

pub use diagnostics::{Diagnostic, DiagnosticCode};
pub use execute::{
    CreatedElement, ExecuteError, ReconcileError, ReconcileOutcome, UnchangedElement,
    UpdatedElement, check_creation, execute_creation_plan, reconcile_creation,
};
pub use intent::{INTENT_TEMPLATE, IntentDocument, IntentParseError};
pub use plan::{BehaviorOutcome, Plan, PlanError, build_plan};
