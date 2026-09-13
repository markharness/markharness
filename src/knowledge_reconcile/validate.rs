//! Static validation of a Knowledge Intent (ADR 0027 §7 Phase 1): checks
//! that depend only on the document itself and the registered Axis
//! registry, not on the repository's current Knowledge. State-dependent
//! matching and validation (existing-UID resolution, `ambiguous_identity`,
//! renames, reparenting) land in a later phase.

use std::collections::HashSet;

use super::diagnostics::{Diagnostic, DiagnosticCode};
use super::intent::IntentDocument;

/// A string that parses as a ULID is treated as an attempted UID reference
/// rather than a document-local `key` (ADR 0027 §2/§3): UIDs are issued via
/// `ulid::Ulid::new()` in `identity::feature_ops`, so reusing that crate's
/// parser here classifies a reference the same way it was produced,
/// instead of re-deriving the ULID format as a second source of truth.
/// Whether an attempted UID actually exists is a repo-state check deferred
/// to a later phase (`unknown_uid`), not this static pass.
fn looks_like_uid(value: &str) -> bool {
    ulid::Ulid::from_string(value).is_ok()
}

/// Runs every check that needs only the Intent document plus the set of
/// currently registered Axis ids (ADR 0027 §4: "Axisは登録済みのものだけを
/// 参照できる").
pub fn validate_static(doc: &IntentDocument, known_axes: &HashSet<String>) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    check_duplicate_keys(doc, &mut diagnostics);
    check_unknown_local_references(doc, &mut diagnostics);
    check_unknown_axes(doc, known_axes, &mut diagnostics);
    diagnostics
}

fn record_key<'a>(
    key: &'a Option<String>,
    location: String,
    seen: &mut HashSet<&'a str>,
    out: &mut Vec<Diagnostic>,
) {
    let Some(key) = key.as_deref() else {
        return;
    };
    if !seen.insert(key) {
        out.push(Diagnostic::new(
            DiagnosticCode::DuplicateKey,
            location,
            format!("key '{key}' is used by more than one element in this Intent"),
        ));
    }
}

fn check_duplicate_keys(doc: &IntentDocument, out: &mut Vec<Diagnostic>) {
    let mut seen: HashSet<&str> = HashSet::new();

    for (i, req) in doc.requirements.iter().enumerate() {
        record_key(&req.key, format!("requirements[{i}].key"), &mut seen, out);
    }
    for (i, feature) in doc.features.iter().enumerate() {
        record_key(&feature.key, format!("features[{i}].key"), &mut seen, out);
        for (j, behavior) in feature.behaviors.iter().enumerate() {
            record_key(
                &behavior.key,
                format!("features[{i}].behaviors[{j}].key"),
                &mut seen,
                out,
            );
        }
    }
}

fn check_unknown_local_references(doc: &IntentDocument, out: &mut Vec<Diagnostic>) {
    let requirement_keys: HashSet<&str> = doc
        .requirements
        .iter()
        .filter_map(|r| r.key.as_deref())
        .collect();

    for (i, feature) in doc.features.iter().enumerate() {
        let Some(contributes_to) = &feature.contributes_to else {
            continue;
        };
        for (j, reference) in contributes_to.iter().enumerate() {
            if requirement_keys.contains(reference.as_str()) || looks_like_uid(reference) {
                continue;
            }
            out.push(Diagnostic::new(
                DiagnosticCode::UnknownLocalReference,
                format!("features[{i}].contributes_to[{j}]"),
                format!(
                    "'{reference}' matches no requirement key declared in this Intent and is not a UID"
                ),
            ));
        }
    }
}

fn check_unknown_axes(
    doc: &IntentDocument,
    known_axes: &HashSet<String>,
    out: &mut Vec<Diagnostic>,
) {
    let check = |axis: &Option<Vec<String>>, location_prefix: String, out: &mut Vec<Diagnostic>| {
        let Some(axis_ids) = axis else {
            return;
        };
        for (k, axis_id) in axis_ids.iter().enumerate() {
            if !known_axes.contains(axis_id) {
                out.push(Diagnostic::new(
                    DiagnosticCode::UnknownAxis,
                    format!("{location_prefix}.axis[{k}]"),
                    format!("axis '{axis_id}' is not registered under axes/"),
                ));
            }
        }
    };

    for (i, req) in doc.requirements.iter().enumerate() {
        check(&req.axis, format!("requirements[{i}]"), out);
    }
    for (i, feature) in doc.features.iter().enumerate() {
        check(&feature.axis, format!("features[{i}]"), out);
        for (j, behavior) in feature.behaviors.iter().enumerate() {
            check(&behavior.axis, format!("features[{i}].behaviors[{j}]"), out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge_reconcile::intent::parse_intent;

    fn axes(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_diagnostics_for_a_fully_valid_intent() {
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - key: req_todo
    id: todo
    source: native
    label: TODO management
    axis: [functional]

features:
  - key: feature_todo
    id: todo-management
    contributes_to: [req_todo]
    label: TODO management
    axis: [functional]
";
        let doc = parse_intent(yaml).unwrap();
        let diagnostics = validate_static(&doc, &axes(&["functional"]));
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn detects_duplicate_key_across_requirements() {
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - key: dup
    id: a
    source: native
    label: A
    axis: []
  - key: dup
    id: b
    source: native
    label: B
    axis: []
";
        let doc = parse_intent(yaml).unwrap();
        let diagnostics = validate_static(&doc, &axes(&[]));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::DuplicateKey);
        assert_eq!(diagnostics[0].location, "requirements[1].key");
    }

    #[test]
    fn detects_duplicate_key_across_different_kinds() {
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - key: shared
    id: a
    source: native
    label: A
    axis: []

features:
  - key: shared
    id: b
    label: B
    axis: []
";
        let doc = parse_intent(yaml).unwrap();
        let diagnostics = validate_static(&doc, &axes(&[]));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::DuplicateKey);
    }

    #[test]
    fn detects_unknown_local_reference_in_contributes_to() {
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

features:
  - id: todo-management
    contributes_to: [does_not_exist]
    label: TODO management
    axis: []
";
        let doc = parse_intent(yaml).unwrap();
        let diagnostics = validate_static(&doc, &axes(&[]));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::UnknownLocalReference);
        assert_eq!(diagnostics[0].location, "features[0].contributes_to[0]");
    }

    #[test]
    fn accepts_ulid_shaped_contributes_to_reference_without_checking_existence() {
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

features:
  - id: todo-management
    contributes_to: [01ARZ3NDEKTSV4RRFFQ69G5FAV]
    label: TODO management
    axis: []
";
        let doc = parse_intent(yaml).unwrap();
        let diagnostics = validate_static(&doc, &axes(&[]));
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn detects_unknown_axis_on_requirement_and_feature() {
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - key: req_todo
    id: todo
    source: native
    label: TODO management
    axis: [nonexistent]

features:
  - id: todo-management
    label: TODO management
    axis: [also_missing]
";
        let doc = parse_intent(yaml).unwrap();
        let diagnostics = validate_static(&doc, &axes(&[]));
        assert_eq!(diagnostics.len(), 2);
        assert!(
            diagnostics
                .iter()
                .all(|d| d.code == DiagnosticCode::UnknownAxis)
        );
    }
}
