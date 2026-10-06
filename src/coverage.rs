//! Release Coverage (v2 design §6.2): for a chosen set of Requirements, what
//! exists to verify them, and — where a release recorded one — what that
//! release selected.
//!
//! "Selected" and "executed" are never conflated (ADR 0024 §5). A binding
//! says a TestCase has a verification means; it does not say anything ran
//! (ADR 0025 §1).

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

use serde::Serialize;

use crate::binding::{self, ExecutionBinding};
use crate::generate::{self, TestCase};
use crate::git;
use crate::knowledge::{self, Requirement, RequirementSource};
use crate::knowledge_source::{GitTreeKnowledgeSource, KnowledgeSource};
use crate::release::{self, ReleaseScope};

const SCHEMA_VERSION: u32 = 1;
const RECORD_KIND: &str = "release_coverage";

/// Bumped when a change here would make the same inputs produce a different
/// answer, so a recomputation can be compared against the rules that
/// produced the original.
pub const RULE_VERSION: u32 = 1;

#[derive(Debug)]
pub enum CoverageError {
    UnknownRequirement(String),
    Malformed { path: String, message: String },
    Release(release::ReleaseError),
    Io(io::Error),
}

impl From<io::Error> for CoverageError {
    fn from(e: io::Error) -> Self {
        CoverageError::Io(e)
    }
}

impl From<release::ReleaseError> for CoverageError {
    fn from(e: release::ReleaseError) -> Self {
        CoverageError::Release(e)
    }
}

impl std::fmt::Display for CoverageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CoverageError::UnknownRequirement(id) => {
                write!(f, "no Requirement '{id}' at the requested ref")
            }
            CoverageError::Malformed { path, message } => write!(f, "{path}: {message}"),
            CoverageError::Release(e) => write!(f, "{e}"),
            CoverageError::Io(e) => write!(f, "filesystem error: {e}"),
        }
    }
}

/// Why a Requirement or Feature has nothing behind it to verify.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GapKind {
    /// AC08: no Feature contributes to this Requirement.
    RequirementHasNoFeature,
    /// AC21: a Feature contributes, but nothing underneath it produces a
    /// TestCase.
    FeatureHasNoCase,
}

#[derive(Debug, Clone, Serialize)]
pub struct CoverageGap {
    pub kind: GapKind,
    pub requirement_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feature_id: Option<String>,
}

/// Whether a binding's `reference` resolves to something at the requested ref.
/// A structural fact about the tree — `Exists` says the path is there, never
/// that anything ran or passed (ADR 0025 §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceStatus {
    Exists,
    Missing,
    /// A URL: reachability is out of scope, so nothing was checked.
    NotChecked,
}

/// The whole string is judged as one repo-relative path: no `::`/`#` suffix
/// is stripped, so a suffixed reference to an existing file is `Missing`.
fn reference_status(
    root: &Path,
    commit: &str,
    reference: &str,
) -> Result<ReferenceStatus, CoverageError> {
    if reference.contains("://") {
        return Ok(ReferenceStatus::NotChecked);
    }
    // An empty name would match the tree root, and a path that is absolute
    // or climbs out with `..` cannot name something inside the repository.
    let leaves_the_repo = reference.is_empty()
        || reference.starts_with('/')
        || reference.split('/').any(|part| part == "..");
    if leaves_the_repo {
        return Ok(ReferenceStatus::Missing);
    }
    Ok(if git::path_exists_at(root, commit, reference)? {
        ReferenceStatus::Exists
    } else {
        ReferenceStatus::Missing
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseCoverage {
    pub case_id: String,
    pub case_uid: Option<String>,
    pub feature_id: String,
    /// The verification means declared for this case, if any. Its presence
    /// says how the case would be verified — never that it was run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binding_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binding_reference: Option<String>,
    /// Present exactly when `binding_reference` is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference_status: Option<ReferenceStatus>,
    /// Only meaningful when a release scope was requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RequirementCoverage {
    pub requirement_id: String,
    pub requirement_uid: Option<String>,
    pub source: &'static str,
    pub feature_ids: Vec<String>,
    pub cases: Vec<CaseCoverage>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReleaseView {
    pub release_id: String,
    /// Case UIDs the release selected that exist in the Knowledge at this
    /// ref.
    pub selected_case_uids: Vec<String>,
    /// AC25: in scope of the requested Requirements, but not selected.
    /// Candidates for a missed selection — markharness does not judge
    /// whether omitting them was wrong.
    pub unselected_case_uids: Vec<String>,
    /// AC26: selected, but absent from the Knowledge at this ref. Reported
    /// as-is; the selection list is never rewritten automatically.
    pub absent_case_uids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReleaseCoverage {
    pub schema_version: u32,
    pub record_kind: &'static str,
    pub rule_version: u32,
    pub at_commit: String,
    pub requirements: Vec<RequirementCoverage>,
    pub gaps: Vec<CoverageGap>,
    /// Absent when no release was requested, or when the requested release
    /// recorded no scope — in which case this is the registered state only,
    /// not a selection (ADR 0024 §4).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release: Option<ReleaseView>,
}

/// Which Requirements to report on.
pub enum RequirementSelector {
    All,
    Ids(Vec<String>),
}

impl RequirementSelector {
    /// `all`, or a comma-separated list of display ids.
    pub fn parse(value: &str) -> Self {
        if value.trim() == "all" {
            return RequirementSelector::All;
        }
        RequirementSelector::Ids(
            value
                .split(',')
                .map(|id| id.trim().to_string())
                .filter(|id| !id.is_empty())
                .collect(),
        )
    }
}

fn requirements_at(
    root: &Path,
    git_ref: &str,
) -> Result<BTreeMap<String, Requirement>, CoverageError> {
    let entries: Vec<git::TreeEntry> = git::ls_tree_recursive(
        root,
        git_ref,
        &format!(
            "{}/requirements",
            crate::project_root::KNOWLEDGE_PATH_IN_REPO
        ),
    )?
    .into_iter()
    .filter(|entry| entry.kind == git::ObjectKind::Blob && entry.path.ends_with("/requirement.yml"))
    .collect();
    let mut requirements = BTreeMap::new();
    for (entry, content) in entries.iter().zip(git::show_blobs_of(root, &entries)?) {
        let requirement =
            knowledge::parse_requirement(&content).map_err(|e| CoverageError::Malformed {
                path: entry.path.clone(),
                message: e.to_string(),
            })?;
        requirements.insert(requirement.id.clone(), requirement);
    }
    Ok(requirements)
}

fn cases_at(root: &Path, git_ref: &str) -> Result<Vec<TestCase>, CoverageError> {
    let snapshot = GitTreeKnowledgeSource::new(root, git_ref).load_snapshot()?;
    Ok(generate::compile_testcases(&snapshot))
}

/// For every Requirement uid, the Features that name it — themselves or
/// through one of their Scenarios — whether or not they produce a TestCase.
/// Read from the YAML directly rather than from compiled TestCases, so a
/// Feature with no Scenario underneath is still visible — that absence is
/// exactly AC21's gap. Read once for all Requirements: reading it per
/// Requirement re-read every Feature and Scenario for each of them.
fn features_by_requirement(
    root: &Path,
    git_ref: &str,
) -> Result<BTreeMap<String, BTreeSet<String>>, CoverageError> {
    let features_root = format!("{}/features", crate::project_root::KNOWLEDGE_PATH_IN_REPO);
    // (Feature directory name, entry) for every feature.yml / scenario.yml.
    let mut feature_entries = Vec::new();
    let mut scenario_entries = Vec::new();
    for entry in git::ls_tree_recursive(root, git_ref, &features_root)? {
        if entry.kind != git::ObjectKind::Blob {
            continue;
        }
        let Some(relative) = entry.path.strip_prefix(&format!("{features_root}/")) else {
            continue;
        };
        let Some((feature_dir, rest)) = relative.split_once('/') else {
            continue;
        };
        let feature_dir = feature_dir.to_string();
        if rest == "feature.yml" {
            feature_entries.push((feature_dir, entry));
        } else if rest.ends_with("/scenario.yml") {
            scenario_entries.push((feature_dir, entry));
        }
    }
    let shas: Vec<&str> = feature_entries
        .iter()
        .chain(&scenario_entries)
        .map(|(_, entry)| entry.sha.as_str())
        .collect();
    let mut contents = git::show_blobs_by_sha(root, &shas)?.into_iter();

    let mut features_by_requirement: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    // Feature directory name -> feature.id, for the Scenarios below (a
    // scenario.yml carries no feature id of its own).
    let mut feature_id_by_dir = BTreeMap::new();
    for (feature_dir, entry) in &feature_entries {
        let content = contents.next().expect("one content per entry");
        let feature = knowledge::parse_feature(&content).map_err(|e| CoverageError::Malformed {
            path: entry.path.clone(),
            message: e.to_string(),
        })?;
        for uid in &feature.requirement_uids {
            features_by_requirement
                .entry(uid.clone())
                .or_default()
                .insert(feature.id.clone());
        }
        feature_id_by_dir.insert(feature_dir.clone(), feature.id);
    }
    // A Scenario naming the Requirement reaches it through the Feature that
    // owns the Scenario, even when the Feature itself does not.
    for (feature_dir, entry) in &scenario_entries {
        let content = contents.next().expect("one content per entry");
        let scenario =
            knowledge::parse_scenario(&content).map_err(|e| CoverageError::Malformed {
                path: entry.path.clone(),
                message: e.to_string(),
            })?;
        let Some(feature_id) = feature_id_by_dir.get(feature_dir) else {
            continue;
        };
        for uid in &scenario.requirement_uids {
            features_by_requirement
                .entry(uid.clone())
                .or_default()
                .insert(feature_id.clone());
        }
    }
    Ok(features_by_requirement)
}

/// Computes Release Coverage at `git_ref`.
///
/// `release_id` is optional: without it, the answer is the registered state
/// (what exists and how it would be verified). With it, the answer also
/// covers what that release selected — but only for a release that actually
/// recorded a scope (ADR 0024 §4).
pub fn compute(
    root: &Path,
    git_ref: &str,
    selector: &RequirementSelector,
    release_id: Option<&str>,
) -> Result<ReleaseCoverage, CoverageError> {
    let at_commit = git::resolve_commit_oid(root, git_ref)?;
    let all_requirements = requirements_at(root, &at_commit)?;
    let selected_requirements: Vec<(String, Requirement)> = match selector {
        RequirementSelector::All => all_requirements
            .iter()
            .map(|(id, requirement)| (id.clone(), requirement.clone()))
            .collect(),
        RequirementSelector::Ids(ids) => {
            let mut selected = Vec::new();
            for id in ids {
                let requirement = all_requirements
                    .get(id)
                    .ok_or_else(|| CoverageError::UnknownRequirement(id.clone()))?;
                selected.push((id.clone(), requirement.clone()));
            }
            selected
        }
    };

    let cases = cases_at(root, &at_commit)?;
    let features_by_requirement = features_by_requirement(root, &at_commit)?;
    // Read at the same ref as the Knowledge and the scope: a question about a
    // past ref must be answered from what that ref recorded, not from today's
    // working tree (design principle P3, AC11).
    let bindings: BTreeMap<String, ExecutionBinding> = binding::read_all_at(root, &at_commit)
        .map_err(|e| CoverageError::Malformed {
            path: format!("{}/bindings", crate::project_root::MARKHARNESS_DIR),
            message: e.to_string(),
        })?
        .into_iter()
        .map(|b| (b.case_uid.as_str().to_string(), b))
        .collect();

    let scope: Option<ReleaseScope> = match release_id {
        Some(id) => match release::read_scope_at(root, &at_commit, id) {
            Ok(scope) => Some(scope),
            // A release with no recorded scope is not an error: coverage
            // then answers with the registered state only (ADR 0024 §4).
            Err(release::ReleaseError::NotFound(_)) => None,
            Err(e) => return Err(e.into()),
        },
        None => None,
    };
    let selected_uids: BTreeSet<String> = scope
        .as_ref()
        .map(|scope| scope.case_uids.iter().cloned().collect())
        .unwrap_or_default();

    let mut requirements = Vec::new();
    let mut gaps = Vec::new();
    let mut in_scope_uids = BTreeSet::new();

    for (requirement_id, requirement) in selected_requirements {
        let Some(requirement_uid) = requirement.uid.clone() else {
            // Without a uid there is nothing a Feature could point at, so
            // this is the same gap as having no Feature at all.
            gaps.push(CoverageGap {
                kind: GapKind::RequirementHasNoFeature,
                requirement_id: requirement_id.clone(),
                feature_id: None,
            });
            continue;
        };
        let feature_ids = features_by_requirement
            .get(&requirement_uid)
            .cloned()
            .unwrap_or_default();
        if feature_ids.is_empty() {
            gaps.push(CoverageGap {
                kind: GapKind::RequirementHasNoFeature,
                requirement_id: requirement_id.clone(),
                feature_id: None,
            });
        }

        let mut covered_features = BTreeSet::new();
        let mut case_coverage = Vec::new();
        for case in &cases {
            // ADR 0031: matched directly against this TestCase's own
            // requirement_uids (which already resolves any Scenario-level
            // override) rather than by Feature membership — a case under a
            // Feature that `contributes_to` this Requirement is not
            // necessarily itself related to it once a sibling Scenario has
            // been given a more precise `contributes_to`.
            let related = case
                .generated_from
                .requirement_uids
                .as_ref()
                .is_some_and(|uids| uids.contains(&requirement_uid));
            if !related {
                continue;
            }
            covered_features.insert(case.generated_from.feature.clone());
            let case_uid = case.case_uid.as_ref().map(|uid| uid.to_string());
            if let Some(uid) = &case_uid {
                in_scope_uids.insert(uid.clone());
            }
            let found = case_uid.as_ref().and_then(|uid| bindings.get(uid));
            let binding_reference = found.and_then(|b| b.reference.clone());
            let reference_status = binding_reference
                .as_deref()
                .map(|reference| reference_status(root, &at_commit, reference))
                .transpose()?;
            case_coverage.push(CaseCoverage {
                case_id: case.case_id.clone(),
                case_uid: case_uid.clone(),
                feature_id: case.generated_from.feature.clone(),
                binding_mode: found.map(|b| b.mode.as_str().to_string()),
                binding_reference,
                reference_status,
                selected: scope.as_ref().map(|_| {
                    case_uid
                        .as_ref()
                        .is_some_and(|uid| selected_uids.contains(uid))
                }),
            });
        }
        case_coverage.sort_by(|a, b| a.case_id.cmp(&b.case_id));

        for feature_id in &feature_ids {
            if !covered_features.contains(feature_id) {
                gaps.push(CoverageGap {
                    kind: GapKind::FeatureHasNoCase,
                    requirement_id: requirement_id.clone(),
                    feature_id: Some(feature_id.clone()),
                });
            }
        }

        requirements.push(RequirementCoverage {
            requirement_id,
            requirement_uid: Some(requirement_uid),
            source: match requirement.source {
                RequirementSource::Native => "native",
                RequirementSource::External => "external",
            },
            feature_ids: feature_ids.into_iter().collect(),
            cases: case_coverage,
        });
    }

    let release = scope.map(|scope| {
        let known: BTreeSet<String> = cases
            .iter()
            .filter_map(|case| case.case_uid.as_ref().map(|uid| uid.to_string()))
            .collect();
        ReleaseView {
            selected_case_uids: scope
                .case_uids
                .iter()
                .filter(|uid| known.contains(*uid))
                .cloned()
                .collect(),
            absent_case_uids: scope
                .case_uids
                .iter()
                .filter(|uid| !known.contains(*uid))
                .cloned()
                .collect(),
            unselected_case_uids: in_scope_uids
                .iter()
                .filter(|uid| !selected_uids.contains(*uid))
                .cloned()
                .collect(),
            release_id: scope.release_id,
        }
    });

    Ok(ReleaseCoverage {
        schema_version: SCHEMA_VERSION,
        record_kind: RECORD_KIND,
        rule_version: RULE_VERSION,
        at_commit,
        requirements,
        gaps,
        release,
    })
}
