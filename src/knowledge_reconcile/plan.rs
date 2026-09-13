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

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
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
        path: PathBuf,
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

/// An existing Scenario reached by UID while walking a Feature's
/// `behaviors` (ADR 0027 §3): content-only update, or an explicit
/// reparent to a different Behavior — both keep the Scenario's UID and
/// are represented identically here, since ADR 0027 §3 handles them with
/// the same rule ("effective contentに応じてCase revisionを再計算する").
/// A Case's revision itself is never stored — `identity::derived_uid::
/// case_revision` derives it purely from `Scenario.phases` whenever a
/// later command reads it, so this module has nothing further to persist
/// for that part of the ADR requirement.
#[derive(Debug)]
pub enum ScenarioOutcome {
    Unchanged {
        uid: String,
        id: String,
        path: PathBuf,
    },
    Updated {
        uid: String,
        /// The `id` before this Intent. Differs from `canonical.id` only
        /// for a rename, which additionally needs an
        /// `IdentityMutation::Renamed` event (ADR 0027 §3).
        before_id: String,
        canonical: Box<Scenario>,
        existing_path: PathBuf,
        new_path: PathBuf,
    },
}

/// A child element rewritten solely because its parent's display id
/// changed: a Behavior stores its Feature's id in `feature:`, a Scenario
/// its Behavior's in `behavior:`. Renaming a parent without rewriting
/// these leaves the children pointing at an id nothing has any more, which
/// later scope checks read as a mismatch. Not selected by the Intent, but
/// reported as updated because the file really did change.
#[derive(Debug)]
pub enum BackReferenceFixup {
    Behavior {
        uid: Option<String>,
        canonical: Box<Behavior>,
        path: PathBuf,
    },
    Scenario {
        uid: Option<String>,
        canonical: Box<Scenario>,
        path: PathBuf,
    },
}

/// An existing Behavior reached by UID while walking a UID-selected
/// Feature's `behaviors` (ADR 0027 §5): the fields the Intent names
/// replace the current ones, omitted fields keep them. `id` is not
/// patchable here — see [`apply_behavior_patch`].
#[derive(Debug)]
pub enum BehaviorOutcome {
    Unchanged {
        uid: String,
        id: String,
        path: PathBuf,
    },
    Updated {
        uid: String,
        /// See [`ScenarioOutcome::Updated::before_id`].
        before_id: String,
        canonical: Box<Behavior>,
        existing_path: PathBuf,
    },
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
        path: PathBuf,
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
    /// Existing Scenarios reached by UID while walking any Feature's
    /// `behaviors` — content updates and/or reparents (see
    /// [`ScenarioOutcome`]). Kept separate from `features` because these
    /// are not new elements of the Feature/Behavior tree being built, but
    /// edits to Scenarios that already exist elsewhere in the tree.
    pub scenario_updates: Vec<ScenarioOutcome>,
    /// Existing Behaviors reached by UID under a UID-selected Feature,
    /// patched in place (see [`BehaviorOutcome`]). Separate from
    /// `features` for the same reason as `scenario_updates`.
    pub behavior_updates: Vec<BehaviorOutcome>,
    /// Children the Intent never named, rewritten only to follow a renamed
    /// parent (see [`BackReferenceFixup`]).
    pub back_reference_fixups: Vec<BackReferenceFixup>,
    /// A snapshot of `.markharness/knowledge`'s on-disk content taken as
    /// [`build_plan`] started reading it (ADR 0027 §6's `stale_plan`).
    /// `None` for a `Plan` no caller built with [`build_plan`] (e.g.
    /// `Plan::default()` in tests) — there is nothing to compare staleness
    /// against, so callers must skip the check rather than treat `None` as
    /// "unchanged".
    pub state_fingerprint: Option<String>,
}

/// Hashes every file under `.markharness/knowledge` (relative path +
/// content) into one opaque token. Two calls returning the same token mean
/// nothing under that tree changed between them — the basis for detecting
/// a [`Plan`] gone stale (ADR 0027 §6) between when [`build_plan`] read
/// current state and when a caller later commits it.
pub(crate) fn state_fingerprint(root: &Path) -> io::Result<String> {
    let knowledge_root = root.join(".markharness/knowledge");
    let mut entries: Vec<(String, String)> = Vec::new();
    collect_knowledge_files(&knowledge_root, &knowledge_root, &mut entries)?;
    entries.sort();
    let mut hasher = DefaultHasher::new();
    entries.hash(&mut hasher);
    Ok(format!("{:016x}", hasher.finish()))
}

fn collect_knowledge_files(
    root: &Path,
    dir: &Path,
    out: &mut Vec<(String, String)>,
) -> io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_knowledge_files(root, &path, out)?;
        } else {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.push((relative, fs::read_to_string(&path)?));
        }
    }
    Ok(())
}

/// What a Feature's walk contributes to the [`Plan`] besides its own
/// [`FeatureOutcome`]: edits to elements that already exist elsewhere in
/// the tree. Bundled so the walk's own signature stays about the Feature
/// rather than about its five separate output lists.
#[derive(Debug, Default)]
struct ExistingElementEdits {
    scenarios: Vec<ScenarioOutcome>,
    behaviors: Vec<BehaviorOutcome>,
    back_reference_fixups: Vec<BackReferenceFixup>,
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
    let fingerprint = state_fingerprint(root)?;
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
    let mut edits = ExistingElementEdits::default();
    for (i, feature) in doc.features.iter().enumerate() {
        if let Some(outcome) = plan_feature(
            root,
            i,
            feature,
            &requirement_uid_by_key,
            &mut diagnostics,
            &mut edits,
        )? {
            feature_outcomes.push(outcome);
        }
    }

    if !diagnostics.is_empty() {
        return Err(PlanError::Diagnostics(diagnostics));
    }
    Ok(Plan {
        requirements: requirement_outcomes,
        features: feature_outcomes,
        scenario_updates: edits.scenarios,
        behavior_updates: edits.behaviors,
        back_reference_fixups: edits.back_reference_fixups,
        state_fingerprint: Some(fingerprint),
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

/// Normalizes an Intent-supplied `description` into the exact form the
/// canonical serializer round-trips to. Every `serialize_*` writes
/// `description` as a literal block scalar, which YAML clips to end in
/// exactly one newline, so a description read back from disk always ends
/// in one — while a description written inline in an Intent does not.
/// Comparing the two forms directly would mean an Intent that sets a
/// description never settles: a re-run would find its own value different
/// from the stored one and report a change forever, contradicting ADR
/// 0027 §3's unchanged rule and §6's "same state and Intent produce the
/// same plan".
fn canonical_description(text: &str) -> String {
    let normalized = text.replace("\r\n", "\n");
    let trimmed = normalized.trim_end_matches('\n');
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}\n")
    }
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
        description: intent.description.as_deref().map(canonical_description),
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
            .as_deref()
            .map(canonical_description)
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
                path: found.path,
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
                    path: found.path,
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
        description: intent.description.as_deref().map(canonical_description),
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
            .as_deref()
            .map(canonical_description)
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
    edits: &mut ExistingElementEdits,
) -> Result<Option<FeatureOutcome>, PlanError> {
    let location = format!("features[{i}]");

    if let Some(uid) = &feature.uid {
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
        // The patch is resolved before walking `behaviors` so that walk
        // knows the id this Feature will actually carry: a Behavior under
        // a Feature renamed by this same Intent must record the new id,
        // not the one still on disk.
        let candidate = apply_feature_patch(&current, feature, resolved_requirement_uids);
        let handled_behavior_uids = if feature.behaviors.is_empty() {
            HashSet::new()
        } else {
            plan_existing_behaviors(
                root,
                &location,
                &current.id,
                &candidate.id,
                &feature.behaviors,
                diagnostics,
                edits,
            )?
        };
        if candidate.id != current.id {
            collect_behavior_back_reference_fixups(
                root,
                &current.id,
                &candidate.id,
                &handled_behavior_uids,
                &mut edits.back_reference_fixups,
            )?;
        }
        if candidate == current {
            return Ok(Some(FeatureOutcome::Unchanged {
                uid: uid.clone(),
                id: current.id,
                path: found.path,
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
            let behaviors = plan_new_behaviors(&location, id, &feature.behaviors, diagnostics)?;
            Ok(Some(FeatureOutcome::New {
                uid,
                canonical,
                behaviors,
            }))
        }
        Some(found) => {
            // Unlike the UID-selected branch above, this Feature was only
            // matched by `id` — no UID was given. ADR 0027 §3 never infers
            // identity from content/id similarity alone, so mutating scope
            // relationships (which Behavior owns which Scenario) here,
            // without the caller having explicitly named this Feature's
            // UID, would be exactly that kind of inference. Reparenting
            // requires selecting the Feature by UID.
            if !feature.behaviors.is_empty() {
                return Err(PlanError::NotYetSupported(format!(
                    "{location}.behaviors: reparenting Scenarios requires selecting the Feature by uid"
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
                    path: found.path,
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
) -> Result<Vec<NewBehavior>, PlanError> {
    let mut planned = Vec::new();
    for (j, behavior) in behaviors.iter().enumerate() {
        let location = format!("{feature_location}.behaviors[{j}]");
        if let Some(uid) = &behavior.uid {
            // The parent Feature is brand-new in this same Intent, so it
            // cannot already own an existing Behavior — a uid-given entry
            // here can only be a scope conflict (ADR 0027 §3), never a
            // legitimate reparent target (Behavior reparenting itself is
            // not supported; only nested Scenarios can move, and only
            // between Behaviors reached under their own current Feature —
            // see `plan_scenario_reparents`).
            diagnostics.push(Diagnostic::new(
                DiagnosticCode::ConflictingScope,
                location,
                format!(
                    "Behavior '{uid}' cannot belong to a Feature that does not exist yet; Behavior reparenting is not supported"
                ),
            ));
            continue;
        }
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
        let procedures: std::collections::BTreeMap<String, knowledge::Procedure> = behavior
            .procedures
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|p| (p.name, knowledge::Procedure { steps: p.steps }))
            .collect();
        let uid = ulid::Ulid::new().to_string();
        let canonical = Behavior {
            id: id.clone(),
            feature: feature_id.to_string(),
            label: behavior.label.clone().unwrap_or_else(|| id.clone()),
            axis: behavior.axis.clone().unwrap_or_default(),
            description: canonical_description(description),
            procedures: procedures.clone(),
            uid: Some(uid.clone()),
        };
        let scenarios =
            plan_new_scenarios(&location, id, &procedures, &behavior.scenarios, diagnostics)?;
        planned.push(NewBehavior {
            uid,
            canonical,
            scenarios,
        });
    }
    Ok(planned)
}

fn plan_new_scenarios(
    behavior_location: &str,
    behavior_id: &str,
    behavior_procedures: &std::collections::BTreeMap<String, knowledge::Procedure>,
    scenarios: &[super::intent::ScenarioIntent],
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Vec<NewScenario>, PlanError> {
    let mut planned = Vec::new();
    for (k, scenario) in scenarios.iter().enumerate() {
        let location = format!("{behavior_location}.scenarios[{k}]");
        if scenario.uid.is_some() {
            // Reparenting an existing Scenario into a brand-new Behavior is
            // plausible under ADR 0027 §3 ("別のFeatureまたはBehaviorへ配置
            // される" does not require the destination to already exist),
            // but this function only ever builds genuinely-new Scenarios;
            // routing a uid-given entry to the reparent path
            // (`plan_scenario_reparents`) as well would need its own
            // `scenario_updates`/diagnostics threading through this
            // brand-new-parent path. Not yet supported.
            return Err(PlanError::NotYetSupported(format!(
                "{location}: reparenting a Scenario into a brand-new Behavior is not supported yet"
            )));
        }
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
        let converted_phases: Vec<KnowledgePhase> = phases.into_iter().map(convert_phase).collect();
        if let Some(diagnostic) =
            check_procedure_references(&location, behavior_procedures, &converted_phases)
        {
            diagnostics.push(diagnostic);
            continue;
        }
        let uid = ulid::Ulid::new().to_string();
        let canonical = Scenario {
            id: id.clone(),
            behavior: behavior_id.to_string(),
            label: scenario.label.clone().unwrap_or_else(|| id.clone()),
            description: canonical_description(description),
            phases: converted_phases,
            implementation_note: None,
            generated_by: None,
            verified_by: None,
            uid: Some(uid.clone()),
        };
        planned.push(NewScenario { uid, canonical });
    }
    Ok(planned)
}

/// Builds a UID-selected Behavior's patched content (ADR 0027 §5: present
/// replaces, omitted keeps current). `axis` and `procedures` are value
/// collections, so an explicit empty one clears them while omitting keeps
/// them. `feature` is not taken from `intent` — moving a Behavior to
/// another Feature is the reparent ADR 0027 §3 reserves for Scenarios — but
/// it does follow `feature_effective_id`, so a Behavior under a Feature
/// this same Intent renames keeps pointing at its parent.
fn apply_behavior_patch(
    current: &Behavior,
    intent: &BehaviorIntent,
    feature_effective_id: &str,
) -> Behavior {
    Behavior {
        id: intent.id.clone().unwrap_or_else(|| current.id.clone()),
        feature: feature_effective_id.to_string(),
        label: intent
            .label
            .clone()
            .unwrap_or_else(|| current.label.clone()),
        axis: intent.axis.clone().unwrap_or_else(|| current.axis.clone()),
        description: intent
            .description
            .as_deref()
            .map(canonical_description)
            .unwrap_or_else(|| current.description.clone()),
        procedures: match &intent.procedures {
            Some(procedures) => procedures
                .iter()
                .map(|p| {
                    (
                        p.name.clone(),
                        knowledge::Procedure {
                            steps: p.steps.clone(),
                        },
                    )
                })
                .collect(),
            None => current.procedures.clone(),
        },
        uid: current.uid.clone(),
    }
}

fn parse_behavior_file(path: &Path) -> io::Result<Behavior> {
    let content = fs::read_to_string(path)?;
    knowledge::parse_behavior(&content).map_err(io::Error::other)
}

fn parse_scenario_file(path: &Path) -> io::Result<Scenario> {
    let content = fs::read_to_string(path)?;
    knowledge::parse_scenario(&content).map_err(io::Error::other)
}

/// Walks `behaviors` (a UID-selected Feature's `behaviors` list) and plans
/// both the Behaviors' own patches (ADR 0027 §5) and any existing
/// Scenarios listed under them, which may be updated in place or
/// reparented (ADR 0027 §3). Each `BehaviorIntent` here must itself carry
/// a `uid` naming a Behavior that *already belongs to*
/// `feature_current_id`; creating a new Behavior under an existing Feature
/// remains unsupported, so a `BehaviorIntent` without a `uid` is rejected.
/// Returns the uids of the Behaviors it handled explicitly, so the caller
/// can skip them when sweeping the Feature's remaining children for
/// back-reference fixups.
fn plan_existing_behaviors(
    root: &Path,
    feature_location: &str,
    feature_current_id: &str,
    feature_effective_id: &str,
    behaviors: &[BehaviorIntent],
    diagnostics: &mut Vec<Diagnostic>,
    edits: &mut ExistingElementEdits,
) -> Result<HashSet<String>, PlanError> {
    let mut handled_behavior_uids = HashSet::new();
    for (j, behavior) in behaviors.iter().enumerate() {
        let location = format!("{feature_location}.behaviors[{j}]");
        let Some(behavior_uid) = &behavior.uid else {
            return Err(PlanError::NotYetSupported(format!(
                "{location}: adding a new Behavior to an existing Feature is not supported yet"
            )));
        };
        let Some(found_behavior) =
            knowledge_walk::find_by_uid(root, EntityKind::Behavior, behavior_uid)?
        else {
            diagnostics.push(Diagnostic::new(
                DiagnosticCode::UnknownUid,
                location,
                format!("no Behavior with uid '{behavior_uid}' exists"),
            ));
            continue;
        };
        handled_behavior_uids.insert(behavior_uid.clone());
        let current_behavior = parse_behavior_file(&found_behavior.path)?;
        if current_behavior.feature != feature_current_id {
            diagnostics.push(Diagnostic::new(
                DiagnosticCode::ConflictingScope,
                location,
                format!(
                    "Behavior '{}' currently belongs to Feature '{}', not '{feature_current_id}'; Behavior reparenting is not supported, only Scenario reparenting",
                    current_behavior.id, current_behavior.feature
                ),
            ));
            continue;
        }
        let patched_behavior =
            apply_behavior_patch(&current_behavior, behavior, feature_effective_id);
        if patched_behavior.id != current_behavior.id
            && let Some(conflict) = behavior_id_taken_within_feature(
                root,
                feature_current_id,
                &patched_behavior.id,
                behavior_uid,
            )?
        {
            diagnostics.push(Diagnostic::new(
                DiagnosticCode::ConflictingExistingValue,
                format!("{location}.id"),
                format!(
                    "Behavior '{conflict}' under this Feature already uses id '{}'",
                    patched_behavior.id
                ),
            ));
            continue;
        }
        edits
            .behaviors
            .push(if patched_behavior == current_behavior {
                BehaviorOutcome::Unchanged {
                    uid: behavior_uid.clone(),
                    id: current_behavior.id.clone(),
                    path: found_behavior.path.clone(),
                }
            } else {
                BehaviorOutcome::Updated {
                    uid: behavior_uid.clone(),
                    before_id: current_behavior.id.clone(),
                    canonical: Box::new(patched_behavior.clone()),
                    existing_path: found_behavior.path.clone(),
                }
            });
        // A renamed Behavior's own file stays where it is, exactly as a
        // renamed Requirement or Feature's does. Deriving its Scenarios'
        // paths from that directory — rather than from the Behavior's new
        // id — is what keeps the subtree together instead of splitting it
        // across an old-named and a new-named directory.
        let behavior_dir = found_behavior
            .path
            .parent()
            .unwrap_or(&found_behavior.path)
            .to_path_buf();
        // Scenarios are checked against the *patched* procedures: a
        // procedure this same Intent adds must be usable by a `use:` step
        // it also adds, and one it removes must stop resolving.
        let current_behavior = patched_behavior;
        let mut handled_scenario_uids: HashSet<String> = HashSet::new();

        for (k, scenario) in behavior.scenarios.iter().enumerate() {
            let scenario_location = format!("{location}.scenarios[{k}]");
            let Some(scenario_uid) = &scenario.uid else {
                return Err(PlanError::NotYetSupported(format!(
                    "{scenario_location}: creating a new Scenario under an existing Behavior is not supported yet"
                )));
            };
            let Some(found_scenario) =
                knowledge_walk::find_by_uid(root, EntityKind::Scenario, scenario_uid)?
            else {
                diagnostics.push(Diagnostic::new(
                    DiagnosticCode::UnknownUid,
                    scenario_location,
                    format!("no Scenario with uid '{scenario_uid}' exists"),
                ));
                continue;
            };
            handled_scenario_uids.insert(scenario_uid.clone());
            let current_scenario = parse_scenario_file(&found_scenario.path)?;
            let candidate = match apply_scenario_patch(
                &scenario_location,
                &current_scenario,
                scenario,
                &current_behavior.id,
            ) {
                Ok(c) => c,
                Err(d) => {
                    diagnostics.push(d);
                    continue;
                }
            };
            if let Some(diagnostic) = check_procedure_references(
                &scenario_location,
                &current_behavior.procedures,
                &candidate.phases,
            ) {
                diagnostics.push(diagnostic);
                continue;
            }
            let new_path = behavior_dir.join(&candidate.id).join("scenario.yml");
            if candidate == current_scenario && new_path == found_scenario.path {
                edits.scenarios.push(ScenarioOutcome::Unchanged {
                    uid: scenario_uid.clone(),
                    id: current_scenario.id.clone(),
                    path: found_scenario.path,
                });
                continue;
            }
            if new_path != found_scenario.path && new_path.is_file() {
                // Same reasoning as `plan_requirement`/`plan_feature`'s
                // path-collision check: a renamed/reparented Scenario's
                // old file is never moved automatically by an unrelated
                // write, so the target path could already be occupied.
                diagnostics.push(Diagnostic::new(
                    DiagnosticCode::ConflictingExistingValue,
                    scenario_location,
                    format!(
                        "a file already exists at the target path ({}); choose a different id or target Behavior",
                        new_path.display()
                    ),
                ));
                continue;
            }
            edits.scenarios.push(ScenarioOutcome::Updated {
                uid: scenario_uid.clone(),
                before_id: current_scenario.id.clone(),
                canonical: Box::new(candidate),
                existing_path: found_scenario.path,
                new_path,
            });
        }

        // Every Scenario the Intent did *not* name still records the old
        // `behavior:` id, so a rename has to carry them along too.
        if current_behavior.id != found_behavior.id {
            collect_scenario_back_reference_fixups(
                root,
                &found_behavior.id,
                &current_behavior.id,
                &handled_scenario_uids,
                &mut edits.back_reference_fixups,
            )?;
        }
    }
    Ok(handled_behavior_uids)
}

/// Whether another Behavior under `feature_id` already uses `candidate_id`
/// — the scope-aware id-uniqueness check a rename needs, since
/// `knowledge_walk::find_by_id` matches across every Feature and would
/// report an unrelated Feature's Behavior as a conflict.
fn behavior_id_taken_within_feature(
    root: &Path,
    feature_id: &str,
    candidate_id: &str,
    renaming_uid: &str,
) -> io::Result<Option<String>> {
    for found in knowledge_walk::list_entities(root, EntityKind::Behavior)? {
        if found.uid.as_deref() == Some(renaming_uid) {
            continue;
        }
        let behavior = parse_behavior_file(&found.path)?;
        if behavior.feature == feature_id && behavior.id == candidate_id {
            return Ok(Some(behavior.id));
        }
    }
    Ok(None)
}

fn collect_behavior_back_reference_fixups(
    root: &Path,
    old_feature_id: &str,
    new_feature_id: &str,
    already_handled: &HashSet<String>,
    fixups: &mut Vec<BackReferenceFixup>,
) -> io::Result<()> {
    for found in knowledge_walk::list_entities(root, EntityKind::Behavior)? {
        if found
            .uid
            .as_deref()
            .is_some_and(|uid| already_handled.contains(uid))
        {
            continue;
        }
        let mut behavior = parse_behavior_file(&found.path)?;
        if behavior.feature != old_feature_id {
            continue;
        }
        behavior.feature = new_feature_id.to_string();
        fixups.push(BackReferenceFixup::Behavior {
            uid: behavior.uid.clone(),
            canonical: Box::new(behavior),
            path: found.path,
        });
    }
    Ok(())
}

fn collect_scenario_back_reference_fixups(
    root: &Path,
    old_behavior_id: &str,
    new_behavior_id: &str,
    already_handled: &HashSet<String>,
    fixups: &mut Vec<BackReferenceFixup>,
) -> io::Result<()> {
    for found in knowledge_walk::list_entities(root, EntityKind::Scenario)? {
        if found
            .uid
            .as_deref()
            .is_some_and(|uid| already_handled.contains(uid))
        {
            continue;
        }
        let mut scenario = parse_scenario_file(&found.path)?;
        if scenario.behavior != old_behavior_id {
            continue;
        }
        scenario.behavior = new_behavior_id.to_string();
        fixups.push(BackReferenceFixup::Scenario {
            uid: scenario.uid.clone(),
            canonical: Box::new(scenario),
            path: found.path,
        });
    }
    Ok(())
}

/// Builds a UID-selected Scenario's patched content (ADR 0027 §5: present
/// replaces, omitted keeps current) and sets `behavior` to
/// `target_behavior_id` — the Behavior this Scenario is explicitly placed
/// under in this Intent, whether that is where it already was
/// (content-only update) or a different Behavior (reparent, ADR 0027 §3).
/// `case_revision` is never computed or stored here: `identity::
/// derived_uid::case_revision` derives it purely from `phases` on demand.
fn apply_scenario_patch(
    location: &str,
    current: &Scenario,
    intent: &super::intent::ScenarioIntent,
    target_behavior_id: &str,
) -> Result<Scenario, Diagnostic> {
    let phases = match &intent.phases {
        Some(phases) => {
            if phases.is_empty() {
                return Err(Diagnostic::new(
                    DiagnosticCode::MissingRequiredField,
                    format!("{location}.phases"),
                    "at least one phase is required",
                ));
            }
            phases.clone().into_iter().map(convert_phase).collect()
        }
        None => current.phases.clone(),
    };
    Ok(Scenario {
        id: intent.id.clone().unwrap_or_else(|| current.id.clone()),
        behavior: target_behavior_id.to_string(),
        label: intent
            .label
            .clone()
            .unwrap_or_else(|| current.label.clone()),
        description: intent
            .description
            .as_deref()
            .map(canonical_description)
            .unwrap_or_else(|| current.description.clone()),
        phases,
        implementation_note: current.implementation_note.clone(),
        generated_by: current.generated_by,
        verified_by: current.verified_by.clone(),
        uid: current.uid.clone(),
    })
}

/// Checks every `use:` step against the owning Behavior's `procedures` map
/// (ADR 0027 §7 `invalid_procedure_reference`). `BehaviorIntent` has no
/// `procedures` field yet (see this module's own doc comment / Notes), so
/// a brand-new Behavior always has an empty map — any `use:` step under a
/// newly created Scenario is therefore unresolvable by construction, not
/// just a possible mistake.
fn check_procedure_references(
    location: &str,
    procedures: &std::collections::BTreeMap<String, knowledge::Procedure>,
    phases: &[KnowledgePhase],
) -> Option<Diagnostic> {
    for (i, phase) in phases.iter().enumerate() {
        for (j, step) in phase.steps.iter().enumerate() {
            if let KnowledgeStepItem::Use { procedure } = step
                && !procedures.contains_key(procedure)
            {
                return Some(Diagnostic::new(
                    DiagnosticCode::InvalidProcedureReference,
                    format!("{location}.phases[{i}].steps[{j}]"),
                    format!("no procedure named '{procedure}' is defined on this Behavior"),
                ));
            }
        }
    }
    None
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
            RequirementOutcome::Unchanged { uid, id, .. } => {
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

    /// A Feature `todo-management` with two Behaviors, `capture` (owning
    /// Scenario `empty-title`) and `review`, plus an unrelated Feature
    /// `other` with its own Behavior `unrelated`. Returns
    /// (dir, feature_uid, capture_behavior_uid, review_behavior_uid,
    /// scenario_uid, other_feature_uid, unrelated_behavior_uid).
    #[allow(clippy::type_complexity)]
    fn reparent_fixture() -> (
        tempfile::TempDir,
        String,
        String,
        String,
        String,
        String,
        String,
    ) {
        let dir = init_project();
        let feature_uid = "01ARZ3NDEKTSV4RRFFQ69G5FE00".to_string();
        let capture_uid = "01ARZ3NDEKTSV4RRFFQ69G5FE01".to_string();
        let review_uid = "01ARZ3NDEKTSV4RRFFQ69G5FE02".to_string();
        let scenario_uid = "01ARZ3NDEKTSV4RRFFQ69G5FE03".to_string();
        let other_feature_uid = "01ARZ3NDEKTSV4RRFFQ69G5FE04".to_string();
        let unrelated_behavior_uid = "01ARZ3NDEKTSV4RRFFQ69G5FE05".to_string();

        fs::create_dir_all(
            dir.path()
                .join(".markharness/knowledge/features/todo-management/capture/empty-title"),
        )
        .unwrap();
        fs::create_dir_all(
            dir.path()
                .join(".markharness/knowledge/features/todo-management/review"),
        )
        .unwrap();
        fs::create_dir_all(
            dir.path()
                .join(".markharness/knowledge/features/other/unrelated"),
        )
        .unwrap();

        fs::write(
            dir.path()
                .join(".markharness/knowledge/features/todo-management/feature.yml"),
            format!(
                "id: todo-management\nrequirement_uids: []\nlabel: TODO management\naxis: []\nuid: {feature_uid}\n"
            ),
        )
        .unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/features/todo-management/capture/behavior.yml"),
            format!(
                "id: capture\nfeature: todo-management\nlabel: capture\naxis: []\ndescription: Capture a TODO.\nprocedures: {{}}\nuid: {capture_uid}\n"
            ),
        )
        .unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/features/todo-management/review/behavior.yml"),
            format!(
                "id: review\nfeature: todo-management\nlabel: review\naxis: []\ndescription: Review a TODO.\nprocedures: {{}}\nuid: {review_uid}\n"
            ),
        )
        .unwrap();
        fs::write(
            dir.path().join(
                ".markharness/knowledge/features/todo-management/capture/empty-title/scenario.yml",
            ),
            format!(
                "id: empty-title\nbehavior: capture\nlabel: empty-title\ndescription: An empty title cannot be added\nphases:\n  - steps:\n      - action: Attempt to add an empty title\n    results:\n      - No TODO is added\nuid: {scenario_uid}\n"
            ),
        )
        .unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/features/other/feature.yml"),
            format!(
                "id: other\nrequirement_uids: []\nlabel: other\naxis: []\nuid: {other_feature_uid}\n"
            ),
        )
        .unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/features/other/unrelated/behavior.yml"),
            format!(
                "id: unrelated\nfeature: other\nlabel: unrelated\naxis: []\ndescription: Unrelated.\nprocedures: {{}}\nuid: {unrelated_behavior_uid}\n"
            ),
        )
        .unwrap();

        (
            dir,
            feature_uid,
            capture_uid,
            review_uid,
            scenario_uid,
            other_feature_uid,
            unrelated_behavior_uid,
        )
    }

    #[test]
    fn reparents_a_scenario_to_a_different_behavior_under_the_same_feature() {
        let (dir, feature_uid, _capture_uid, review_uid, scenario_uid, ..) = reparent_fixture();
        let yaml = format!(
            "\
format: markharness/knowledge-intent/v1
mode: merge

features:
  - uid: {feature_uid}
    behaviors:
      - uid: {review_uid}
        scenarios:
          - uid: {scenario_uid}
"
        );
        let doc = parse_intent(&yaml).unwrap();
        let plan = build_plan(dir.path(), &doc).unwrap();
        assert_eq!(plan.scenario_updates.len(), 1);
        match &plan.scenario_updates[0] {
            ScenarioOutcome::Updated {
                canonical,
                existing_path,
                new_path,
                ..
            } => {
                assert_eq!(canonical.behavior, "review");
                assert_ne!(existing_path, new_path);
                assert!(new_path.ends_with("review/empty-title/scenario.yml"));
            }
            other => panic!("expected Updated, got {other:?}"),
        }
    }

    #[test]
    fn reparents_a_scenario_to_a_behavior_under_a_different_feature() {
        let (
            dir,
            _feature_uid,
            _capture_uid,
            _review_uid,
            scenario_uid,
            other_feature_uid,
            unrelated_behavior_uid,
        ) = reparent_fixture();
        let yaml = format!(
            "\
format: markharness/knowledge-intent/v1
mode: merge

features:
  - uid: {other_feature_uid}
    behaviors:
      - uid: {unrelated_behavior_uid}
        scenarios:
          - uid: {scenario_uid}
"
        );
        let doc = parse_intent(&yaml).unwrap();
        let plan = build_plan(dir.path(), &doc).unwrap();
        assert_eq!(plan.scenario_updates.len(), 1);
        match &plan.scenario_updates[0] {
            ScenarioOutcome::Updated {
                canonical,
                new_path,
                ..
            } => {
                assert_eq!(canonical.behavior, "unrelated");
                assert!(new_path.ends_with("other/unrelated/empty-title/scenario.yml"));
            }
            other => panic!("expected Updated, got {other:?}"),
        }
    }

    #[test]
    fn a_scenario_placed_back_under_its_current_behavior_with_no_content_change_is_unchanged() {
        let (dir, feature_uid, capture_uid, _review_uid, scenario_uid, ..) = reparent_fixture();
        let yaml = format!(
            "\
format: markharness/knowledge-intent/v1
mode: merge

features:
  - uid: {feature_uid}
    behaviors:
      - uid: {capture_uid}
        scenarios:
          - uid: {scenario_uid}
"
        );
        let doc = parse_intent(&yaml).unwrap();
        let plan = build_plan(dir.path(), &doc).unwrap();
        assert_eq!(plan.scenario_updates.len(), 1);
        assert!(matches!(
            &plan.scenario_updates[0],
            ScenarioOutcome::Unchanged { .. }
        ));
    }

    #[test]
    fn reparenting_also_applies_a_content_patch_in_the_same_operation() {
        let (dir, feature_uid, _capture_uid, review_uid, scenario_uid, ..) = reparent_fixture();
        let yaml = format!(
            "\
format: markharness/knowledge-intent/v1
mode: merge

features:
  - uid: {feature_uid}
    behaviors:
      - uid: {review_uid}
        scenarios:
          - uid: {scenario_uid}
            label: Renamed label
"
        );
        let doc = parse_intent(&yaml).unwrap();
        let plan = build_plan(dir.path(), &doc).unwrap();
        match &plan.scenario_updates[0] {
            ScenarioOutcome::Updated { canonical, .. } => {
                assert_eq!(canonical.behavior, "review");
                assert_eq!(canonical.label, "Renamed label");
            }
            other => panic!("expected Updated, got {other:?}"),
        }
    }

    #[test]
    fn referencing_a_behavior_that_belongs_to_a_different_feature_reports_conflicting_scope() {
        let (
            dir,
            feature_uid,
            _capture_uid,
            _review_uid,
            scenario_uid,
            _other_feature_uid,
            unrelated_behavior_uid,
        ) = reparent_fixture();
        // `unrelated_behavior_uid` actually belongs to Feature "other", not
        // to the Feature named by `feature_uid` here.
        let yaml = format!(
            "\
format: markharness/knowledge-intent/v1
mode: merge

features:
  - uid: {feature_uid}
    behaviors:
      - uid: {unrelated_behavior_uid}
        scenarios:
          - uid: {scenario_uid}
"
        );
        let doc = parse_intent(&yaml).unwrap();
        let err = build_plan(dir.path(), &doc).unwrap_err();
        match err {
            PlanError::Diagnostics(diagnostics) => {
                assert_eq!(diagnostics[0].code, DiagnosticCode::ConflictingScope);
            }
            other => panic!("expected Diagnostics, got {other:?}"),
        }
    }

    #[test]
    fn unknown_behavior_uid_during_reparent_reports_unknown_uid() {
        let (dir, feature_uid, .., scenario_uid, _other_feature_uid, _unrelated_behavior_uid) =
            reparent_fixture();
        let yaml = format!(
            "\
format: markharness/knowledge-intent/v1
mode: merge

features:
  - uid: {feature_uid}
    behaviors:
      - uid: 01ARZ3NDEKTSV4RRFFQ69G5FFFF
        scenarios:
          - uid: {scenario_uid}
"
        );
        let doc = parse_intent(&yaml).unwrap();
        let err = build_plan(dir.path(), &doc).unwrap_err();
        match err {
            PlanError::Diagnostics(diagnostics) => {
                assert_eq!(diagnostics[0].code, DiagnosticCode::UnknownUid);
            }
            other => panic!("expected Diagnostics, got {other:?}"),
        }
    }

    #[test]
    fn unknown_scenario_uid_during_reparent_reports_unknown_uid() {
        let (dir, feature_uid, capture_uid, ..) = reparent_fixture();
        let yaml = format!(
            "\
format: markharness/knowledge-intent/v1
mode: merge

features:
  - uid: {feature_uid}
    behaviors:
      - uid: {capture_uid}
        scenarios:
          - uid: 01ARZ3NDEKTSV4RRFFQ69G5FFFF
"
        );
        let doc = parse_intent(&yaml).unwrap();
        let err = build_plan(dir.path(), &doc).unwrap_err();
        match err {
            PlanError::Diagnostics(diagnostics) => {
                assert_eq!(diagnostics[0].code, DiagnosticCode::UnknownUid);
            }
            other => panic!("expected Diagnostics, got {other:?}"),
        }
    }

    #[test]
    fn a_new_behavior_under_an_existing_feature_is_not_yet_supported() {
        let (dir, feature_uid, ..) = reparent_fixture();
        let yaml = format!(
            "\
format: markharness/knowledge-intent/v1
mode: merge

features:
  - uid: {feature_uid}
    behaviors:
      - id: brand-new
        description: A brand new Behavior.
"
        );
        let doc = parse_intent(&yaml).unwrap();
        let err = build_plan(dir.path(), &doc).unwrap_err();
        assert!(matches!(err, PlanError::NotYetSupported(_)));
    }

    #[test]
    fn a_new_scenario_under_an_existing_behavior_is_not_yet_supported() {
        let (dir, feature_uid, capture_uid, ..) = reparent_fixture();
        let yaml = format!(
            "\
format: markharness/knowledge-intent/v1
mode: merge

features:
  - uid: {feature_uid}
    behaviors:
      - uid: {capture_uid}
        scenarios:
          - id: brand-new
            description: A brand new Scenario.
            phases:
              - steps:
                  - action: Do it.
                results:
                  - Confirmed.
"
        );
        let doc = parse_intent(&yaml).unwrap();
        let err = build_plan(dir.path(), &doc).unwrap_err();
        assert!(matches!(err, PlanError::NotYetSupported(_)));
    }

    #[test]
    fn a_uid_given_behavior_under_a_brand_new_feature_reports_conflicting_scope() {
        let (dir, _feature_uid, capture_uid, ..) = reparent_fixture();
        let yaml = format!(
            "\
format: markharness/knowledge-intent/v1
mode: merge

features:
  - id: brand-new-feature
    label: Brand new feature
    axis: []
    behaviors:
      - uid: {capture_uid}
"
        );
        let doc = parse_intent(&yaml).unwrap();
        let err = build_plan(dir.path(), &doc).unwrap_err();
        match err {
            PlanError::Diagnostics(diagnostics) => {
                assert_eq!(diagnostics[0].code, DiagnosticCode::ConflictingScope);
            }
            other => panic!("expected Diagnostics, got {other:?}"),
        }
    }

    /// ADR 0027 §5 applies to a UID-selected Behavior exactly as it does
    /// to a Requirement or Feature: every field the Intent names replaces
    /// the current one. Silently keeping the old values while reporting
    /// success would leave the caller's declaration and the stored state
    /// diverged.
    #[test]
    fn a_uid_selected_behavior_patches_label_axis_description_and_procedures() {
        let (dir, feature_uid, capture_uid, ..) = reparent_fixture();
        let yaml = format!(
            "\
format: markharness/knowledge-intent/v1
mode: merge

features:
  - uid: {feature_uid}
    behaviors:
      - uid: {capture_uid}
        label: Capture (renamed label)
        axis: [functional]
        description: A new description.
        procedures:
          - name: validate_title
            steps: [Check the title is non-empty]
"
        );
        let doc = parse_intent(&yaml).unwrap();
        let plan = build_plan(dir.path(), &doc).unwrap();
        assert_eq!(plan.behavior_updates.len(), 1);
        let BehaviorOutcome::Updated { canonical, .. } = &plan.behavior_updates[0] else {
            panic!("expected Updated, got {:?}", plan.behavior_updates[0]);
        };
        assert_eq!(canonical.label, "Capture (renamed label)");
        assert_eq!(canonical.axis, vec!["functional".to_string()]);
        // Canonical descriptions are newline-terminated: the serializer
        // always writes a block scalar, which reads back that way.
        assert_eq!(canonical.description, "A new description.\n");
        assert_eq!(
            canonical.procedures.get("validate_title").unwrap().steps,
            vec!["Check the title is non-empty".to_string()]
        );
        assert_eq!(canonical.id, "capture", "id is untouched");
        assert_eq!(canonical.feature, "todo-management", "scope is untouched");
    }

    /// The other half of ADR 0027 §5: an omitted field keeps its current
    /// value, while an explicitly empty value collection clears it.
    #[test]
    fn a_uid_selected_behavior_keeps_omitted_fields_and_clears_explicitly_empty_collections() {
        let (dir, feature_uid, capture_uid, ..) = reparent_fixture();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/features/todo-management/capture/behavior.yml"),
            format!(
                "id: capture\nfeature: todo-management\nlabel: capture\naxis: [functional]\ndescription: Capture a TODO.\nprocedures:\n  validate_title:\n    steps:\n      - Check the title is non-empty\nuid: {capture_uid}\n"
            ),
        )
        .unwrap();

        // Omitting every patchable field leaves the Behavior untouched.
        let keep = format!(
            "format: markharness/knowledge-intent/v1\nmode: merge\n\nfeatures:\n  - uid: {feature_uid}\n    behaviors:\n      - uid: {capture_uid}\n"
        );
        let plan = build_plan(dir.path(), &parse_intent(&keep).unwrap()).unwrap();
        assert!(
            matches!(
                &plan.behavior_updates[0],
                BehaviorOutcome::Unchanged { id, .. } if id == "capture"
            ),
            "got {:?}",
            plan.behavior_updates[0]
        );

        // Explicit empty collections clear both.
        let clear = format!(
            "format: markharness/knowledge-intent/v1\nmode: merge\n\nfeatures:\n  - uid: {feature_uid}\n    behaviors:\n      - uid: {capture_uid}\n        axis: []\n        procedures: []\n"
        );
        let plan = build_plan(dir.path(), &parse_intent(&clear).unwrap()).unwrap();
        let BehaviorOutcome::Updated { canonical, .. } = &plan.behavior_updates[0] else {
            panic!("expected Updated, got {:?}", plan.behavior_updates[0]);
        };
        assert!(canonical.axis.is_empty());
        assert!(canonical.procedures.is_empty());
        assert_eq!(
            canonical.description, "Capture a TODO.",
            "an omitted scalar keeps the current value verbatim"
        );
    }

    /// ADR 0027 §3's rename row is not kind-specific: a UID-selected
    /// Behavior whose only difference is its display id is an explicit
    /// rename. Its unnamed child Scenarios must follow, since each stores
    /// the parent's id in `behavior:`.
    #[test]
    fn renaming_a_uid_selected_behavior_carries_its_unnamed_scenarios_along() {
        let (dir, feature_uid, capture_uid, ..) = reparent_fixture();
        let yaml = format!(
            "format: markharness/knowledge-intent/v1\nmode: merge\n\nfeatures:\n  - uid: {feature_uid}\n    behaviors:\n      - uid: {capture_uid}\n        id: recorded\n"
        );
        let doc = parse_intent(&yaml).unwrap();
        let plan = build_plan(dir.path(), &doc).unwrap();

        let BehaviorOutcome::Updated {
            before_id,
            canonical,
            ..
        } = &plan.behavior_updates[0]
        else {
            panic!("expected Updated, got {:?}", plan.behavior_updates[0]);
        };
        assert_eq!(before_id, "capture");
        assert_eq!(canonical.id, "recorded");

        assert_eq!(plan.back_reference_fixups.len(), 1);
        let BackReferenceFixup::Scenario { canonical, .. } = &plan.back_reference_fixups[0] else {
            panic!(
                "expected a Scenario fixup, got {:?}",
                plan.back_reference_fixups[0]
            );
        };
        assert_eq!(canonical.id, "empty-title");
        assert_eq!(canonical.behavior, "recorded");
    }

    /// Two Behaviors under the same Feature must not end up sharing a
    /// display id. `find_by_id` matches across every Feature, so the check
    /// has to be scope-aware — an unrelated Feature's Behavior with the
    /// same id is not a conflict.
    #[test]
    fn renaming_a_behavior_onto_a_sibling_id_reports_conflicting_existing_value() {
        let (dir, feature_uid, capture_uid, ..) = reparent_fixture();
        let yaml = format!(
            "format: markharness/knowledge-intent/v1\nmode: merge\n\nfeatures:\n  - uid: {feature_uid}\n    behaviors:\n      - uid: {capture_uid}\n        id: review\n"
        );
        let doc = parse_intent(&yaml).unwrap();
        match build_plan(dir.path(), &doc).unwrap_err() {
            PlanError::Diagnostics(diagnostics) => assert_eq!(
                diagnostics[0].code,
                DiagnosticCode::ConflictingExistingValue
            ),
            other => panic!("expected Diagnostics, got {other:?}"),
        }
    }

    /// The same rename row applies to a Scenario, whose file does move:
    /// its path is derived from its own id, so the new id gives a new
    /// path under the same Behavior directory.
    #[test]
    fn renaming_a_uid_selected_scenario_moves_it_within_its_behavior() {
        let (dir, feature_uid, capture_uid, _review_uid, scenario_uid, ..) = reparent_fixture();
        let yaml = format!(
            "format: markharness/knowledge-intent/v1\nmode: merge\n\nfeatures:\n  - uid: {feature_uid}\n    behaviors:\n      - uid: {capture_uid}\n        scenarios:\n          - uid: {scenario_uid}\n            id: blank-title\n"
        );
        let doc = parse_intent(&yaml).unwrap();
        let plan = build_plan(dir.path(), &doc).unwrap();

        let ScenarioOutcome::Updated {
            before_id,
            canonical,
            existing_path,
            new_path,
            ..
        } = &plan.scenario_updates[0]
        else {
            panic!("expected Updated, got {:?}", plan.scenario_updates[0]);
        };
        assert_eq!(before_id, "empty-title");
        assert_eq!(canonical.id, "blank-title");
        assert!(existing_path.ends_with("capture/empty-title/scenario.yml"));
        assert!(new_path.ends_with("capture/blank-title/scenario.yml"));
    }

    /// A procedure declared on an existing Behavior in this same Intent
    /// must already resolve for a `use:` step patched into one of its
    /// Scenarios — the check runs against the patched procedures, not the
    /// pre-patch ones.
    #[test]
    fn a_procedure_added_to_an_existing_behavior_resolves_for_its_scenarios_use_step() {
        let (dir, feature_uid, capture_uid, _review_uid, scenario_uid, ..) = reparent_fixture();
        let yaml = format!(
            "\
format: markharness/knowledge-intent/v1
mode: merge

features:
  - uid: {feature_uid}
    behaviors:
      - uid: {capture_uid}
        procedures:
          - name: validate_title
            steps: [Check the title is non-empty]
        scenarios:
          - uid: {scenario_uid}
            phases:
              - steps:
                  - use: validate_title
                results:
                  - No TODO is added
"
        );
        let doc = parse_intent(&yaml).unwrap();
        let plan = build_plan(dir.path(), &doc).unwrap();
        assert_eq!(plan.behavior_updates.len(), 1);
        assert_eq!(plan.scenario_updates.len(), 1);
    }

    /// A brand-new Behavior always starts with an empty `procedures` map —
    /// `BehaviorIntent` has no field to populate it yet (see this module's
    /// doc comment) — so a `use:` step under a Scenario nested in the same
    /// new Behavior can never resolve.
    #[test]
    fn a_new_scenarios_use_step_referencing_an_undefined_procedure_is_rejected() {
        let dir = init_project();
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

features:
  - id: todo-management
    label: TODO management
    axis: []
    behaviors:
      - id: capture
        description: Capture a TODO.
        scenarios:
          - id: empty-title
            description: An empty title cannot be added
            phases:
              - steps:
                  - use: validate_title
                results:
                  - No TODO is added
";
        let doc = parse_intent(yaml).unwrap();
        let err = build_plan(dir.path(), &doc).unwrap_err();
        match err {
            PlanError::Diagnostics(diagnostics) => {
                assert_eq!(
                    diagnostics[0].code,
                    DiagnosticCode::InvalidProcedureReference
                );
            }
            other => panic!("expected Diagnostics, got {other:?}"),
        }
    }

    /// ADR 0027 §5's value-collection replace rule applies to a Behavior's
    /// `procedures` the same as `axis`/`contributes_to`; declaring one on a
    /// brand-new Behavior makes it immediately usable by a `use:` step in
    /// a Scenario nested under that same Behavior in this Intent — the
    /// same capability `knowledge_draft`'s `apply_draft` already has for a
    /// new Behavior (never for an existing one, which is why this stays
    /// scoped to `plan_new_behaviors`).
    #[test]
    fn a_new_behaviors_procedures_are_usable_by_a_use_step_in_its_own_new_scenario() {
        let dir = init_project();
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

features:
  - id: todo-management
    label: TODO management
    axis: []
    behaviors:
      - id: capture
        description: Capture a TODO.
        procedures:
          - name: validate_title
            steps: [Check the title is non-empty]
        scenarios:
          - id: empty-title
            description: An empty title cannot be added
            phases:
              - steps:
                  - use: validate_title
                results:
                  - No TODO is added
";
        let doc = parse_intent(yaml).unwrap();
        let plan = build_plan(dir.path(), &doc).unwrap();
        let feature = &plan.features[0];
        let FeatureOutcome::New { behaviors, .. } = feature else {
            panic!("expected a New Feature outcome");
        };
        assert_eq!(behaviors[0].canonical.procedures.len(), 1);
        assert!(
            behaviors[0]
                .canonical
                .procedures
                .contains_key("validate_title")
        );
        assert_eq!(behaviors[0].scenarios.len(), 1);
    }

    /// An existing Scenario patched/reparented under a UID-selected
    /// Behavior must resolve `use:` steps against *that* Behavior's actual
    /// `procedures` map, not an empty one — this is the same check as the
    /// new-Scenario case above, but exercised on the reparent/patch path.
    #[test]
    fn a_patched_scenarios_use_step_referencing_an_undefined_procedure_is_rejected() {
        let (dir, feature_uid, capture_uid, _review_uid, scenario_uid, ..) = reparent_fixture();
        let yaml = format!(
            "\
format: markharness/knowledge-intent/v1
mode: merge

features:
  - uid: {feature_uid}
    behaviors:
      - uid: {capture_uid}
        scenarios:
          - uid: {scenario_uid}
            phases:
              - steps:
                  - use: validate_title
                results:
                  - No TODO is added
"
        );
        let doc = parse_intent(&yaml).unwrap();
        let err = build_plan(dir.path(), &doc).unwrap_err();
        match err {
            PlanError::Diagnostics(diagnostics) => {
                assert_eq!(
                    diagnostics[0].code,
                    DiagnosticCode::InvalidProcedureReference
                );
            }
            other => panic!("expected Diagnostics, got {other:?}"),
        }
    }

    /// `Plan::default()` (hand-built, not from [`build_plan`]) carries no
    /// fingerprint to compare against — there is nothing to detect
    /// staleness *from* — so [`state_fingerprint`] itself, and the
    /// `None` case, must not be confused with "unchanged".
    #[test]
    fn build_plan_populates_a_state_fingerprint_that_changes_when_the_knowledge_tree_does() {
        let dir = init_project();
        let empty_doc =
            parse_intent("format: markharness/knowledge-intent/v1\nmode: merge\n").unwrap();
        let before = build_plan(dir.path(), &empty_doc).unwrap();
        assert!(before.state_fingerprint.is_some());

        fs::create_dir_all(dir.path().join(".markharness/knowledge/requirements/todo")).unwrap();
        fs::write(
            dir.path()
                .join(".markharness/knowledge/requirements/todo/requirement.yml"),
            "id: todo\nsource: native\nlabel: TODO\naxis: []\nuid: 01ARZ3NDEKTSV4RRFFQ69G5FAV\n",
        )
        .unwrap();

        let after = build_plan(dir.path(), &empty_doc).unwrap();
        assert_ne!(before.state_fingerprint, after.state_fingerprint);

        let unchanged = build_plan(dir.path(), &empty_doc).unwrap();
        assert_eq!(after.state_fingerprint, unchanged.state_fingerprint);
    }
}
