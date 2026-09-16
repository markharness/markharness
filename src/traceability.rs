//! `traceability`: a read-only view of Requirement/Feature/Behavior/Scenario/
//! TestCase relations for external tools such as markharness-view (ADR 0032,
//! docs/design/cli-read-model-design.md §5). Never goes through
//! `CommandOutcome`/`Presenter` — like `impact`/`coverage`, it builds its own
//! struct and is serialized directly by `cli.rs`.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use serde::Serialize;

use crate::generate::{self, KnowledgeCaseSnapshot};
use crate::git;
use crate::identity::{CaseRevision, CaseUid};
use crate::knowledge::{self, Feature, Requirement, RequirementSource};
use crate::knowledge_source::{
    GitTreeKnowledgeSource, KnowledgeSource, WorkingTreeKnowledgeSource,
};

const SCHEMA_VERSION: u32 = 1;
const RECORD_KIND: &str = "traceability";
/// `at`'s value when `--at` is omitted (ADR 0033). Reserved the same way
/// `HEAD` effectively is: a real Git ref sharing this exact name would
/// collide, an accepted practical risk rather than something worth guarding
/// against.
const WORKING_TREE: &str = "working-tree";

#[derive(Debug)]
pub enum TraceabilityError {
    Malformed { path: String, message: String },
    Io(io::Error),
}

impl From<io::Error> for TraceabilityError {
    fn from(e: io::Error) -> Self {
        TraceabilityError::Io(e)
    }
}

impl std::fmt::Display for TraceabilityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TraceabilityError::Malformed { path, message } => write!(f, "{path}: {message}"),
            TraceabilityError::Io(e) => write!(f, "filesystem error: {e}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RequirementNode {
    pub requirement_id: String,
    pub requirement_uid: Option<String>,
    pub source: &'static str,
    /// Present only when `source` is `"external"` (ADR 0023): lets a reader
    /// reach the underlying StrictDoc content. Always `None` for `"native"`.
    pub source_locator: Option<String>,
    /// StrictDoc's MID (ADR 0030). Always `None` for `"native"`.
    pub source_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FeatureNode {
    pub feature_id: String,
    pub feature_uid: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BehaviorNode {
    pub behavior_id: String,
    /// `None` until `identity migrate` assigns one; the fixture data behind
    /// this read model does not carry a Behavior UID today (only
    /// `Behavior.uid` in `knowledge/`, not yet threaded through
    /// `KnowledgeCaseSnapshot`), so this is always `None` in the current
    /// implementation. Kept as a field (rather than omitted) so a future
    /// implementation can populate it without a schema_version bump.
    pub behavior_uid: Option<String>,
    pub feature_id: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ScenarioNode {
    pub scenario_id: String,
    pub scenario_uid: Option<String>,
    pub behavior_id: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TestCaseNode {
    pub case_id: String,
    pub case_uid: Option<CaseUid>,
    pub case_revision: CaseRevision,
    pub relative_path: String,
    pub scenario_id: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    /// A Feature or Scenario contributes to a Requirement.
    ContributesTo,
    /// A TestCase was generated from a Scenario.
    GeneratedFrom,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TraceabilityRelation {
    pub from_uid: String,
    pub to_uid: String,
    pub kind: RelationKind,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TraceabilityReadModel {
    pub schema_version: u32,
    pub record_kind: &'static str,
    pub at: String,
    pub requirements: Vec<RequirementNode>,
    pub features: Vec<FeatureNode>,
    pub behaviors: Vec<BehaviorNode>,
    pub scenarios: Vec<ScenarioNode>,
    pub test_cases: Vec<TestCaseNode>,
    pub relations: Vec<TraceabilityRelation>,
}

/// Every Requirement at `git_ref` (the working tree when `None`, ADR 0033),
/// keyed by display id. The Git-ref branch mirrors
/// `impact::requirements_at`/`coverage::requirements_at`.
fn requirements_at(
    root: &Path,
    git_ref: Option<&str>,
) -> Result<BTreeMap<String, Requirement>, TraceabilityError> {
    let mut requirements = BTreeMap::new();
    match git_ref {
        Some(git_ref) => {
            for entry in git::ls_tree_recursive(
                root,
                git_ref,
                &format!(
                    "{}/requirements",
                    crate::project_root::KNOWLEDGE_PATH_IN_REPO
                ),
            )? {
                if entry.kind != git::ObjectKind::Blob || !entry.path.ends_with("/requirement.yml")
                {
                    continue;
                }
                let content = git::show_blob_by_sha(root, &entry.sha)?;
                let requirement = knowledge::parse_requirement(&content).map_err(|e| {
                    TraceabilityError::Malformed {
                        path: entry.path.clone(),
                        message: e.to_string(),
                    }
                })?;
                check_requirement_source_mode(&requirement, &entry.path)?;
                requirements.insert(requirement.id.clone(), requirement);
            }
        }
        None => {
            let requirements_dir = root
                .join(crate::project_root::KNOWLEDGE_PATH_IN_REPO)
                .join("requirements");
            for dir in generate::sorted_subdirs(&requirements_dir)? {
                let path = dir.join("requirement.yml");
                if !path.is_file() {
                    continue;
                }
                let content = std::fs::read_to_string(&path)?;
                let requirement = knowledge::parse_requirement(&content).map_err(|e| {
                    TraceabilityError::Malformed {
                        path: repo_relative_path(root, &path),
                        message: e.to_string(),
                    }
                })?;
                check_requirement_source_mode(&requirement, &repo_relative_path(root, &path))?;
                requirements.insert(requirement.id.clone(), requirement);
            }
        }
    }
    Ok(requirements)
}

/// Rejects a Requirement whose `source_locator`/`source_key` disagree with
/// its `source` (ADR 0023). `traceability` exposes both fields verbatim
/// (`RequirementNode`), so passing through a native Requirement that also
/// carries them would let a reader (e.g. markharness-view) follow a stale or
/// unrelated external reference for what is actually native content.
/// `validate` catches this too, but `traceability` cannot assume `validate`
/// has run against every commit it might be asked to read.
fn check_requirement_source_mode(
    requirement: &Requirement,
    path: &str,
) -> Result<(), TraceabilityError> {
    let malformed = |message: &str| TraceabilityError::Malformed {
        path: path.to_string(),
        message: message.to_string(),
    };
    match requirement.source {
        RequirementSource::Native => {
            if requirement.source_locator.is_some() || requirement.source_key.is_some() {
                return Err(malformed(
                    "source: native must not carry `source_locator`/`source_key` (those belong to source: external)",
                ));
            }
        }
        RequirementSource::External => {
            if requirement.source_locator.is_none() || requirement.source_key.is_none() {
                return Err(malformed(
                    "source: external requires both `source_locator` and `source_key`",
                ));
            }
        }
    }
    Ok(())
}

/// Every Feature at `git_ref` (the working tree when `None`, ADR 0033),
/// keyed by display id. Read directly (like
/// `coverage::features_for_requirement`) rather than derived from generated
/// TestCases, so a Feature with no Behavior/Scenario underneath is still
/// visible (the same gap coverage's AC21 cares about).
fn features_at(
    root: &Path,
    git_ref: Option<&str>,
) -> Result<BTreeMap<String, Feature>, TraceabilityError> {
    let mut features = BTreeMap::new();
    match git_ref {
        Some(git_ref) => {
            for entry in git::ls_tree_recursive(
                root,
                git_ref,
                &format!("{}/features", crate::project_root::KNOWLEDGE_PATH_IN_REPO),
            )? {
                if entry.kind != git::ObjectKind::Blob || !entry.path.ends_with("/feature.yml") {
                    continue;
                }
                let content = git::show_blob_by_sha(root, &entry.sha)?;
                let feature = knowledge::parse_feature(&content).map_err(|e| {
                    TraceabilityError::Malformed {
                        path: entry.path.clone(),
                        message: e.to_string(),
                    }
                })?;
                features.insert(feature.id.clone(), feature);
            }
        }
        None => {
            let features_dir = root
                .join(crate::project_root::KNOWLEDGE_PATH_IN_REPO)
                .join("features");
            for dir in generate::sorted_subdirs(&features_dir)? {
                let path = dir.join("feature.yml");
                if !path.is_file() {
                    continue;
                }
                let content = std::fs::read_to_string(&path)?;
                let feature = knowledge::parse_feature(&content).map_err(|e| {
                    TraceabilityError::Malformed {
                        path: repo_relative_path(root, &path),
                        message: e.to_string(),
                    }
                })?;
                features.insert(feature.id.clone(), feature);
            }
        }
    }
    Ok(features)
}

/// Formats `path` the same way Git-ref reads report one: repo-relative,
/// forward-slashed. Keeps error messages consistent regardless of which
/// source (`--at <ref>` or the working tree) produced them.
fn repo_relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn push_relation(
    relations: &mut Vec<TraceabilityRelation>,
    from: Option<&str>,
    to: Option<&str>,
    kind: RelationKind,
) {
    // A relation names both sides by UID; an entity with no UID yet (not
    // migrated) cannot appear in one, the same way it cannot appear in any
    // other UID-keyed contract this codebase produces.
    if let (Some(from), Some(to)) = (from, to) {
        relations.push(TraceabilityRelation {
            from_uid: from.to_string(),
            to_uid: to.to_string(),
            kind,
        });
    }
}

/// Builds the read model from already-loaded Knowledge, independent of how
/// it was loaded (kept separate from `compute` so the assembly logic can be
/// exercised without a Git fixture).
fn build(
    at: String,
    requirements: &BTreeMap<String, Requirement>,
    features: &BTreeMap<String, Feature>,
    cases: &[KnowledgeCaseSnapshot],
) -> TraceabilityReadModel {
    let requirement_nodes = requirements
        .values()
        .map(|requirement| RequirementNode {
            requirement_id: requirement.id.clone(),
            requirement_uid: requirement.uid.clone(),
            source: match requirement.source {
                RequirementSource::Native => "native",
                RequirementSource::External => "external",
            },
            source_locator: requirement.source_locator.clone(),
            source_key: requirement.source_key.clone(),
        })
        .collect();

    let feature_nodes: Vec<FeatureNode> = features
        .values()
        .map(|feature| FeatureNode {
            feature_id: feature.id.clone(),
            feature_uid: feature.uid.clone(),
        })
        .collect();

    let mut behaviors: BTreeMap<(String, String), BehaviorNode> = BTreeMap::new();
    let mut scenarios: BTreeMap<(String, String, String), ScenarioNode> = BTreeMap::new();
    let mut test_cases: Vec<TestCaseNode> = Vec::new();
    let mut relations: Vec<TraceabilityRelation> = Vec::new();

    for feature in features.values() {
        for requirement_uid in &feature.requirement_uids {
            push_relation(
                &mut relations,
                feature.uid.as_deref(),
                Some(requirement_uid),
                RelationKind::ContributesTo,
            );
        }
    }

    for case in cases {
        behaviors
            .entry((case.feature_id.clone(), case.behavior_id.clone()))
            .or_insert_with(|| BehaviorNode {
                behavior_id: case.behavior_id.clone(),
                behavior_uid: None,
                feature_id: case.feature_id.clone(),
            });
        scenarios
            .entry((
                case.feature_id.clone(),
                case.behavior_id.clone(),
                case.scenario_id.clone(),
            ))
            .or_insert_with(|| ScenarioNode {
                scenario_id: case.scenario_id.clone(),
                scenario_uid: case.scenario_uid.clone(),
                behavior_id: case.behavior_id.clone(),
            });

        let case_id = format!(
            "tc-{}-{}-{}",
            case.feature_id, case.behavior_id, case.scenario_id
        );
        let case_uid = case.scenario_uid.as_deref().map(|scenario_uid| {
            CaseUid::new(crate::identity::derived_uid::case_uid(scenario_uid))
                .expect("derived_uid::case_uid always formats a valid CaseUid")
        });
        test_cases.push(TestCaseNode {
            case_id,
            case_uid: case_uid.clone(),
            case_revision: generate::compute_case_revision(&case.phases),
            relative_path: Path::new(&case.feature_id)
                .join(&case.behavior_id)
                .join(format!("{}.yml", case.scenario_id))
                .to_string_lossy()
                .replace('\\', "/"),
            scenario_id: case.scenario_id.clone(),
        });

        for requirement_uid in &case.requirement_uids {
            push_relation(
                &mut relations,
                case.scenario_uid.as_deref(),
                Some(requirement_uid),
                RelationKind::ContributesTo,
            );
        }
        push_relation(
            &mut relations,
            case_uid.as_ref().map(|uid| uid.as_str()),
            case.scenario_uid.as_deref(),
            RelationKind::GeneratedFrom,
        );
    }

    TraceabilityReadModel {
        schema_version: SCHEMA_VERSION,
        record_kind: RECORD_KIND,
        at,
        requirements: requirement_nodes,
        features: feature_nodes,
        behaviors: behaviors.into_values().collect(),
        scenarios: scenarios.into_values().collect(),
        test_cases,
        relations,
    }
}

/// Computes the Traceability read model. `git_ref` reads that Git revision;
/// `None` reads the working tree instead (ADR 0033) — unlike `impact`
/// (`base..head`) and `coverage` (release auditing), `traceability` has no
/// requirement that its input already be committed.
pub fn compute(
    root: &Path,
    git_ref: Option<&str>,
) -> Result<TraceabilityReadModel, TraceabilityError> {
    let requirements = requirements_at(root, git_ref)?;
    let features = features_at(root, git_ref)?;
    let snapshot = match git_ref {
        Some(git_ref) => GitTreeKnowledgeSource::new(root, git_ref).load_snapshot()?,
        None => {
            WorkingTreeKnowledgeSource::new(root.join(crate::project_root::KNOWLEDGE_PATH_IN_REPO))
                .load_snapshot()?
        }
    };
    let at = git_ref.map_or_else(|| WORKING_TREE.to_string(), |git_ref| git_ref.to_string());
    Ok(build(at, &requirements, &features, &snapshot.cases))
}
