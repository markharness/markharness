//! Stable, machine-readable diagnostic codes for `knowledge reconcile`
//! (ADR 0027 §7). The `code` string is the external contract; Rust type
//! names and internal error strings never leak into `--json` output.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticCode {
    InvalidFormat,
    DuplicateKey,
    DuplicateUid,
    UnknownLocalReference,
    UnknownAxis,
    AmbiguousIdentity,
    UnknownUid,
    ConflictingScope,
    ConflictingExistingValue,
    InvalidProcedureReference,
    InvalidSourceRevision,
    StalePlan,
    InvariantViolation,
}

impl DiagnosticCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            DiagnosticCode::InvalidFormat => "invalid_format",
            DiagnosticCode::DuplicateKey => "duplicate_key",
            DiagnosticCode::DuplicateUid => "duplicate_uid",
            DiagnosticCode::UnknownLocalReference => "unknown_local_reference",
            DiagnosticCode::UnknownAxis => "unknown_axis",
            DiagnosticCode::AmbiguousIdentity => "ambiguous_identity",
            DiagnosticCode::UnknownUid => "unknown_uid",
            DiagnosticCode::ConflictingScope => "conflicting_scope",
            DiagnosticCode::ConflictingExistingValue => "conflicting_existing_value",
            DiagnosticCode::InvalidProcedureReference => "invalid_procedure_reference",
            DiagnosticCode::InvalidSourceRevision => "invalid_source_revision",
            DiagnosticCode::StalePlan => "stale_plan",
            DiagnosticCode::InvariantViolation => "invariant_violation",
        }
    }
}

/// One reported problem: a stable `code`, the Intent-document location it
/// applies to (e.g. `"requirements[0]"`, `"features[1].contributes_to"`),
/// and a human-readable message (ADR 0027 §7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub location: String,
    pub message: String,
}

impl Diagnostic {
    pub fn new(
        code: DiagnosticCode,
        location: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Diagnostic {
            code,
            location: location.into(),
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_str_matches_adr_0027_diagnostic_names() {
        assert_eq!(DiagnosticCode::InvalidFormat.as_str(), "invalid_format");
        assert_eq!(DiagnosticCode::DuplicateKey.as_str(), "duplicate_key");
        assert_eq!(DiagnosticCode::DuplicateUid.as_str(), "duplicate_uid");
        assert_eq!(
            DiagnosticCode::UnknownLocalReference.as_str(),
            "unknown_local_reference"
        );
        assert_eq!(DiagnosticCode::UnknownAxis.as_str(), "unknown_axis");
        assert_eq!(
            DiagnosticCode::AmbiguousIdentity.as_str(),
            "ambiguous_identity"
        );
        assert_eq!(DiagnosticCode::UnknownUid.as_str(), "unknown_uid");
        assert_eq!(
            DiagnosticCode::ConflictingScope.as_str(),
            "conflicting_scope"
        );
        assert_eq!(
            DiagnosticCode::ConflictingExistingValue.as_str(),
            "conflicting_existing_value"
        );
        assert_eq!(
            DiagnosticCode::InvalidProcedureReference.as_str(),
            "invalid_procedure_reference"
        );
        assert_eq!(
            DiagnosticCode::InvalidSourceRevision.as_str(),
            "invalid_source_revision"
        );
        assert_eq!(DiagnosticCode::StalePlan.as_str(), "stale_plan");
        assert_eq!(
            DiagnosticCode::InvariantViolation.as_str(),
            "invariant_violation"
        );
    }
}
