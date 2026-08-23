//! The `audit_scope` field (design doc §11, ADR 0013 検証規則): every
//! command's JSON output that touches `.markharness` state must say, in a
//! machine-readable way, how much history it actually looked at — so a CI
//! gate can tell a narrow, cheap check (`changes compute`, `verify`) apart
//! from `identity migrate`'s working-tree scan or `identity audit`'s full
//! commit-history walk, without relying on documentation alone. A typed
//! enum (rather than each call site spelling out its own string literal)
//! makes an unsupported value or a typo'd rename a compile error instead
//! of a silent JSON contract break.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditScope {
    /// `changes compute`, `verify pending`, `verify trace`: compares
    /// exactly the two `.markharness` snapshots named by the command's
    /// arguments, and nothing else.
    TwoSnapshot,
    /// `identity migrate`: inspects only the current working tree, not
    /// any committed history.
    WorkingTree,
    /// `identity audit`: walks a ref's entire first-parent commit
    /// history (`identity::audit::run_audit`).
    FullHistory,
}

impl AuditScope {
    pub fn as_str(self) -> &'static str {
        match self {
            AuditScope::TwoSnapshot => "two_snapshot",
            AuditScope::WorkingTree => "working_tree",
            AuditScope::FullHistory => "full_history",
        }
    }
}

impl std::fmt::Display for AuditScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_str_matches_the_serialized_json_value_for_every_variant() {
        for (scope, expected) in [
            (AuditScope::TwoSnapshot, "two_snapshot"),
            (AuditScope::WorkingTree, "working_tree"),
            (AuditScope::FullHistory, "full_history"),
        ] {
            assert_eq!(scope.as_str(), expected);
            assert_eq!(
                serde_json::to_value(scope).unwrap(),
                serde_json::Value::String(expected.to_string())
            );
        }
    }
}
