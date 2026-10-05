//! `traceability`: a read-only view of Requirement/Feature/Behavior/Scenario/
//! TestCase relations for external tools such as markharness-view (ADR 0032,
//! docs/design/cli-read-model-design.md §5). Never goes through
//! `CommandOutcome`/`Presenter` — like `impact`/`coverage`, it builds its own
//! struct and is serialized directly by `cli.rs`.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use serde::Serialize;

use crate::generate::{self, KnowledgeBehaviorSnapshot, KnowledgeCaseSnapshot, TestCase};
use crate::identity::{CaseRevision, CaseUid};
use crate::knowledge::{self, Feature, Requirement, RequirementSource};
use crate::knowledge_source::{KnowledgeSource, WorkingTreeKnowledgeSource};

const SCHEMA_VERSION: u32 = 1;
const RECORD_KIND: &str = "traceability";

#[derive(Debug)]
pub enum TraceabilityError {
    Malformed {
        path: String,
        message: String,
    },
    /// `traceability show --uid` named a uid that no element has.
    NotFound {
        uid: String,
    },
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
            TraceabilityError::NotFound { uid } => write!(f, "no element has uid {uid}"),
            TraceabilityError::Io(e) => write!(f, "filesystem error: {e}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RequirementNode {
    pub requirement_id: String,
    pub requirement_uid: Option<String>,
    pub source: &'static str,
    /// Present only for `source: "native"`. Never given a representative
    /// text for `"external"`, since markharness doesn't own that content
    /// (ADR 0023).
    pub label: Option<String>,
    /// Present only when `source` is `"external"` (ADR 0023): lets a reader
    /// reach the underlying StrictDoc content. Always `None` for `"native"`.
    pub source_locator: Option<String>,
    /// StrictDoc's MID (ADR 0030). Always `None` for `"native"`.
    pub source_key: Option<String>,
    /// The `case_uid` of every TestCase that relates to this Requirement,
    /// by the same rule `coverage` uses (`generate::testcases_for_requirement`):
    /// a Scenario naming a Requirement of its own replaces its Feature's, so
    /// this is not the union of the Feature and Scenario relations. Sorted;
    /// empty while the Requirement has no uid. A case with no `case_uid` yet
    /// (its Scenario is not migrated) cannot be listed.
    pub case_uids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FeatureNode {
    pub feature_id: String,
    pub feature_uid: Option<String>,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BehaviorNode {
    pub behavior_id: String,
    /// `None` until `identity migrate` assigns one.
    pub behavior_uid: Option<String>,
    pub feature_id: String,
    /// `None` while the parent Feature has no uid (not migrated).
    pub feature_uid: Option<String>,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ScenarioNode {
    pub scenario_id: String,
    pub scenario_uid: Option<String>,
    pub behavior_id: String,
    /// `None` while the parent Behavior has no uid (not migrated).
    pub behavior_uid: Option<String>,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TestCaseNode {
    pub case_id: String,
    pub case_uid: Option<CaseUid>,
    pub case_revision: CaseRevision,
    pub relative_path: String,
    pub scenario_id: String,
    /// `None` while the parent Scenario has no uid (not migrated).
    pub scenario_uid: Option<String>,
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
    pub requirements: Vec<RequirementNode>,
    pub features: Vec<FeatureNode>,
    pub behaviors: Vec<BehaviorNode>,
    pub scenarios: Vec<ScenarioNode>,
    pub test_cases: Vec<TestCaseNode>,
    pub relations: Vec<TraceabilityRelation>,
}

/// Every Requirement in the working tree, keyed by display id.
pub(crate) fn requirements_at(
    root: &Path,
) -> Result<BTreeMap<String, Requirement>, TraceabilityError> {
    let mut requirements = BTreeMap::new();
    let requirements_dir = root
        .join(crate::project_root::KNOWLEDGE_PATH_IN_REPO)
        .join("requirements");
    for dir in generate::sorted_subdirs(&requirements_dir)? {
        let path = dir.join("requirement.yml");
        if !path.is_file() {
            continue;
        }
        let content = std::fs::read_to_string(&path)?;
        let requirement =
            knowledge::parse_requirement(&content).map_err(|e| TraceabilityError::Malformed {
                path: repo_relative_path(root, &path),
                message: e.to_string(),
            })?;
        check_requirement_source_mode(&requirement, &repo_relative_path(root, &path))?;
        requirements.insert(requirement.id.clone(), requirement);
    }
    Ok(requirements)
}

/// Rejects a Requirement whose fields disagree with its `source` (ADR 0023:
/// each mode owns a disjoint set of fields). Mirrors
/// `validate::check_requirement_source_mode` in full, not just the two
/// fields (`source_locator`/`source_key`) `RequirementNode` happens to
/// expose: `traceability` cannot assume `validate` has already run against
/// every commit it might be asked to read, and a Requirement that would fail
/// `validate` should not be treated as well-formed here either. Passing
/// through a native Requirement that also carries `source_locator`/
/// `source_key` in particular would let a reader (e.g. markharness-view)
/// follow a stale or unrelated external reference for what is actually
/// native content — the concrete case this exists to prevent — but the
/// other disjoint fields are checked too, so a Requirement `validate` would
/// reject never gets treated as clean here by coincidence.
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
            if requirement.label.is_none() {
                return Err(malformed(
                    "source: native requires `label` (markharness owns the content)",
                ));
            }
            if requirement.source_locator.is_some() {
                return Err(malformed(
                    "source: native must not carry `source_locator` (that belongs to source: external)",
                ));
            }
            if requirement.source_revision.is_some() {
                return Err(malformed(
                    "source: native must not carry `source_revision` (that belongs to source: external)",
                ));
            }
            if requirement.source_key.is_some() {
                return Err(malformed(
                    "source: native must not carry `source_key` (that belongs to source: external)",
                ));
            }
        }
        RequirementSource::External => {
            if requirement.source_locator.is_none() {
                return Err(malformed(
                    "source: external requires `source_locator` (the path of the StrictDoc source file, .sdoc or .md, in this repository)",
                ));
            }
            if requirement.source_revision.is_none() {
                return Err(malformed(
                    "source: external requires `source_revision` (the pinned blob OID of that source file)",
                ));
            }
            if requirement.source_key.is_none() {
                return Err(malformed(
                    "source: external requires `source_key` (StrictDoc's own MID, held verbatim)",
                ));
            }
            if requirement.label.is_some() {
                return Err(malformed(
                    "source: external must not carry `label` — the external document owns the content",
                ));
            }
            if requirement.description.is_some() {
                return Err(malformed(
                    "source: external must not carry `description` — the external document owns the content",
                ));
            }
        }
    }
    Ok(())
}

/// Every Feature in the working tree, keyed by display id. Read directly
/// rather than derived from generated TestCases, so a Feature with no
/// Behavior/Scenario underneath is still visible (the same gap coverage's
/// AC21 cares about).
pub(crate) fn features_at(root: &Path) -> Result<BTreeMap<String, Feature>, TraceabilityError> {
    let mut features = BTreeMap::new();
    let features_dir = root
        .join(crate::project_root::KNOWLEDGE_PATH_IN_REPO)
        .join("features");
    for dir in generate::sorted_subdirs(&features_dir)? {
        let path = dir.join("feature.yml");
        if !path.is_file() {
            continue;
        }
        let content = std::fs::read_to_string(&path)?;
        let feature =
            knowledge::parse_feature(&content).map_err(|e| TraceabilityError::Malformed {
                path: repo_relative_path(root, &path),
                message: e.to_string(),
            })?;
        features.insert(feature.id.clone(), feature);
    }
    Ok(features)
}

/// Formats `path` for error messages: repo-relative, forward-slashed.
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
    requirements: &BTreeMap<String, Requirement>,
    features: &BTreeMap<String, Feature>,
    behaviors: &[KnowledgeBehaviorSnapshot],
    cases: &[KnowledgeCaseSnapshot],
    testcases: &[TestCase],
) -> TraceabilityReadModel {
    let requirement_nodes = requirements
        .values()
        .map(|requirement| RequirementNode {
            case_uids: requirement
                .uid
                .as_deref()
                .map(|uid| {
                    let mut case_uids: Vec<String> =
                        generate::testcases_for_requirement(testcases, uid)
                            .filter_map(|case| case.case_uid.as_ref().map(|uid| uid.to_string()))
                            .collect();
                    case_uids.sort();
                    case_uids
                })
                .unwrap_or_default(),
            requirement_id: requirement.id.clone(),
            requirement_uid: requirement.uid.clone(),
            source: match requirement.source {
                RequirementSource::Native => "native",
                RequirementSource::External => "external",
            },
            label: requirement.label.clone(),
            source_locator: requirement.source_locator.clone(),
            source_key: requirement.source_key.clone(),
        })
        .collect();

    let feature_nodes: Vec<FeatureNode> = features
        .values()
        .map(|feature| FeatureNode {
            feature_id: feature.id.clone(),
            feature_uid: feature.uid.clone(),
            label: feature.label.clone(),
        })
        .collect();

    let mut behavior_nodes: Vec<BehaviorNode> = behaviors
        .iter()
        .map(|behavior| BehaviorNode {
            behavior_id: behavior.behavior_id.clone(),
            behavior_uid: behavior.behavior_uid.clone(),
            feature_id: behavior.feature_id.clone(),
            feature_uid: behavior.feature_uid.clone(),
            label: behavior.label.clone(),
        })
        .collect();
    behavior_nodes
        .sort_by(|a, b| (&a.feature_id, &a.behavior_id).cmp(&(&b.feature_id, &b.behavior_id)));
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
                behavior_uid: case.behavior_uid.clone(),
                label: case.scenario_label.clone(),
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
            scenario_uid: case.scenario_uid.clone(),
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
        requirements: requirement_nodes,
        features: feature_nodes,
        behaviors: behavior_nodes,
        scenarios: scenarios.into_values().collect(),
        test_cases,
        relations,
    }
}

/// Computes the Traceability read model from the working tree. Uncommitted
/// edits are included; a committed state is read through `coverage --at`.
pub fn compute(root: &Path) -> Result<TraceabilityReadModel, TraceabilityError> {
    let requirements = requirements_at(root)?;
    let features = features_at(root)?;
    let snapshot =
        WorkingTreeKnowledgeSource::new(root.join(crate::project_root::KNOWLEDGE_PATH_IN_REPO))
            .load_snapshot()?;
    Ok(build(
        &requirements,
        &features,
        &snapshot.behaviors,
        &snapshot.cases,
        &generate::compile_testcases(&snapshot),
    ))
}
