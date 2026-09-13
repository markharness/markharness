//! Static validation of a Knowledge Intent (ADR 0027 §7 Phase 1): checks
//! that depend only on the document itself and the registered Axis
//! registry, not on the repository's current Knowledge. State-dependent
//! matching and validation (existing-UID resolution, `ambiguous_identity`,
//! renames, reparenting) land in a later phase.

use std::collections::HashSet;

use super::diagnostics::{Diagnostic, DiagnosticCode};
use super::intent::{IntentDocument, StepIntent};

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
    check_duplicate_uids(doc, &mut diagnostics);
    check_unknown_local_references(doc, &mut diagnostics);
    check_unknown_axes(doc, known_axes, &mut diagnostics);
    check_display_ids_and_labels(doc, &mut diagnostics);
    check_blank_strings(doc, &mut diagnostics);
    diagnostics
}

/// A present-but-blank string is not the same as an omitted one, and is
/// never what the author meant. `knowledge::serialize_*` writes `label:`
/// as a plain scalar, so an empty one round-trips as YAML null and leaves
/// the file `knowledge reconcile` just wrote failing `markharness
/// validate`; an empty `action` or `results` entry is a step a Test
/// Executor cannot perform (the rule the deleted KnowledgeDraft validator
/// reported as `missing_steps`). Whitespace-only counts as blank: the
/// canonical form trims it away, so it would be stored as empty anyway.
fn check_blank_strings(doc: &IntentDocument, out: &mut Vec<Diagnostic>) {
    fn check(value: &Option<String>, location: String, out: &mut Vec<Diagnostic>) {
        if value.as_deref().is_some_and(|v| v.trim().is_empty()) {
            push_blank(location, out);
        }
    }

    for (i, req) in doc.requirements.iter().enumerate() {
        let location = format!("requirements[{i}]");
        check(&req.label, format!("{location}.label"), out);
        check(&req.description, format!("{location}.description"), out);
        check(
            &req.source_locator,
            format!("{location}.source_locator"),
            out,
        );
        for (j, issue) in req.related_issues.iter().flatten().enumerate() {
            if issue.trim().is_empty() {
                push_blank(format!("{location}.related_issues[{j}]"), out);
            }
        }
    }

    for (i, feature) in doc.features.iter().enumerate() {
        let location = format!("features[{i}]");
        check(&feature.label, format!("{location}.label"), out);
        check(&feature.description, format!("{location}.description"), out);
        for (j, behavior) in feature.behaviors.iter().enumerate() {
            let location = format!("{location}.behaviors[{j}]");
            check(&behavior.label, format!("{location}.label"), out);
            check(
                &behavior.description,
                format!("{location}.description"),
                out,
            );
            for (k, procedure) in behavior.procedures.iter().flatten().enumerate() {
                let location = format!("{location}.procedures[{k}]");
                if procedure.name.trim().is_empty() {
                    push_blank(format!("{location}.name"), out);
                }
                for (n, step) in procedure.steps.iter().enumerate() {
                    if step.trim().is_empty() {
                        push_blank(format!("{location}.steps[{n}]"), out);
                    }
                }
            }
            for (k, scenario) in behavior.scenarios.iter().enumerate() {
                let location = format!("{location}.scenarios[{k}]");
                check(&scenario.label, format!("{location}.label"), out);
                check(
                    &scenario.description,
                    format!("{location}.description"),
                    out,
                );
                check(
                    &scenario.implementation_note,
                    format!("{location}.implementation_note"),
                    out,
                );
                for (n, phase) in scenario.phases.iter().flatten().enumerate() {
                    let location = format!("{location}.phases[{n}]");
                    for (m, step) in phase.steps.iter().enumerate() {
                        let (field, value) = match step {
                            StepIntent::Action { action } => ("action", action),
                            StepIntent::Use { procedure } => ("use", procedure),
                        };
                        if value.trim().is_empty() {
                            push_blank(format!("{location}.steps[{m}].{field}"), out);
                        }
                    }
                    for (m, result) in phase.results.iter().enumerate() {
                        if result.trim().is_empty() {
                            push_blank(format!("{location}.results[{m}]"), out);
                        }
                    }
                }
            }
        }
    }
}

fn push_blank(location: String, out: &mut Vec<Diagnostic>) {
    out.push(Diagnostic::new(
        DiagnosticCode::MissingRequiredField,
        location,
        "must not be empty",
    ));
}

/// ADR 0028 §2: the display-id and label rules the deleted KnowledgeDraft
/// validator owned. A display id becomes a directory name and the key
/// other elements are matched by, and `knowledge::serialize_*` writes
/// `label:` as a plain scalar — a newline there produces a file that no
/// longer parses back.
fn check_display_ids_and_labels(doc: &IntentDocument, out: &mut Vec<Diagnostic>) {
    let mut check = |id: &Option<String>, label: &Option<String>, location: String| {
        if let Some(id) = id
            && !crate::knowledge::is_valid_slug(id)
        {
            out.push(Diagnostic::new(
                DiagnosticCode::InvalidSlug,
                format!("{location}.id"),
                format!(
                    "'{id}' is not a valid id (lowercase ASCII letters, digits and hyphen only)"
                ),
            ));
        }
        if let Some(label) = label
            && (label.contains('\n') || label.contains('\r'))
        {
            out.push(Diagnostic::new(
                DiagnosticCode::MultilineLabel,
                format!("{location}.label"),
                "label must be a single line",
            ));
        }
    };

    for (i, req) in doc.requirements.iter().enumerate() {
        check(&req.id, &req.label, format!("requirements[{i}]"));
    }
    for (i, feature) in doc.features.iter().enumerate() {
        check(&feature.id, &feature.label, format!("features[{i}]"));
        for (j, behavior) in feature.behaviors.iter().enumerate() {
            let location = format!("features[{i}].behaviors[{j}]");
            check(&behavior.id, &behavior.label, location.clone());
            for (k, scenario) in behavior.scenarios.iter().enumerate() {
                check(
                    &scenario.id,
                    &scenario.label,
                    format!("{location}.scenarios[{k}]"),
                );
            }
        }
    }
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

/// A `uid` names one existing element to select; the same Intent naming it
/// twice (even under different `key`s or nesting) is an ambiguous
/// instruction — there is no rule for which occurrence's fields should
/// win — so this is rejected statically rather than left to whichever
/// state-dependent pass runs last.
fn record_uid<'a>(
    uid: &'a Option<String>,
    location: String,
    seen: &mut HashSet<&'a str>,
    out: &mut Vec<Diagnostic>,
) {
    let Some(uid) = uid.as_deref() else {
        return;
    };
    if !seen.insert(uid) {
        out.push(Diagnostic::new(
            DiagnosticCode::DuplicateUid,
            location,
            format!("uid '{uid}' is referenced by more than one element in this Intent"),
        ));
    }
}

fn check_duplicate_uids(doc: &IntentDocument, out: &mut Vec<Diagnostic>) {
    let mut seen: HashSet<&str> = HashSet::new();

    for (i, req) in doc.requirements.iter().enumerate() {
        record_uid(&req.uid, format!("requirements[{i}].uid"), &mut seen, out);
    }
    for (i, feature) in doc.features.iter().enumerate() {
        record_uid(&feature.uid, format!("features[{i}].uid"), &mut seen, out);
        for (j, behavior) in feature.behaviors.iter().enumerate() {
            record_uid(
                &behavior.uid,
                format!("features[{i}].behaviors[{j}].uid"),
                &mut seen,
                out,
            );
            for (k, scenario) in behavior.scenarios.iter().enumerate() {
                record_uid(
                    &scenario.uid,
                    format!("features[{i}].behaviors[{j}].scenarios[{k}].uid"),
                    &mut seen,
                    out,
                );
            }
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
    fn detects_duplicate_uid_across_requirements() {
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - uid: 01ARZ3NDEKTSV4RRFFQ69G5FAV
    label: A
  - uid: 01ARZ3NDEKTSV4RRFFQ69G5FAV
    label: B
";
        let doc = parse_intent(yaml).unwrap();
        let diagnostics = validate_static(&doc, &axes(&[]));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::DuplicateUid);
        assert_eq!(diagnostics[0].location, "requirements[1].uid");
    }

    #[test]
    fn detects_duplicate_uid_across_different_kinds() {
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - uid: 01ARZ3NDEKTSV4RRFFQ69G5FAV
    label: A

features:
  - uid: 01ARZ3NDEKTSV4RRFFQ69G5FAV
    label: B
";
        let doc = parse_intent(yaml).unwrap();
        let diagnostics = validate_static(&doc, &axes(&[]));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::DuplicateUid);
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

    /// ADR 0028 §2: the display-id format rule the old KnowledgeDraft
    /// validator owned moves here rather than disappearing with it. A
    /// display id becomes a directory name and a reference key, so it
    /// cannot be left unchecked.
    #[test]
    fn detects_an_invalid_slug_on_every_kind_of_display_id() {
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - key: req
    id: Todo_Management
    source: native
    label: TODO
    axis: []

features:
  - key: feature
    id: Add Todo
    contributes_to: [req]
    label: Add
    axis: []
    behaviors:
      - id: ADD
        label: Add
        axis: []
        description: Adds.
        scenarios:
          - id: empty~title
            label: Empty
            description: Empty title.
            phases:
              - steps:
                  - action: Submit
                results:
                  - Rejected
";
        let doc = parse_intent(yaml).unwrap();
        let diagnostics = validate_static(&doc, &axes(&[]));

        let slugs: Vec<&str> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::InvalidSlug)
            .map(|d| d.location.as_str())
            .collect();
        assert_eq!(
            slugs,
            vec![
                "requirements[0].id",
                "features[0].id",
                "features[0].behaviors[0].id",
                "features[0].behaviors[0].scenarios[0].id",
            ]
        );
    }

    /// ADR 0028 §2: `knowledge::serialize_*` writes `label:` as a plain
    /// scalar, so a label carrying a newline produces a file that no
    /// longer round-trips. The old KnowledgeDraft validator was what kept
    /// that invariant; without this check, deleting it would let a broken
    /// file be written.
    #[test]
    fn detects_a_multiline_label_on_every_kind() {
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - key: req
    id: todo
    source: native
    label: \"line one\\nline two\"
    axis: []

features:
  - key: feature
    id: add-todo
    contributes_to: [req]
    label: \"add\\ntodo\"
    axis: []
    behaviors:
      - id: add
        label: \"a\\nb\"
        axis: []
        description: Adds.
        scenarios:
          - id: empty-title
            label: \"c\\nd\"
            description: Empty title.
            phases:
              - steps:
                  - action: Submit
                results:
                  - Rejected
";
        let doc = parse_intent(yaml).unwrap();
        let diagnostics = validate_static(&doc, &axes(&[]));

        let labels: Vec<&str> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::MultilineLabel)
            .map(|d| d.location.as_str())
            .collect();
        assert_eq!(
            labels,
            vec![
                "requirements[0].label",
                "features[0].label",
                "features[0].behaviors[0].label",
                "features[0].behaviors[0].scenarios[0].label",
            ]
        );
    }

    #[test]
    fn a_valid_intent_reports_no_slug_or_label_diagnostics() {
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - key: req
    id: todo-management
    source: native
    label: TODO management
    axis: []
";
        let doc = parse_intent(yaml).unwrap();
        assert!(validate_static(&doc, &axes(&[])).is_empty());
    }

    /// A present-but-blank required string is not the same as an omitted
    /// one: `label: ""` serializes as `label: `, which parses back as YAML
    /// null, so the file `knowledge reconcile` wrote no longer satisfies
    /// `markharness validate`. Blank `steps`/`results` entries are
    /// meaningless for the same reason the old KnowledgeDraft validator
    /// rejected them — a Test Executor cannot perform an empty step.
    #[test]
    fn detects_blank_required_strings() {
        let yaml = "format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - key: req
    id: todo
    source: native
    label: \"\"
    axis: []

features:
  - key: feature
    id: add-todo
    contributes_to: [req]
    label: add-todo
    axis: []
    description: \"   \"
    behaviors:
      - id: add
        label: add
        axis: []
        description: Adds.
        procedures:
          - name: \"\"
            steps:
              - \"\"
        scenarios:
          - id: empty-title
            label: empty-title
            description: Empty title.
            phases:
              - steps:
                  - action: \"\"
                results:
                  - \"  \"
";
        let doc = parse_intent(yaml).unwrap();
        let diagnostics = validate_static(&doc, &axes(&[]));
        let blanks: Vec<&str> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::MissingRequiredField)
            .map(|d| d.location.as_str())
            .collect();
        for expected in [
            "requirements[0].label",
            "features[0].description",
            "features[0].behaviors[0].procedures[0].name",
            "features[0].behaviors[0].procedures[0].steps[0]",
            "features[0].behaviors[0].scenarios[0].phases[0].steps[0].action",
            "features[0].behaviors[0].scenarios[0].phases[0].results[0]",
        ] {
            assert!(
                blanks.contains(&expected),
                "expected a blank-string diagnostic at {expected}, got {blanks:?}"
            );
        }
    }

    #[test]
    fn a_blank_source_locator_is_reported() {
        let yaml = "format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - id: todo
    source: external
    axis: []
    source_locator: \"\"
    source_revision: current
";
        let doc = parse_intent(yaml).unwrap();
        let diagnostics = validate_static(&doc, &axes(&[]));
        assert!(
            diagnostics
                .iter()
                .any(|d| d.location == "requirements[0].source_locator"
                    && d.code == DiagnosticCode::MissingRequiredField),
            "{diagnostics:?}"
        );
    }
}
