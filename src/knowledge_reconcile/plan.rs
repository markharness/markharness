//! Builds a plan for a Knowledge Intent that has already passed
//! [`super::validate::validate_static`] (ADR 0027 §7 Phase 3). Implements
//! the matching table (ADR 0027 §3) for Requirement and Feature: new,
//! unchanged (no UID, content matches), UID-selected update/rename, and
//! the fail-closed `ambiguous_identity`/`unknown_uid` diagnostics.
//!
//! Behavior/Scenario creation is supported only nested under a Feature (or
//! Behavior) that is itself brand-new in this same Intent: see
//! [`PlanError::NotYetSupported`] for the boundary this phase does not yet
//! cross. Adding a Behavior/Scenario to an *existing* Feature/Behavior
//! needs scope-aware existing-element matching (ADR 0027 §3's "Scenario以外
//! でscopeが矛盾する" row) that a later phase still has to add; nested
//! under a guaranteed-new parent, no such check is needed because the
//! parent's own directory cannot exist yet.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::identity::{EntityKind, knowledge_walk};
use crate::knowledge::{
    self, Behavior, Feature, Phase as KnowledgePhase, Requirement, RequirementSource, Scenario,
    StepItem as KnowledgeStepItem,
};

use super::diagnostics::{Diagnostic, DiagnosticCode};
use super::intent::{
    BehaviorIntent, FeatureIntent, IntentDocument, PhaseIntent, RequirementIntent, StepIntent,
};

#[derive(Debug)]
pub enum RequirementOutcome {
    New {
        uid: String,
        canonical: Requirement,
    },
    Unchanged {
        uid: String,
        id: String,
    },
    Updated {
        uid: String,
        before_id: String,
        canonical: Requirement,
        existing_path: PathBuf,
    },
}

impl RequirementOutcome {
    pub fn uid(&self) -> &str {
        match self {
            RequirementOutcome::New { uid, .. }
            | RequirementOutcome::Unchanged { uid, .. }
            | RequirementOutcome::Updated { uid, .. } => uid,
        }
    }
}

#[derive(Debug)]
pub struct NewBehavior {
    pub uid: String,
    pub canonical: Behavior,
    pub scenarios: Vec<NewScenario>,
}

#[derive(Debug)]
pub struct NewScenario {
    pub uid: String,
    pub canonical: Scenario,
}

#[derive(Debug)]
pub enum FeatureOutcome {
    New {
        uid: String,
        canonical: Feature,
        behaviors: Vec<NewBehavior>,
    },
    Unchanged {
        uid: String,
        id: String,
    },
    Updated {
        uid: String,
        before_id: String,
        canonical: Feature,
        existing_path: PathBuf,
    },
}

#[derive(Debug, Default)]
pub struct Plan {
    pub requirements: Vec<RequirementOutcome>,
    pub features: Vec<FeatureOutcome>,
}

/// Why [`build_plan`] could not produce a [`Plan`].
#[derive(Debug)]
pub enum PlanError {
    /// One or more elements failed a state-dependent check (ADR 0027 §7).
    Diagnostics(Vec<Diagnostic>),
    /// The Intent contains an element this phase does not yet resolve —
    /// see this module's own doc comment for the current boundary. Not one
    /// of §7's stable diagnostic codes: a later phase replaces this
    /// function's handling of these cases entirely, so this variant is not
    /// a contract callers should depend on.
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
    let mut requirement_outcomes = Vec::new();
    let mut requirement_uid_by_key: HashMap<&str, String> = HashMap::new();

    for (i, req) in doc.requirements.iter().enumerate() {
        if let Some(outcome) = plan_requirement(root, i, req, &mut diagnostics)? {
            if let Some(key) = &req.key {
                requirement_uid_by_key.insert(key.as_str(), outcome.uid().to_string());
            }
            requirement_outcomes.push(outcome);
        }
    }

    let mut feature_outcomes = Vec::new();
    for (i, feature) in doc.features.iter().enumerate() {
        if let Some(outcome) =
            plan_feature(root, i, feature, &requirement_uid_by_key, &mut diagnostics)?
        {
            feature_outcomes.push(outcome);
        }
    }

    if !diagnostics.is_empty() {
        return Err(PlanError::Diagnostics(diagnostics));
    }
    Ok(Plan {
        requirements: requirement_outcomes,
        features: feature_outcomes,
    })
}

fn parse_requirement_file(path: &Path) -> io::Result<Requirement> {
    let content = fs::read_to_string(path)?;
    knowledge::parse_requirement(&content).map_err(io::Error::other)
}

fn parse_feature_file(path: &Path) -> io::Result<Feature> {
    let content = fs::read_to_string(path)?;
    knowledge::parse_feature(&content).map_err(io::Error::other)
}

/// Builds a Requirement's full content using creation-time defaulting
/// (label defaults to id, omitted collections default empty). Used both
/// for a genuinely new Requirement and for comparing an existing, no-UID
/// match against "what a fresh creation from this Intent would produce"
/// (ADR 0027 §3's "正規化後の内容が完全一致する" unchanged row) — *not* for
/// a UID-selected patch, which keeps omitted fields at their current
/// value instead (see [`apply_requirement_patch`]).
fn build_requirement_content(
    location: &str,
    id: &str,
    uid: &str,
    intent: &RequirementIntent,
) -> Result<Requirement, Diagnostic> {
    let source = parse_source(location, intent.source.as_deref())?;
    Ok(Requirement {
        id: id.to_string(),
        source,
        label: Some(intent.label.clone().unwrap_or_else(|| id.to_string())),
        axis: intent.axis.clone().unwrap_or_default(),
        description: intent.description.clone(),
        source_locator: intent.source_locator.clone(),
        source_revision: None,
        related_issues: intent.related_issues.clone().unwrap_or_default(),
        uid: Some(uid.to_string()),
    })
}

/// Builds a UID-selected Requirement's patched content: a field present in
/// `intent` replaces the current value; an omitted field keeps it (ADR
/// 0027 §5). `axis`/`related_issues` are value collections — replaced
/// wholesale when present, kept wholesale when omitted.
/// `source`/`source_revision` are resolved by the caller ([`resolve_source_revision`]
/// needs filesystem/Git access this function does not have) — every other
/// field follows ordinary patch semantics (ADR 0027 §5: present replaces,
/// omitted keeps current).
fn apply_requirement_patch(
    current: &Requirement,
    intent: &RequirementIntent,
    source: RequirementSource,
    source_revision: Option<String>,
) -> Requirement {
    Requirement {
        id: intent.id.clone().unwrap_or_else(|| current.id.clone()),
        source,
        label: intent.label.clone().or_else(|| current.label.clone()),
        axis: intent.axis.clone().unwrap_or_else(|| current.axis.clone()),
        description: intent
            .description
            .clone()
            .or_else(|| current.description.clone()),
        source_locator: intent
            .source_locator
            .clone()
            .or_else(|| current.source_locator.clone()),
        source_revision,
        related_issues: intent
            .related_issues
            .clone()
            .unwrap_or_else(|| current.related_issues.clone()),
        uid: current.uid.clone(),
    }
}

/// Resolves a UID-selected Requirement's `source_revision` (ADR 0027 §5).
/// `current` is never persisted as-is: it is an Intent-only instruction to
/// advance the pin to whatever blob OID `source_locator` resolves to right
/// now, mirroring the existing standalone `requirement repin` command's
/// `blob OID` validation (same locator-exists and `git hash-object`
/// checks). Omitting the field keeps the current pin; any value other
/// than the literal `current` is rejected — this Intent field is not a
/// place to write an arbitrary OID directly.
fn resolve_source_revision(
    root: &Path,
    location: &str,
    current: &Requirement,
    effective_source: RequirementSource,
    intent_source_revision: &Option<String>,
) -> io::Result<Result<Option<String>, Diagnostic>> {
    match intent_source_revision.as_deref() {
        None => Ok(Ok(current.source_revision.clone())),
        Some("current") => {
            if effective_source != RequirementSource::External {
                return Ok(Err(Diagnostic::new(
                    DiagnosticCode::InvalidSourceRevision,
                    location,
                    "source_revision: current applies only to source: external Requirements",
                )));
            }
            let Some(locator) = current.source_locator.clone() else {
                return Ok(Err(Diagnostic::new(
                    DiagnosticCode::InvalidSourceRevision,
                    location,
                    "source: external Requirement has no source_locator to resolve",
                )));
            };
            if !root.join(&locator).is_file() {
                return Ok(Err(Diagnostic::new(
                    DiagnosticCode::InvalidSourceRevision,
                    location,
                    format!("locator '{locator}' does not exist in the working tree"),
                )));
            }
            match crate::git::hash_object(root, &locator) {
                Ok(oid) => Ok(Ok(Some(oid))),
                Err(e) => Ok(Err(Diagnostic::new(
                    DiagnosticCode::InvalidSourceRevision,
                    location,
                    format!("failed to resolve the current blob OID for '{locator}': {e}"),
                ))),
            }
        }
        Some(other) => Ok(Err(Diagnostic::new(
            DiagnosticCode::InvalidSourceRevision,
            location,
            format!(
                "source_revision must be 'current' (an Intent-only instruction), got '{other}'"
            ),
        ))),
    }
}

fn parse_source(location: &str, source: Option<&str>) -> Result<RequirementSource, Diagnostic> {
    match source {
        Some("native") => Ok(RequirementSource::Native),
        Some("external") => Ok(RequirementSource::External),
        _ => Err(Diagnostic::new(
            DiagnosticCode::MissingRequiredField,
            format!("{location}.source"),
            "source must be 'native' or 'external'",
        )),
    }
}

fn plan_requirement(
    root: &Path,
    i: usize,
    req: &RequirementIntent,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Option<RequirementOutcome>, PlanError> {
    let location = format!("requirements[{i}]");

    if let Some(uid) = &req.uid {
        let Some(found) = knowledge_walk::find_by_uid(root, EntityKind::Requirement, uid)? else {
            diagnostics.push(Diagnostic::new(
                DiagnosticCode::UnknownUid,
                location,
                format!("no Requirement with uid '{uid}' exists"),
            ));
            return Ok(None);
        };
        let current = parse_requirement_file(&found.path)?;
        let effective_source = match &req.source {
            Some(_) => match parse_source(&location, req.source.as_deref()) {
                Ok(s) => s,
                Err(d) => {
                    diagnostics.push(d);
                    return Ok(None);
                }
            },
            None => current.source,
        };
        let source_revision = match resolve_source_revision(
            root,
            &location,
            &current,
            effective_source,
            &req.source_revision,
        )? {
            Ok(v) => v,
            Err(d) => {
                diagnostics.push(d);
                return Ok(None);
            }
        };
        let candidate = apply_requirement_patch(&current, req, effective_source, source_revision);
        if candidate == current {
            return Ok(Some(RequirementOutcome::Unchanged {
                uid: uid.clone(),
                id: current.id,
            }));
        }
        return Ok(Some(RequirementOutcome::Updated {
            uid: uid.clone(),
            before_id: current.id.clone(),
            canonical: candidate,
            existing_path: found.path,
        }));
    }

    let Some(id) = &req.id else {
        diagnostics.push(Diagnostic::new(
            DiagnosticCode::MissingRequiredField,
            format!("{location}.id"),
            "id is required",
        ));
        return Ok(None);
    };

    match knowledge_walk::find_by_id(root, EntityKind::Requirement, id)? {
        None => {
            let target_path = super::paths::requirement_path(root, id);
            if target_path.is_file() {
                // No entity currently has this `id` (checked above), yet a
                // file already sits at the path a fresh creation would use.
                // The only way that happens is a rename that changed a
                // file's `id:` without moving its directory (this module's
                // own `execute::commit_plan` does exactly that, matching
                // `feature_ops::write_id_and_uid`'s existing precedent):
                // the old id's directory is still physically there. Writing
                // a new Requirement here would silently overwrite that
                // renamed entity's file — fail closed instead.
                diagnostics.push(Diagnostic::new(
                    DiagnosticCode::ConflictingExistingValue,
                    location,
                    format!(
                        "a file already exists at the path id '{id}' would use ({}), likely a renamed Requirement whose directory was not moved; choose a different id",
                        target_path.display()
                    ),
                ));
                return Ok(None);
            }
            let uid = ulid::Ulid::new().to_string();
            match build_requirement_content(&location, id, &uid, req) {
                Ok(canonical) => Ok(Some(RequirementOutcome::New { uid, canonical })),
                Err(d) => {
                    diagnostics.push(d);
                    Ok(None)
                }
            }
        }
        Some(found) => {
            let current = parse_requirement_file(&found.path)?;
            let Some(existing_uid) = current.uid.clone() else {
                diagnostics.push(Diagnostic::new(
                    DiagnosticCode::InvariantViolation,
                    location,
                    format!("existing Requirement '{id}' has no uid; run identity migrate first"),
                ));
                return Ok(None);
            };
            let candidate = match build_requirement_content(&location, id, &existing_uid, req) {
                Ok(c) => c,
                Err(d) => {
                    diagnostics.push(d);
                    return Ok(None);
                }
            };
            if candidate == current {
                Ok(Some(RequirementOutcome::Unchanged {
                    uid: existing_uid,
                    id: id.clone(),
                }))
            } else {
                diagnostics.push(Diagnostic::new(
                    DiagnosticCode::AmbiguousIdentity,
                    location,
                    format!(
                        "a Requirement with id '{id}' already exists with different content; supply its UID to patch or rename it"
                    ),
                ));
                Ok(None)
            }
        }
    }
}

fn resolve_contributes_to(
    root: &Path,
    location_prefix: &str,
    contributes_to: &[String],
    requirement_uid_by_key: &HashMap<&str, String>,
    diagnostics: &mut Vec<Diagnostic>,
) -> io::Result<Vec<String>> {
    let mut resolved = Vec::new();
    for (j, reference) in contributes_to.iter().enumerate() {
        if let Some(uid) = requirement_uid_by_key.get(reference.as_str()) {
            resolved.push(uid.clone());
            continue;
        }
        match knowledge_walk::find_by_uid(root, EntityKind::Requirement, reference)? {
            Some(_) => resolved.push(reference.clone()),
            None => diagnostics.push(Diagnostic::new(
                DiagnosticCode::UnknownUid,
                format!("{location_prefix}[{j}]"),
                format!("no Requirement with uid '{reference}' exists"),
            )),
        }
    }
    Ok(resolved)
}

fn build_feature_content(
    id: &str,
    uid: &str,
    requirement_uids: Vec<String>,
    intent: &FeatureIntent,
) -> Feature {
    Feature {
        id: id.to_string(),
        requirement_uids,
        label: intent.label.clone().unwrap_or_else(|| id.to_string()),
        axis: intent.axis.clone().unwrap_or_default(),
        description: intent.description.clone(),
        forked_from: None,
        uid: Some(uid.to_string()),
    }
}

fn apply_feature_patch(
    current: &Feature,
    intent: &FeatureIntent,
    resolved_requirement_uids: Option<Vec<String>>,
) -> Feature {
    Feature {
        id: intent.id.clone().unwrap_or_else(|| current.id.clone()),
        requirement_uids: resolved_requirement_uids
            .unwrap_or_else(|| current.requirement_uids.clone()),
        label: intent
            .label
            .clone()
            .unwrap_or_else(|| current.label.clone()),
        axis: intent.axis.clone().unwrap_or_else(|| current.axis.clone()),
        description: intent
            .description
            .clone()
            .or_else(|| current.description.clone()),
        forked_from: current.forked_from.clone(),
        uid: current.uid.clone(),
    }
}

fn plan_feature(
    root: &Path,
    i: usize,
    feature: &FeatureIntent,
    requirement_uid_by_key: &HashMap<&str, String>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Option<FeatureOutcome>, PlanError> {
    let location = format!("features[{i}]");

    if let Some(uid) = &feature.uid {
        if !feature.behaviors.is_empty() {
            return Err(PlanError::NotYetSupported(format!(
                "{location}.behaviors: adding Behaviors to an existing Feature is not supported yet"
            )));
        }
        let Some(found) = knowledge_walk::find_by_uid(root, EntityKind::Feature, uid)? else {
            diagnostics.push(Diagnostic::new(
                DiagnosticCode::UnknownUid,
                location,
                format!("no Feature with uid '{uid}' exists"),
            ));
            return Ok(None);
        };
        let current = parse_feature_file(&found.path)?;
        let resolved_requirement_uids = match &feature.contributes_to {
            Some(refs) => Some(resolve_contributes_to(
                root,
                &format!("{location}.contributes_to"),
                refs,
                requirement_uid_by_key,
                diagnostics,
            )?),
            None => None,
        };
        let candidate = apply_feature_patch(&current, feature, resolved_requirement_uids);
        if candidate == current {
            return Ok(Some(FeatureOutcome::Unchanged {
                uid: uid.clone(),
                id: current.id,
            }));
        }
        return Ok(Some(FeatureOutcome::Updated {
            uid: uid.clone(),
            before_id: current.id.clone(),
            canonical: candidate,
            existing_path: found.path,
        }));
    }

    let Some(id) = &feature.id else {
        diagnostics.push(Diagnostic::new(
            DiagnosticCode::MissingRequiredField,
            format!("{location}.id"),
            "id is required",
        ));
        return Ok(None);
    };

    match knowledge_walk::find_by_id(root, EntityKind::Feature, id)? {
        None => {
            let target_path = super::paths::feature_path(root, id);
            if target_path.is_file() {
                // See the identical check in `plan_requirement`: a renamed
                // Feature's directory is not moved, so its old id's path
                // can still exist even though no Feature currently has
                // that id. Fail closed instead of silently overwriting it.
                diagnostics.push(Diagnostic::new(
                    DiagnosticCode::ConflictingExistingValue,
                    location,
                    format!(
                        "a file already exists at the path id '{id}' would use ({}), likely a renamed Feature whose directory was not moved; choose a different id",
                        target_path.display()
                    ),
                ));
                return Ok(None);
            }
            let uid = ulid::Ulid::new().to_string();
            let requirement_uids = match &feature.contributes_to {
                Some(refs) => resolve_contributes_to(
                    root,
                    &format!("{location}.contributes_to"),
                    refs,
                    requirement_uid_by_key,
                    diagnostics,
                )?,
                None => Vec::new(),
            };
            let canonical = build_feature_content(id, &uid, requirement_uids, feature);
            let behaviors = plan_new_behaviors(&location, id, &feature.behaviors, diagnostics);
            Ok(Some(FeatureOutcome::New {
                uid,
                canonical,
                behaviors,
            }))
        }
        Some(found) => {
            if !feature.behaviors.is_empty() {
                return Err(PlanError::NotYetSupported(format!(
                    "{location}.behaviors: adding Behaviors to an existing Feature is not supported yet"
                )));
            }
            let current = parse_feature_file(&found.path)?;
            let Some(existing_uid) = current.uid.clone() else {
                diagnostics.push(Diagnostic::new(
                    DiagnosticCode::InvariantViolation,
                    location,
                    format!("existing Feature '{id}' has no uid; run identity migrate first"),
                ));
                return Ok(None);
            };
            let requirement_uids = match &feature.contributes_to {
                Some(refs) => resolve_contributes_to(
                    root,
                    &format!("{location}.contributes_to"),
                    refs,
                    requirement_uid_by_key,
                    diagnostics,
                )?,
                None => Vec::new(),
            };
            let candidate = build_feature_content(id, &existing_uid, requirement_uids, feature);
            if candidate == current {
                Ok(Some(FeatureOutcome::Unchanged {
                    uid: existing_uid,
                    id: id.clone(),
                }))
            } else {
                diagnostics.push(Diagnostic::new(
                    DiagnosticCode::AmbiguousIdentity,
                    location,
                    format!(
                        "a Feature with id '{id}' already exists with different content; supply its UID to patch or rename it"
                    ),
                ));
                Ok(None)
            }
        }
    }
}

/// Plans every Behavior (and nested Scenario) under a Feature guaranteed
/// to be brand-new in this same Intent: since the Feature's own directory
/// cannot exist yet, no existing-element lookup is needed — every entry
/// simply gets a fresh UID. See this module's doc comment for why this is
/// restricted to a new parent.
fn plan_new_behaviors(
    feature_location: &str,
    feature_id: &str,
    behaviors: &[BehaviorIntent],
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<NewBehavior> {
    let mut planned = Vec::new();
    for (j, behavior) in behaviors.iter().enumerate() {
        let location = format!("{feature_location}.behaviors[{j}]");
        let Some(id) = &behavior.id else {
            diagnostics.push(Diagnostic::new(
                DiagnosticCode::MissingRequiredField,
                format!("{location}.id"),
                "id is required",
            ));
            continue;
        };
        let Some(description) = &behavior.description else {
            diagnostics.push(Diagnostic::new(
                DiagnosticCode::MissingRequiredField,
                format!("{location}.description"),
                "description is required",
            ));
            continue;
        };
        let uid = ulid::Ulid::new().to_string();
        let canonical = Behavior {
            id: id.clone(),
            feature: feature_id.to_string(),
            label: behavior.label.clone().unwrap_or_else(|| id.clone()),
            axis: behavior.axis.clone().unwrap_or_default(),
            description: description.clone(),
            procedures: std::collections::BTreeMap::new(),
            uid: Some(uid.clone()),
        };
        let scenarios = plan_new_scenarios(&location, id, &behavior.scenarios, diagnostics);
        planned.push(NewBehavior {
            uid,
            canonical,
            scenarios,
        });
    }
    planned
}

fn plan_new_scenarios(
    behavior_location: &str,
    behavior_id: &str,
    scenarios: &[super::intent::ScenarioIntent],
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<NewScenario> {
    let mut planned = Vec::new();
    for (k, scenario) in scenarios.iter().enumerate() {
        let location = format!("{behavior_location}.scenarios[{k}]");
        let Some(id) = &scenario.id else {
            diagnostics.push(Diagnostic::new(
                DiagnosticCode::MissingRequiredField,
                format!("{location}.id"),
                "id is required",
            ));
            continue;
        };
        let Some(description) = &scenario.description else {
            diagnostics.push(Diagnostic::new(
                DiagnosticCode::MissingRequiredField,
                format!("{location}.description"),
                "description is required",
            ));
            continue;
        };
        let phases = scenario.phases.clone().unwrap_or_default();
        if phases.is_empty() {
            diagnostics.push(Diagnostic::new(
                DiagnosticCode::MissingRequiredField,
                format!("{location}.phases"),
                "at least one phase is required",
            ));
            continue;
        }
        let uid = ulid::Ulid::new().to_string();
        let canonical = Scenario {
            id: id.clone(),
            behavior: behavior_id.to_string(),
            label: scenario.label.clone().unwrap_or_else(|| id.clone()),
            description: description.clone(),
            phases: phases.into_iter().map(convert_phase).collect(),
            implementation_note: None,
            generated_by: None,
            verified_by: None,
            uid: Some(uid.clone()),
        };
        planned.push(NewScenario { uid, canonical });
    }
    planned
}

fn convert_phase(phase: PhaseIntent) -> KnowledgePhase {
    KnowledgePhase {
        steps: phase.steps.into_iter().map(convert_step).collect(),
        results: phase.results,
    }
}

fn convert_step(step: StepIntent) -> KnowledgeStepItem {
    match step {
        StepIntent::Action { action } => KnowledgeStepItem::Action { action },
        StepIntent::Use { procedure } => KnowledgeStepItem::Use { procedure },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge_reconcile::intent::parse_intent;

    fn init_project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".markharness/knowledge")).unwrap();
        dir
    }

    fn write_requirement(dir: &Path, id: &str, uid: &str, label: &str) {
        fs::create_dir_all(dir.join(".markharness/knowledge/requirements").join(id)).unwrap();
        fs::write(
            dir.join(".markharness/knowledge/requirements")
                .join(id)
                .join("requirement.yml"),
            format!("id: {id}\nsource: native\nlabel: {label}\naxis: []\nuid: {uid}\n"),
        )
        .unwrap();
    }

    fn init_git_repo(dir: &Path) {
        let status = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(args)
                .status()
                .unwrap()
        };
        assert!(status(&["init", "-q"]).success());
        assert!(status(&["config", "user.email", "test@example.com"]).success());
        assert!(status(&["config", "user.name", "Test"]).success());
        assert!(status(&["config", "core.autocrlf", "false"]).success());
    }

    fn write_external_requirement(dir: &Path, id: &str, uid: &str, locator: &str, revision: &str) {
        fs::create_dir_all(dir.join(".markharness/knowledge/requirements").join(id)).unwrap();
        fs::write(
            dir.join(".markharness/knowledge/requirements")
                .join(id)
                .join("requirement.yml"),
            format!(
                "id: {id}\nsource: external\naxis: []\nsource_locator: {locator}\nsource_revision: {revision}\nuid: {uid}\n"
            ),
        )
        .unwrap();
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

        assert_eq!(plan.requirements.len(), 1);
        assert_eq!(plan.features.len(), 1);
        let RequirementOutcome::New {
            uid: requirement_uid,
            ..
        } = &plan.requirements[0]
        else {
            panic!("expected New, got {:?}", plan.requirements[0]);
        };
        let FeatureOutcome::New { canonical, .. } = &plan.features[0] else {
            panic!("expected New, got {:?}", plan.features[0]);
        };
        assert_eq!(&canonical.requirement_uids, &vec![requirement_uid.clone()]);
    }

    #[test]
    fn rerunning_the_same_intent_reports_unchanged_instead_of_ambiguous() {
        let dir = init_project();
        write_requirement(
            dir.path(),
            "todo",
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "TODO management",
        );
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
        let plan = build_plan(dir.path(), &doc).unwrap();
        assert_eq!(plan.requirements.len(), 1);
        match &plan.requirements[0] {
            RequirementOutcome::Unchanged { uid, id } => {
                assert_eq!(uid, "01ARZ3NDEKTSV4RRFFQ69G5FAV");
                assert_eq!(id, "todo");
            }
            other => panic!("expected Unchanged, got {other:?}"),
        }
    }

    #[test]
    fn reports_ambiguous_identity_when_a_requirement_id_already_exists_with_different_content() {
        let dir = init_project();
        write_requirement(
            dir.path(),
            "todo",
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "Old label",
        );
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - id: todo
    source: native
    label: New label
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
        write_requirement(dir.path(), "todo", "01ARZ3NDEKTSV4RRFFQ69G5FAV", "todo");
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
        let FeatureOutcome::New { canonical, .. } = &plan.features[0] else {
            panic!("expected New");
        };
        assert_eq!(
            canonical.requirement_uids,
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
    fn uid_selected_requirement_patches_label_and_keeps_omitted_fields() {
        let dir = init_project();
        write_requirement(
            dir.path(),
            "todo",
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "Old label",
        );
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - uid: 01ARZ3NDEKTSV4RRFFQ69G5FAV
    label: New label
";
        let doc = parse_intent(yaml).unwrap();
        let plan = build_plan(dir.path(), &doc).unwrap();
        match &plan.requirements[0] {
            RequirementOutcome::Updated {
                canonical,
                before_id,
                ..
            } => {
                assert_eq!(before_id, "todo");
                assert_eq!(canonical.id, "todo");
                assert_eq!(canonical.label, Some("New label".to_string()));
                assert_eq!(canonical.source, RequirementSource::Native);
            }
            other => panic!("expected Updated, got {other:?}"),
        }
    }

    #[test]
    fn uid_selected_requirement_with_no_changes_reports_unchanged() {
        let dir = init_project();
        write_requirement(
            dir.path(),
            "todo",
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "Same label",
        );
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - uid: 01ARZ3NDEKTSV4RRFFQ69G5FAV
    label: Same label
";
        let doc = parse_intent(yaml).unwrap();
        let plan = build_plan(dir.path(), &doc).unwrap();
        assert!(matches!(
            &plan.requirements[0],
            RequirementOutcome::Unchanged { .. }
        ));
    }

    #[test]
    fn uid_selected_requirement_rename_changes_id() {
        let dir = init_project();
        write_requirement(
            dir.path(),
            "todo",
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "TODO management",
        );
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - uid: 01ARZ3NDEKTSV4RRFFQ69G5FAV
    id: task
";
        let doc = parse_intent(yaml).unwrap();
        let plan = build_plan(dir.path(), &doc).unwrap();
        match &plan.requirements[0] {
            RequirementOutcome::Updated {
                canonical,
                before_id,
                ..
            } => {
                assert_eq!(before_id, "todo");
                assert_eq!(canonical.id, "task");
                assert_eq!(canonical.label, Some("TODO management".to_string()));
            }
            other => panic!("expected Updated, got {other:?}"),
        }
    }

    #[test]
    fn unknown_uid_on_a_requirement_reports_unknown_uid() {
        let dir = init_project();
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - uid: 01ARZ3NDEKTSV4RRFFQ69G5FAV
    label: New label
";
        let doc = parse_intent(yaml).unwrap();
        let err = build_plan(dir.path(), &doc).unwrap_err();
        match err {
            PlanError::Diagnostics(diagnostics) => {
                assert_eq!(diagnostics[0].code, DiagnosticCode::UnknownUid);
            }
            other => panic!("expected Diagnostics, got {other:?}"),
        }
    }

    /// Regression test for the reviewer-flagged corruption risk: a rename
    /// (via UID) changes a Requirement's `id:` field but does not move its
    /// directory (this module's own `execute::commit_plan`, matching
    /// `feature_ops::write_id_and_uid`'s existing precedent). The old id's
    /// path (`requirements/todo/requirement.yml` here) can therefore still
    /// physically exist even though `find_by_id("todo")` now correctly
    /// finds nothing — reusing "todo" for a brand-new Requirement must not
    /// silently overwrite that renamed entity's file at the collided path.
    #[test]
    fn reusing_a_renamed_away_id_reports_conflicting_existing_value_instead_of_overwriting() {
        let dir = init_project();
        // Simulates the on-disk state right after renaming a Requirement
        // from id "todo" to id "task" via UID patch: the file stays at the
        // "todo" directory, but its own `id:` field now says "task".
        write_requirement(
            dir.path(),
            "todo",
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "TODO management",
        );
        let content = fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/requirements/todo/requirement.yml"),
        )
        .unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/requirements/todo/requirement.yml"),
            content.replace("id: todo", "id: task"),
        )
        .unwrap();
        assert!(
            knowledge_walk::find_by_id(dir.path(), EntityKind::Requirement, "todo")
                .unwrap()
                .is_none(),
            "no Requirement should currently have id 'todo'"
        );

        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - id: todo
    source: native
    label: A brand new TODO requirement
    axis: []
";
        let doc = parse_intent(yaml).unwrap();
        let err = build_plan(dir.path(), &doc).unwrap_err();
        match err {
            PlanError::Diagnostics(diagnostics) => {
                assert_eq!(diagnostics.len(), 1);
                assert_eq!(
                    diagnostics[0].code,
                    DiagnosticCode::ConflictingExistingValue
                );
            }
            other => panic!("expected Diagnostics, got {other:?}"),
        }

        // The renamed entity's file must be untouched by the failed attempt.
        let content = fs::read_to_string(
            dir.path()
                .join(".markharness/knowledge/requirements/todo/requirement.yml"),
        )
        .unwrap();
        assert!(content.contains("id: task"));
        assert!(content.contains("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
    }

    #[test]
    fn plans_new_behaviors_and_scenarios_nested_under_a_new_feature() {
        let dir = init_project();
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

features:
  - id: todo-management
    label: TODO management
    axis: []
    behaviors:
      - id: add-todo
        description: Add a TODO
        scenarios:
          - id: empty-title
            description: An empty title cannot be added
            phases:
              - steps:
                  - action: Attempt to add an empty title
                results:
                  - No TODO is added
";
        let doc = parse_intent(yaml).unwrap();
        let plan = build_plan(dir.path(), &doc).unwrap();
        let FeatureOutcome::New { behaviors, .. } = &plan.features[0] else {
            panic!("expected New");
        };
        assert_eq!(behaviors.len(), 1);
        assert_eq!(behaviors[0].canonical.id, "add-todo");
        assert_eq!(behaviors[0].canonical.feature, "todo-management");
        assert_eq!(behaviors[0].scenarios.len(), 1);
        assert_eq!(behaviors[0].scenarios[0].canonical.id, "empty-title");
        assert_eq!(behaviors[0].scenarios[0].canonical.behavior, "add-todo");
    }

    #[test]
    fn treats_adding_behaviors_to_an_existing_feature_as_not_yet_supported() {
        let dir = init_project();
        fs::create_dir_all(
            dir.path()
                .join(".markharness/knowledge/features/todo-management"),
        )
        .unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/features/todo-management/feature.yml"),
            "id: todo-management\nrequirement_uids: []\nlabel: TODO management\naxis: []\nuid: 01ARZ3NDEKTSV4RRFFQ69G5FAV\n",
        )
        .unwrap();
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

features:
  - id: todo-management
    label: TODO management
    axis: []
    behaviors:
      - id: add-todo
        description: Add a TODO
";
        let doc = parse_intent(yaml).unwrap();
        let err = build_plan(dir.path(), &doc).unwrap_err();
        assert!(matches!(err, PlanError::NotYetSupported(_)));
    }

    #[test]
    fn source_revision_current_repins_an_external_requirement_to_its_blob_oid() {
        let dir = init_project();
        init_git_repo(dir.path());
        fs::write(dir.path().join("requirements.sdoc"), "original text\n").unwrap();
        write_external_requirement(
            dir.path(),
            "controls",
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "requirements.sdoc",
            "0000000000000000000000000000000000000000",
        );
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - uid: 01ARZ3NDEKTSV4RRFFQ69G5FAV
    source_revision: current
";
        let doc = parse_intent(yaml).unwrap();
        let plan = build_plan(dir.path(), &doc).unwrap();
        match &plan.requirements[0] {
            RequirementOutcome::Updated { canonical, .. } => {
                let expected = crate::git::hash_object(dir.path(), "requirements.sdoc").unwrap();
                assert_eq!(canonical.source_revision, Some(expected));
                assert_ne!(
                    canonical.source_revision,
                    Some("0000000000000000000000000000000000000000".to_string())
                );
            }
            other => panic!("expected Updated, got {other:?}"),
        }
    }

    #[test]
    fn source_revision_current_is_rejected_for_a_native_requirement() {
        let dir = init_project();
        write_requirement(
            dir.path(),
            "todo",
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "TODO management",
        );
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - uid: 01ARZ3NDEKTSV4RRFFQ69G5FAV
    source_revision: current
";
        let doc = parse_intent(yaml).unwrap();
        let err = build_plan(dir.path(), &doc).unwrap_err();
        match err {
            PlanError::Diagnostics(diagnostics) => {
                assert_eq!(diagnostics[0].code, DiagnosticCode::InvalidSourceRevision);
            }
            other => panic!("expected Diagnostics, got {other:?}"),
        }
    }

    #[test]
    fn source_revision_current_is_rejected_when_the_locator_is_missing() {
        let dir = init_project();
        init_git_repo(dir.path());
        write_external_requirement(
            dir.path(),
            "controls",
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "does-not-exist.sdoc",
            "0000000000000000000000000000000000000000",
        );
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - uid: 01ARZ3NDEKTSV4RRFFQ69G5FAV
    source_revision: current
";
        let doc = parse_intent(yaml).unwrap();
        let err = build_plan(dir.path(), &doc).unwrap_err();
        match err {
            PlanError::Diagnostics(diagnostics) => {
                assert_eq!(diagnostics[0].code, DiagnosticCode::InvalidSourceRevision);
            }
            other => panic!("expected Diagnostics, got {other:?}"),
        }
    }

    #[test]
    fn source_revision_other_than_current_is_rejected() {
        let dir = init_project();
        init_git_repo(dir.path());
        fs::write(dir.path().join("requirements.sdoc"), "text\n").unwrap();
        write_external_requirement(
            dir.path(),
            "controls",
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "requirements.sdoc",
            "0000000000000000000000000000000000000000",
        );
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - uid: 01ARZ3NDEKTSV4RRFFQ69G5FAV
    source_revision: deadbeef00000000000000000000000000000000
";
        let doc = parse_intent(yaml).unwrap();
        let err = build_plan(dir.path(), &doc).unwrap_err();
        match err {
            PlanError::Diagnostics(diagnostics) => {
                assert_eq!(diagnostics[0].code, DiagnosticCode::InvalidSourceRevision);
            }
            other => panic!("expected Diagnostics, got {other:?}"),
        }
    }

    #[test]
    fn omitting_source_revision_keeps_the_current_pin() {
        let dir = init_project();
        init_git_repo(dir.path());
        fs::write(dir.path().join("requirements.sdoc"), "text\n").unwrap();
        write_external_requirement(
            dir.path(),
            "controls",
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "requirements.sdoc",
            "0000000000000000000000000000000000000000",
        );
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - uid: 01ARZ3NDEKTSV4RRFFQ69G5FAV
    label: unrelated patch
";
        let doc = parse_intent(yaml).unwrap();
        let plan = build_plan(dir.path(), &doc).unwrap();
        match &plan.requirements[0] {
            RequirementOutcome::Updated { canonical, .. } => {
                assert_eq!(
                    canonical.source_revision,
                    Some("0000000000000000000000000000000000000000".to_string())
                );
            }
            other => panic!("expected Updated, got {other:?}"),
        }
    }
}
