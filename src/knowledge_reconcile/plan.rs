//! Builds a creation plan for a Knowledge Intent that has already passed
//! [`super::validate::validate_static`] (ADR 0027 §7 Phase 2). This phase
//! implements only the matching table's "new" row (ADR 0027 §3): no UID,
//! and no existing element with the same kind/scope/id. UID-selected
//! updates and renames (Phase 3) are not yet supported here; see
//! [`PlanError::NotYetSupported`].

use std::collections::HashMap;
use std::io;
use std::path::Path;

use crate::identity::{EntityKind, knowledge_walk};
use crate::knowledge::{Feature, Requirement, RequirementSource};

use super::diagnostics::{Diagnostic, DiagnosticCode};
use super::intent::IntentDocument;

#[derive(Debug)]
pub struct NewRequirement {
    pub uid: String,
    pub id: String,
    pub source: RequirementSource,
    pub label: String,
    pub axis: Vec<String>,
    pub description: Option<String>,
}

impl NewRequirement {
    pub fn to_canonical(&self) -> Requirement {
        Requirement {
            id: self.id.clone(),
            source: self.source,
            label: Some(self.label.clone()),
            axis: self.axis.clone(),
            description: self.description.clone(),
            source_locator: None,
            source_revision: None,
            related_issues: Vec::new(),
            uid: Some(self.uid.clone()),
        }
    }
}

#[derive(Debug)]
pub struct NewFeature {
    pub uid: String,
    pub id: String,
    pub requirement_uids: Vec<String>,
    pub label: String,
    pub axis: Vec<String>,
    pub description: Option<String>,
}

impl NewFeature {
    pub fn to_canonical(&self) -> Feature {
        Feature {
            id: self.id.clone(),
            requirement_uids: self.requirement_uids.clone(),
            label: self.label.clone(),
            axis: self.axis.clone(),
            description: self.description.clone(),
            forked_from: None,
            uid: Some(self.uid.clone()),
        }
    }
}

#[derive(Debug, Default)]
pub struct Plan {
    pub new_requirements: Vec<NewRequirement>,
    pub new_features: Vec<NewFeature>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.new_requirements.is_empty() && self.new_features.is_empty()
    }
}

/// Why [`build_plan`] could not produce a [`Plan`].
#[derive(Debug)]
pub enum PlanError {
    /// One or more elements failed a state-dependent check (ADR 0027 §7).
    Diagnostics(Vec<Diagnostic>),
    /// The Intent contains an element this phase does not yet resolve: a
    /// UID-selected element (patch/rename, ADR 0027 §3's "existing" rows)
    /// or an existing element with no UID given (this phase cannot yet
    /// tell "unchanged" apart from "ambiguous_identity" without comparing
    /// content). Not one of §7's stable diagnostic codes — a later phase
    /// replaces this function's handling of these rows entirely, so this
    /// variant is not a contract callers should depend on.
    NotYetSupported(String),
    Io(io::Error),
}

impl From<io::Error> for PlanError {
    fn from(e: io::Error) -> Self {
        PlanError::Io(e)
    }
}

pub fn build_plan(root: &Path, doc: &IntentDocument) -> Result<Plan, PlanError> {
    let mut diagnostics = Vec::new();
    let mut plan = Plan::default();
    let mut requirement_uid_by_key: HashMap<&str, String> = HashMap::new();

    for (i, req) in doc.requirements.iter().enumerate() {
        if req.uid.is_some() {
            return Err(PlanError::NotYetSupported(format!(
                "requirements[{i}]: UID-selected updates are not supported before ADR 0027 Phase 3"
            )));
        }
        let Some(id) = &req.id else {
            diagnostics.push(Diagnostic::new(
                DiagnosticCode::MissingRequiredField,
                format!("requirements[{i}].id"),
                "id is required",
            ));
            continue;
        };
        if knowledge_walk::find_by_id(root, EntityKind::Requirement, id)?.is_some() {
            diagnostics.push(Diagnostic::new(
                DiagnosticCode::AmbiguousIdentity,
                format!("requirements[{i}]"),
                format!(
                    "a Requirement with id '{id}' already exists; supply its UID to patch or rename it"
                ),
            ));
            continue;
        }
        let source = match req.source.as_deref() {
            Some("native") => RequirementSource::Native,
            Some("external") => RequirementSource::External,
            _ => {
                diagnostics.push(Diagnostic::new(
                    DiagnosticCode::MissingRequiredField,
                    format!("requirements[{i}].source"),
                    "source must be 'native' or 'external'",
                ));
                continue;
            }
        };
        let uid = ulid::Ulid::new().to_string();
        if let Some(key) = &req.key {
            requirement_uid_by_key.insert(key.as_str(), uid.clone());
        }
        plan.new_requirements.push(NewRequirement {
            uid,
            id: id.clone(),
            source,
            label: req.label.clone().unwrap_or_else(|| id.clone()),
            axis: req.axis.clone().unwrap_or_default(),
            description: req.description.clone(),
        });
    }

    for (i, feature) in doc.features.iter().enumerate() {
        if feature.uid.is_some() {
            return Err(PlanError::NotYetSupported(format!(
                "features[{i}]: UID-selected updates are not supported before ADR 0027 Phase 3"
            )));
        }
        let Some(id) = &feature.id else {
            diagnostics.push(Diagnostic::new(
                DiagnosticCode::MissingRequiredField,
                format!("features[{i}].id"),
                "id is required",
            ));
            continue;
        };
        if knowledge_walk::find_by_id(root, EntityKind::Feature, id)?.is_some() {
            diagnostics.push(Diagnostic::new(
                DiagnosticCode::AmbiguousIdentity,
                format!("features[{i}]"),
                format!(
                    "a Feature with id '{id}' already exists; supply its UID to patch or rename it"
                ),
            ));
            continue;
        }
        let mut requirement_uids = Vec::new();
        if let Some(contributes_to) = &feature.contributes_to {
            for (j, reference) in contributes_to.iter().enumerate() {
                if let Some(uid) = requirement_uid_by_key.get(reference.as_str()) {
                    requirement_uids.push(uid.clone());
                    continue;
                }
                match knowledge_walk::find_by_uid(root, EntityKind::Requirement, reference)? {
                    Some(_) => requirement_uids.push(reference.clone()),
                    None => diagnostics.push(Diagnostic::new(
                        DiagnosticCode::UnknownUid,
                        format!("features[{i}].contributes_to[{j}]"),
                        format!("no Requirement with uid '{reference}' exists"),
                    )),
                }
            }
        }
        plan.new_features.push(NewFeature {
            uid: ulid::Ulid::new().to_string(),
            id: id.clone(),
            requirement_uids,
            label: feature.label.clone().unwrap_or_else(|| id.clone()),
            axis: feature.axis.clone().unwrap_or_default(),
            description: feature.description.clone(),
        });
    }

    if !diagnostics.is_empty() {
        return Err(PlanError::Diagnostics(diagnostics));
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge_reconcile::intent::parse_intent;

    fn init_project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".markharness/knowledge")).unwrap();
        dir
    }

    #[test]
    fn plans_a_new_requirement_and_feature_resolving_contributes_to_by_local_key() {
        let dir = init_project();
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
        let plan = build_plan(dir.path(), &doc).unwrap();

        assert_eq!(plan.new_requirements.len(), 1);
        assert_eq!(plan.new_features.len(), 1);
        let requirement_uid = plan.new_requirements[0].uid.clone();
        assert_eq!(plan.new_features[0].requirement_uids, vec![requirement_uid]);
    }

    #[test]
    fn reports_ambiguous_identity_when_a_requirement_id_already_exists() {
        let dir = init_project();
        std::fs::create_dir_all(dir.path().join(".markharness/knowledge/requirements/todo"))
            .unwrap();
        std::fs::write(
            dir.path()
                .join(".markharness/knowledge/requirements/todo/requirement.yml"),
            "id: todo\nsource: native\nlabel: todo\naxis: []\n",
        )
        .unwrap();
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - id: todo
    source: native
    label: TODO management
    axis: []
";
        let doc = parse_intent(yaml).unwrap();
        let err = build_plan(dir.path(), &doc).unwrap_err();
        match err {
            PlanError::Diagnostics(diagnostics) => {
                assert_eq!(diagnostics.len(), 1);
                assert_eq!(diagnostics[0].code, DiagnosticCode::AmbiguousIdentity);
            }
            other => panic!("expected Diagnostics, got {other:?}"),
        }
    }

    #[test]
    fn reports_unknown_uid_when_contributes_to_names_a_nonexistent_requirement() {
        let dir = init_project();
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
        let err = build_plan(dir.path(), &doc).unwrap_err();
        match err {
            PlanError::Diagnostics(diagnostics) => {
                assert_eq!(diagnostics.len(), 1);
                assert_eq!(diagnostics[0].code, DiagnosticCode::UnknownUid);
            }
            other => panic!("expected Diagnostics, got {other:?}"),
        }
    }

    #[test]
    fn resolves_contributes_to_against_an_existing_requirement_uid() {
        let dir = init_project();
        std::fs::create_dir_all(dir.path().join(".markharness/knowledge/requirements/todo"))
            .unwrap();
        std::fs::write(
            dir.path()
                .join(".markharness/knowledge/requirements/todo/requirement.yml"),
            "id: todo\nsource: native\nlabel: todo\naxis: []\nuid: 01ARZ3NDEKTSV4RRFFQ69G5FAV\n",
        )
        .unwrap();
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
        let plan = build_plan(dir.path(), &doc).unwrap();
        assert_eq!(
            plan.new_features[0].requirement_uids,
            vec!["01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string()]
        );
    }

    #[test]
    fn rejects_a_requirement_with_missing_source() {
        let dir = init_project();
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - id: todo
    label: TODO management
    axis: []
";
        let doc = parse_intent(yaml).unwrap();
        let err = build_plan(dir.path(), &doc).unwrap_err();
        match err {
            PlanError::Diagnostics(diagnostics) => {
                assert_eq!(diagnostics[0].code, DiagnosticCode::MissingRequiredField);
            }
            other => panic!("expected Diagnostics, got {other:?}"),
        }
    }

    #[test]
    fn treats_a_uid_selected_requirement_as_not_yet_supported() {
        let dir = init_project();
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - uid: 01ARZ3NDEKTSV4RRFFQ69G5FAV
    label: Renamed
";
        let doc = parse_intent(yaml).unwrap();
        let err = build_plan(dir.path(), &doc).unwrap_err();
        assert!(matches!(err, PlanError::NotYetSupported(_)));
    }
}
