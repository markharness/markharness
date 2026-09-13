//! Reconciliation Module for ADR 0027 (`knowledge reconcile`). CLI-agnostic:
//! `src/cli.rs` calls into this module the way it calls into
//! `identity::feature_ops`, keeping the Interface reusable by a future GUI.

pub mod diagnostics;
pub mod intent;
pub mod validate;

pub use diagnostics::{Diagnostic, DiagnosticCode};
pub use intent::{IntentDocument, IntentParseError};
