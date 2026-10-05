//! `traceability show`: the content of one element picked from the
//! `traceability` tree, for a detail pane (ADR 0040,
//! docs/design/cli-read-model-design.md §5.5). Like `traceability`, it builds
//! its own struct and is serialized directly by `cli.rs`.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use crate::generate::{self, KnowledgeSnapshot};
use crate::identity::CaseRevision;
use crate::knowledge::{Feature, Phase, Procedure, Requirement, RequirementSource};
use crate::knowledge_source::{KnowledgeSource, WorkingTreeKnowledgeSource};
use crate::traceability::{TraceabilityError, features_at, requirements_at};

const SCHEMA_VERSION: u32 = 1;
const RECORD_KIND: &str = "traceability_detail";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DetailElement {
    Requirement {
        uid: String,
        requirement_id: String,
        axis: Vec<String>,
        /// Omitted for `source: external`: the external document owns that
        /// content (ADR 0023).
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    Feature {
        uid: String,
        feature_id: String,
        axis: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    Behavior {
        uid: String,
        behavior_id: String,
        axis: Vec<String>,
        description: String,
        procedures: BTreeMap<String, Procedure>,
    },
    /// Phases are as written in Knowledge: `use:` steps stay unexpanded.
    Scenario {
        uid: String,
        scenario_id: String,
        description: String,
        phases: Vec<Phase>,
        #[serde(skip_serializing_if = "Option::is_none")]
        implementation_note: Option<String>,
    },
    /// Phases are what the generated TestCase actually runs: every `use:`
    /// step already replaced by its Procedure's steps.
    TestCase {
        uid: String,
        case_id: String,
        case_revision: CaseRevision,
        phases: Vec<generate::Phase>,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TraceabilityDetail {
    pub schema_version: u32,
    pub record_kind: &'static str,
    #[serde(flatten)]
    pub element: DetailElement,
}

fn requirement_detail(
    uid: &str,
    requirements: &BTreeMap<String, Requirement>,
) -> Option<DetailElement> {
    requirements
        .values()
        .find(|requirement| requirement.uid.as_deref() == Some(uid))
        .map(|requirement| DetailElement::Requirement {
            uid: uid.to_string(),
            requirement_id: requirement.id.clone(),
            axis: requirement.axis.clone(),
            description: match requirement.source {
                RequirementSource::Native => requirement.description.clone(),
                RequirementSource::External => None,
            },
        })
}

fn feature_detail(uid: &str, features: &BTreeMap<String, Feature>) -> Option<DetailElement> {
    features
        .values()
        .find(|feature| feature.uid.as_deref() == Some(uid))
        .map(|feature| DetailElement::Feature {
            uid: uid.to_string(),
            feature_id: feature.id.clone(),
            axis: feature.axis.clone(),
            description: feature.description.clone(),
        })
}

fn behavior_detail(uid: &str, snapshot: &KnowledgeSnapshot) -> Option<DetailElement> {
    snapshot
        .behaviors
        .iter()
        .find(|behavior| behavior.behavior_uid.as_deref() == Some(uid))
        .map(|behavior| DetailElement::Behavior {
            uid: uid.to_string(),
            behavior_id: behavior.behavior_id.clone(),
            axis: behavior.axis.clone(),
            description: behavior.description.clone(),
            procedures: behavior.procedures.clone(),
        })
}

fn scenario_detail(uid: &str, snapshot: &KnowledgeSnapshot) -> Option<DetailElement> {
    snapshot
        .cases
        .iter()
        .find(|case| case.scenario_uid.as_deref() == Some(uid))
        .map(|case| DetailElement::Scenario {
            uid: uid.to_string(),
            scenario_id: case.scenario_id.clone(),
            description: case.scenario_description.clone(),
            phases: case.scenario_phases.clone(),
            implementation_note: case.scenario_implementation_note.clone(),
        })
}

/// A TestCase's uid is derived from its Scenario's (ADR 0017 §3), so it is
/// matched by deriving each Scenario's case uid rather than stored anywhere.
fn test_case_detail(uid: &str, snapshot: &KnowledgeSnapshot) -> Option<DetailElement> {
    snapshot.cases.iter().find_map(|case| {
        let scenario_uid = case.scenario_uid.as_deref()?;
        (crate::identity::derived_uid::case_uid(scenario_uid) == uid).then(|| {
            DetailElement::TestCase {
                uid: uid.to_string(),
                case_id: format!(
                    "tc-{}-{}-{}",
                    case.feature_id, case.behavior_id, case.scenario_id
                ),
                case_revision: generate::compute_case_revision(&case.phases),
                phases: case.phases.clone(),
            }
        })
    })
}

/// Looks `uid` up among every element kind in the working tree. Uids are
/// unique across kinds, so at most one kind matches.
pub fn compute(root: &Path, uid: &str) -> Result<TraceabilityDetail, TraceabilityError> {
    let requirements = requirements_at(root)?;
    let features = features_at(root)?;
    let snapshot =
        WorkingTreeKnowledgeSource::new(root.join(crate::project_root::KNOWLEDGE_PATH_IN_REPO))
            .load_snapshot()?;

    let element = requirement_detail(uid, &requirements)
        .or_else(|| feature_detail(uid, &features))
        .or_else(|| behavior_detail(uid, &snapshot))
        .or_else(|| scenario_detail(uid, &snapshot))
        .or_else(|| test_case_detail(uid, &snapshot))
        .ok_or_else(|| TraceabilityError::NotFound {
            uid: uid.to_string(),
        })?;

    Ok(TraceabilityDetail {
        schema_version: SCHEMA_VERSION,
        record_kind: RECORD_KIND,
        element,
    })
}
