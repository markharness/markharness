//! Change Impact over a `base..head` range (v2 design §6.1).
//!
//! Answers, for every Requirement the range touched: which Features and
//! TestCases hang off it, and whether a human confirmed the correspondence.
//! The verdict per pair is three-valued — **followed up**, **confirmed**, or
//! **unconfirmed** — and a same-PR edit to both sides is never silently
//! promoted to "confirmed" (ADR 0019).

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

use serde::Serialize;

use crate::alignment::{self, Confirmation, RejectedTrailer};
use crate::generate::{self, TestCase};
use crate::git;
use crate::knowledge::{self, Requirement, RequirementSource};
use crate::knowledge_source::{GitTreeKnowledgeSource, KnowledgeSource};

/// Bumped when a change to this module would make the same inputs produce a
/// different verdict. Emitted in the output so a recomputation can be
/// compared against the rules that produced the original (AC37).
pub const RULE_VERSION: u32 = 1;

const SCHEMA_VERSION: u32 = 1;
const RECORD_KIND: &str = "change_impact";

#[derive(Debug)]
pub enum ImpactError {
    /// AC17: the range's commits cannot be walked (a shallow clone, a ref
    /// that is not an ancestor). Reported rather than treated as "no commits
    /// in range", which would silently turn missing history into "nothing to
    /// confirm".
    HistoryUnavailable {
        base: String,
        head: String,
        detail: String,
    },
    Malformed {
        path: String,
        message: String,
    },
    Io(io::Error),
}

impl From<io::Error> for ImpactError {
    fn from(e: io::Error) -> Self {
        ImpactError::Io(e)
    }
}

impl std::fmt::Display for ImpactError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImpactError::HistoryUnavailable { base, head, detail } => write!(
                f,
                "cannot walk the commits in {base}..{head}: {detail}. Alignment checks need the full range; fetch the missing history (a shallow clone needs `git fetch --unshallow`) and retry"
            ),
            ImpactError::Malformed { path, message } => write!(f, "{path}: {message}"),
            ImpactError::Io(e) => write!(f, "filesystem error: {e}"),
        }
    }
}

/// What the range says about one (Requirement, TestCase) pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AlignmentStatus {
    /// A human recorded a `Spec-Reviewed` trailer for this exact pair, and
    /// nothing in the range has changed either side since.
    Confirmed,
    /// Both sides changed in the range, but nobody recorded a confirmation.
    /// That the TestCase moved is evidence of work, not of a human judging
    /// the two to still agree (ADR 0019).
    FollowedUp,
    /// The spec side changed and nothing here says the TestCase was
    /// reconciled with it.
    Unconfirmed,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseAlignment {
    pub case_id: String,
    pub case_uid: Option<String>,
    pub case_changed: bool,
    pub status: AlignmentStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct RequirementImpact {
    pub requirement_id: String,
    pub requirement_uid: Option<String>,
    pub source: &'static str,
    /// Whether the spec side itself changed between base and head.
    pub spec_changed: bool,
    pub feature_ids: Vec<String>,
    pub cases: Vec<CaseAlignment>,
}

/// An external Requirement whose pinned revision is behind the `.sdoc` as
/// head records it. Reported separately from a spec change: a stale pin is
/// bookkeeping, not evidence that the requirement's text moved in this range
/// (AC10c).
#[derive(Debug, Clone, Serialize)]
pub struct StalePin {
    pub requirement_id: String,
    pub source_locator: String,
    pub pinned_revision: String,
    pub head_revision: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RejectedTrailerReport {
    pub commit: String,
    pub line: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChangeImpact {
    pub schema_version: u32,
    pub record_kind: &'static str,
    pub rule_version: u32,
    pub base_commit: String,
    pub head_commit: String,
    pub requirements: Vec<RequirementImpact>,
    pub stale_pins: Vec<StalePin>,
    /// Trailers that were written but could not be counted, with why. A
    /// silently dropped trailer would look identical to never having written
    /// one.
    pub rejected_trailers: Vec<RejectedTrailerReport>,
}

impl ChangeImpact {
    /// Whether anything here warrants a human's attention — what
    /// `--fail-on-findings` turns into a non-zero exit.
    pub fn has_findings(&self) -> bool {
        self.requirements.iter().any(|requirement| {
            requirement
                .cases
                .iter()
                .any(|case| case.status != AlignmentStatus::Confirmed)
        }) || !self.stale_pins.is_empty()
            || !self.rejected_trailers.is_empty()
    }
}

/// Every Requirement at `git_ref`, keyed by display id.
fn requirements_at(
    root: &Path,
    git_ref: &str,
) -> Result<BTreeMap<String, Requirement>, ImpactError> {
    let mut requirements = BTreeMap::new();
    let entries = git::ls_tree_recursive(
        root,
        git_ref,
        &format!(
            "{}/requirements",
            crate::project_root::KNOWLEDGE_PATH_IN_REPO
        ),
    )?;
    for entry in entries {
        if entry.kind != git::ObjectKind::Blob || !entry.path.ends_with("/requirement.yml") {
            continue;
        }
        let content = git::show_blob_by_sha(root, &entry.sha)?;
        let requirement =
            knowledge::parse_requirement(&content).map_err(|e| ImpactError::Malformed {
                path: entry.path.clone(),
                message: e.to_string(),
            })?;
        requirements.insert(requirement.id.clone(), requirement);
    }
    Ok(requirements)
}

/// The TestCases `git_ref` would generate, keyed by `case_id`.
fn cases_at(root: &Path, git_ref: &str) -> Result<BTreeMap<String, TestCase>, ImpactError> {
    let snapshot = GitTreeKnowledgeSource::new(root, git_ref).load_snapshot()?;
    Ok(generate::compile_testcases(&snapshot)
        .into_iter()
        .map(|case| (case.case_id.clone(), case))
        .collect())
}

/// The blob a Requirement's content effectively lives in, per mode: the
/// `requirement.yml` itself for native, the `.sdoc` it pins for external
/// (ADR 0023 §2/§3).
fn spec_blob_path(requirement: &Requirement, requirement_id: &str) -> String {
    match requirement.source {
        RequirementSource::Native => format!(
            "{}/requirements/{requirement_id}/requirement.yml",
            crate::project_root::KNOWLEDGE_PATH_IN_REPO
        ),
        RequirementSource::External => requirement.source_locator.clone().unwrap_or_default(),
    }
}

/// Requirement UIDs and Case UIDs whose effective content differs between
/// two refs. Used both for the base..head verdict and, per commit, for the
/// invalidation rule in §5.3.
fn changed_between(
    root: &Path,
    from: &str,
    to: &str,
) -> Result<crate::alignment::ChangedUids, ImpactError> {
    let from_requirements = requirements_at(root, from)?;
    let to_requirements = requirements_at(root, to)?;
    let mut changed_requirements = BTreeSet::new();
    for (id, requirement) in &to_requirements {
        let Some(uid) = requirement.uid.clone() else {
            continue;
        };
        let path = spec_blob_path(requirement, id);
        if path.is_empty() {
            continue;
        }
        let before = git::blob_sha_at(root, from, &path)?;
        let after = git::blob_sha_at(root, to, &path)?;
        // A Requirement that did not exist at `from` counts as changed: its
        // spec content is new to this range.
        let existed = from_requirements.contains_key(id);
        if before != after || !existed {
            changed_requirements.insert(uid);
        }
    }

    let from_cases = cases_at(root, from)?;
    let to_cases = cases_at(root, to)?;
    let mut changed_cases = BTreeSet::new();
    for (case_id, case) in &to_cases {
        let Some(uid) = case.case_uid.as_ref() else {
            continue;
        };
        let changed = match from_cases.get(case_id) {
            // Case revision is derived from the expanded phases alone, so it
            // is exactly "did what this case verifies change" (ADR 0017 §3).
            Some(before) => before.case_revision != case.case_revision,
            None => true,
        };
        if changed {
            changed_cases.insert(uid.to_string());
        }
    }
    Ok((changed_requirements, changed_cases))
}

/// Resolves one commit's trailers into pair confirmations, using the display
/// ids as they stood **at that commit** (ADR 0019). A trailer naming an id
/// that does not resolve there, or a case with no `case_uid` yet, is not
/// counted — an unresolvable trailer cannot identify a pair.
fn confirmations_at_commit(
    root: &Path,
    commit: &str,
) -> Result<(Vec<Confirmation>, Vec<RejectedTrailerReport>), ImpactError> {
    let message = git::commit_message(root, commit)?;
    let (parsed, rejected) = alignment::parse_trailers(&message);
    let mut reports: Vec<RejectedTrailerReport> = rejected
        .into_iter()
        .map(|RejectedTrailer { line, reason }| RejectedTrailerReport {
            commit: commit.to_string(),
            line,
            reason: reason.to_string(),
        })
        .collect();
    if parsed.is_empty() {
        return Ok((Vec::new(), reports));
    }

    let requirements = requirements_at(root, commit)?;
    let cases = cases_at(root, commit)?;
    let mut confirmations = Vec::new();
    for trailer in parsed {
        let requirement_uid = requirements
            .get(&trailer.requirement_id)
            .and_then(|requirement| requirement.uid.clone());
        let case_uid = cases
            .get(&trailer.case_id)
            .and_then(|case| case.case_uid.as_ref().map(|uid| uid.to_string()));
        match (requirement_uid, case_uid) {
            (Some(requirement_uid), Some(case_uid)) => confirmations.push(Confirmation {
                commit: commit.to_string(),
                requirement_uid,
                case_uid,
            }),
            _ => reports.push(RejectedTrailerReport {
                commit: commit.to_string(),
                line: format!(
                    "{}: requirement={} case={}",
                    alignment::TRAILER_KEY,
                    trailer.requirement_id,
                    trailer.case_id
                ),
                reason: "requirement or case could not be resolved to a uid at this commit"
                    .to_string(),
            }),
        }
    }
    Ok((confirmations, reports))
}

/// Computes Change Impact for `base..head`.
pub fn compute(root: &Path, base: &str, head: &str) -> Result<ChangeImpact, ImpactError> {
    if git::is_shallow(root) {
        return Err(ImpactError::HistoryUnavailable {
            base: base.to_string(),
            head: head.to_string(),
            detail: "this is a shallow clone".to_string(),
        });
    }
    let base_commit =
        git::resolve_commit_oid(root, base).map_err(|e| ImpactError::HistoryUnavailable {
            base: base.to_string(),
            head: head.to_string(),
            detail: e.to_string(),
        })?;
    let head_commit =
        git::resolve_commit_oid(root, head).map_err(|e| ImpactError::HistoryUnavailable {
            base: base.to_string(),
            head: head.to_string(),
            detail: e.to_string(),
        })?;
    let commits = git::commits_in_range(root, &base_commit, &head_commit).map_err(|e| {
        ImpactError::HistoryUnavailable {
            base: base.to_string(),
            head: head.to_string(),
            detail: e.to_string(),
        }
    })?;

    let mut confirmations = Vec::new();
    let mut rejected_trailers = Vec::new();
    for commit in &commits {
        let (found, rejected) = confirmations_at_commit(root, commit)?;
        confirmations.extend(found);
        rejected_trailers.extend(rejected);
    }

    // Per-commit change sets, for §5.3's invalidation rule. Computed once
    // here rather than inside the closure so the same commit is never
    // diffed twice.
    let mut per_commit: BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)> = BTreeMap::new();
    if !confirmations.is_empty() {
        for commit in &commits {
            let parents = git::parents(root, commit)?;
            let Some(parent) = parents.first() else {
                continue;
            };
            per_commit.insert(commit.clone(), changed_between(root, parent, commit)?);
        }
    }
    let surviving = alignment::surviving_confirmations(&commits, &confirmations, &|commit| {
        per_commit.get(commit).cloned().unwrap_or_default()
    });
    let confirmed_pairs: BTreeSet<(String, String)> = surviving
        .into_iter()
        .map(|confirmation| (confirmation.requirement_uid, confirmation.case_uid))
        .collect();

    let (changed_requirements, changed_cases) = changed_between(root, &base_commit, &head_commit)?;
    let head_requirements = requirements_at(root, &head_commit)?;
    let head_cases = cases_at(root, &head_commit)?;

    let mut requirements = Vec::new();
    let mut stale_pins = Vec::new();
    for (requirement_id, requirement) in &head_requirements {
        if let (RequirementSource::External, Some(locator), Some(pinned)) = (
            requirement.source,
            requirement.source_locator.as_ref(),
            requirement.source_revision.as_ref(),
        ) && let Some(head_revision) = git::blob_sha_at(root, &head_commit, locator)?
            && &head_revision != pinned
        {
            stale_pins.push(StalePin {
                requirement_id: requirement_id.clone(),
                source_locator: locator.clone(),
                pinned_revision: pinned.clone(),
                head_revision,
            });
        }

        let Some(requirement_uid) = requirement.uid.clone() else {
            continue;
        };
        let spec_changed = changed_requirements.contains(&requirement_uid);
        if !spec_changed {
            continue;
        }

        // Reverse lookup: a Requirement does not record its Features, the
        // Features record it (ADR 0017 §1/§3), so the relation is read from
        // the case side.
        let mut feature_ids = BTreeSet::new();
        let mut cases = Vec::new();
        for case in head_cases.values() {
            let related = case
                .generated_from
                .requirement_uids
                .as_ref()
                .is_some_and(|uids| uids.contains(&requirement_uid));
            if !related {
                continue;
            }
            feature_ids.insert(case.generated_from.feature.clone());
            let case_uid = case.case_uid.as_ref().map(|uid| uid.to_string());
            let case_changed = case_uid
                .as_ref()
                .is_some_and(|uid| changed_cases.contains(uid));
            let confirmed = case_uid.as_ref().is_some_and(|uid| {
                confirmed_pairs.contains(&(requirement_uid.clone(), uid.clone()))
            });
            let status = if confirmed {
                AlignmentStatus::Confirmed
            } else if case_changed {
                AlignmentStatus::FollowedUp
            } else {
                AlignmentStatus::Unconfirmed
            };
            cases.push(CaseAlignment {
                case_id: case.case_id.clone(),
                case_uid,
                case_changed,
                status,
            });
        }
        cases.sort_by(|a, b| a.case_id.cmp(&b.case_id));

        requirements.push(RequirementImpact {
            requirement_id: requirement_id.clone(),
            requirement_uid: Some(requirement_uid),
            source: match requirement.source {
                RequirementSource::Native => "native",
                RequirementSource::External => "external",
            },
            spec_changed,
            feature_ids: feature_ids.into_iter().collect(),
            cases,
        });
    }

    Ok(ChangeImpact {
        schema_version: SCHEMA_VERSION,
        record_kind: RECORD_KIND,
        rule_version: RULE_VERSION,
        base_commit,
        head_commit,
        requirements,
        stale_pins,
        rejected_trailers,
    })
}
