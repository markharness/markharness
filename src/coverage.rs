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
    let mut requirements = BTreeMap::new();
    for entry in git::ls_tree_recursive(
        root,
        git_ref,
        &format!(
            "{}/requirements",
            crate::project_root::KNOWLEDGE_PATH_IN_REPO
        ),
    )? {
        if entry.kind != git::ObjectKind::Blob || !entry.path.ends_with("/requirement.yml") {
            continue;
        }
        let content = git::show_blob_by_sha(root, &entry.sha)?;
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

/// Every Feature that names `requirement_uid`, whether or not it produces a
/// TestCase. Read from `feature.yml` directly rather than from compiled
/// TestCases, so a Feature with no Scenario underneath is still visible —
/// that absence is exactly AC21's gap.
fn features_for_requirement(
    root: &Path,
    git_ref: &str,
    requirement_uid: &str,
) -> Result<BTreeSet<String>, CoverageError> {
    let mut feature_ids = BTreeSet::new();
    for entry in git::ls_tree_recursive(
        root,
        git_ref,
        &format!("{}/features", crate::project_root::KNOWLEDGE_PATH_IN_REPO),
    )? {
        if entry.kind != git::ObjectKind::Blob || !entry.path.ends_with("/feature.yml") {
            continue;
        }
        let content = git::show_blob_by_sha(root, &entry.sha)?;
        let feature = knowledge::parse_feature(&content).map_err(|e| CoverageError::Malformed {
            path: entry.path.clone(),
            message: e.to_string(),
        })?;
        if feature
            .requirement_uids
            .iter()
            .any(|uid| uid == requirement_uid)
        {
            feature_ids.insert(feature.id);
        }
    }
    Ok(feature_ids)
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
    let bindings: BTreeMap<String, ExecutionBinding> = binding::read_all(root)
        .map_err(|e| CoverageError::Malformed {
            path: binding::bindings_dir(root)
                .to_string_lossy()
                .replace('\\', "/"),
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
        let feature_ids = features_for_requirement(root, &at_commit, &requirement_uid)?;
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
            if !feature_ids.contains(&case.generated_from.feature) {
                continue;
            }
            covered_features.insert(case.generated_from.feature.clone());
            let case_uid = case.case_uid.as_ref().map(|uid| uid.to_string());
            if let Some(uid) = &case_uid {
                in_scope_uids.insert(uid.clone());
            }
            let found = case_uid.as_ref().and_then(|uid| bindings.get(uid));
            case_coverage.push(CaseCoverage {
                case_id: case.case_id.clone(),
                case_uid: case_uid.clone(),
                feature_id: case.generated_from.feature.clone(),
                binding_mode: found.map(|b| b.mode.as_str().to_string()),
                binding_reference: found.and_then(|b| b.reference.clone()),
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
