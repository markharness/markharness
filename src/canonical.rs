use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{generate, git, id_cache};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Feature,
    Condition,
    TestCase,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactVersion {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_oid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canonical_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Provenance {
    pub importer: String,
    pub importer_version: String,
    pub source_locator: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CanonicalArtifact {
    pub canonical_id: String,
    pub source: String,
    pub external_id: String,
    pub kind: ArtifactKind,
    pub version: ArtifactVersion,
    pub provenance: Provenance,
    /// The artifact's immutable identity (ADR 0013), when the source
    /// system tracks one. `external_id` still reflects the current,
    /// human-readable id — `uid` lets a consumer correlate this artifact
    /// across snapshots even after `external_id` changes (a rename).
    /// `None` for artifact kinds that don't carry a `uid` yet (Condition
    /// and TestCase, until Phase 3/4 of ADR 0013's migration) and for
    /// importers (e.g. junit) with no concept of one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum RelationOriginKind {
    Stored,
    Derived,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelationOrigin {
    pub kind: RelationOriginKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CanonicalRelation {
    pub from: String,
    pub relation_type: String,
    pub to: String,
    pub origin: RelationOrigin,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CanonicalSnapshot {
    pub schema_version: u32,
    pub artifacts: Vec<CanonicalArtifact>,
    pub relations: Vec<CanonicalRelation>,
    pub evidence: Vec<CanonicalEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CanonicalEvidence {
    pub test_id: String,
    pub result: EvidenceResult,
    pub executed_at: Option<String>,
    pub bound_versions: BTreeMap<String, String>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceResult {
    Pass,
    Fail,
    Skip,
}

#[derive(Debug, Deserialize)]
struct JunitSuite {
    #[serde(rename = "@timestamp")]
    timestamp: Option<String>,
    #[serde(rename = "testcase", default)]
    testcases: Vec<JunitCase>,
}

#[derive(Debug, Deserialize)]
struct JunitSuites {
    #[serde(rename = "testsuite", default)]
    suites: Vec<JunitSuite>,
}

#[derive(Debug, Deserialize)]
struct JunitCase {
    #[serde(rename = "@classname", default)]
    classname: String,
    #[serde(rename = "@name")]
    name: String,
    failure: Option<JunitMarker>,
    skipped: Option<JunitMarker>,
    properties: Option<JunitProperties>,
}

#[derive(Debug, Deserialize)]
struct JunitMarker {
    #[serde(rename = "@message")]
    _message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JunitProperties {
    #[serde(rename = "property", default)]
    properties: Vec<JunitProperty>,
}

#[derive(Debug, Deserialize)]
struct JunitProperty {
    #[serde(rename = "@name")]
    name: String,
    #[serde(rename = "@value")]
    value: String,
}

fn canonical_id(kind: ArtifactKind, external_id: &str) -> String {
    let kind = match kind {
        ArtifactKind::Feature => "feature",
        ArtifactKind::Condition => "condition",
        ArtifactKind::TestCase => "test_case",
    };
    format!("markharness-native:{kind}:{external_id}")
}

fn canonical_hash(value: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(value.as_bytes()))
}

pub fn import_native(root: &Path, git_ref: &str) -> io::Result<CanonicalSnapshot> {
    let temporary = tempfile::tempdir()?;
    let worktree = temporary.path().join("snapshot");
    git::add_detached_worktree(root, &worktree, git_ref)?;
    let loaded = generate::load_knowledge_snapshot(
        &worktree
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge"),
    );
    let _ = git::remove_worktree(root, &worktree);
    let knowledge = loaded?;
    let testcases = generate::compile_testcases(&knowledge);
    let feature_versions: BTreeMap<String, id_cache::FeatureVersion> =
        id_cache::resolve_feature_versions(root, git_ref, false)?
            .into_iter()
            .map(|version| (version.id.clone(), version))
            .collect();
    let provenance = Provenance {
        importer: "markharness-native".to_string(),
        importer_version: "1".to_string(),
        source_locator: git_ref.to_string(),
    };

    let mut artifacts = Vec::new();
    for (feature_id, version) in &feature_versions {
        artifacts.push(CanonicalArtifact {
            canonical_id: canonical_id(ArtifactKind::Feature, feature_id),
            source: "markharness-native".to_string(),
            external_id: feature_id.clone(),
            kind: ArtifactKind::Feature,
            version: ArtifactVersion {
                git_oid: Some(version.tree_sha.clone()),
                canonical_hash: None,
            },
            provenance: provenance.clone(),
            uid: version.uid.clone(),
        });
    }

    let mut relations = Vec::new();
    for testcase in &testcases {
        let Some(case) = knowledge.cases.iter().find(|case| {
            case.requirement_id == testcase.generated_from.requirement
                && case.feature_id == testcase.generated_from.feature
                && case.behavior_id == testcase.generated_from.behavior
                && case.condition_id == testcase.generated_from.condition
        }) else {
            continue;
        };
        let version = ArtifactVersion {
            git_oid: feature_versions
                .get(&case.feature_id)
                .map(|v| v.tree_sha.clone()),
            canonical_hash: None,
        };
        artifacts.push(CanonicalArtifact {
            canonical_id: canonical_id(ArtifactKind::Condition, &case.condition_id),
            source: "markharness-native".to_string(),
            external_id: case.condition_id.clone(),
            kind: ArtifactKind::Condition,
            version: version.clone(),
            provenance: provenance.clone(),
            uid: None,
        });
        artifacts.push(CanonicalArtifact {
            canonical_id: canonical_id(ArtifactKind::TestCase, &testcase.case_id),
            source: "markharness-native".to_string(),
            external_id: testcase.case_id.clone(),
            kind: ArtifactKind::TestCase,
            version,
            provenance: provenance.clone(),
            uid: None,
        });
        relations.push(CanonicalRelation {
            from: canonical_id(ArtifactKind::TestCase, &testcase.case_id),
            relation_type: "verifies".to_string(),
            to: canonical_id(ArtifactKind::Condition, &case.condition_id),
            origin: RelationOrigin {
                kind: RelationOriginKind::Derived,
                rule: Some("markharness-generate".to_string()),
                rule_version: Some("1".to_string()),
            },
            confidence: 1.0,
        });
    }

    artifacts.sort_by(|a, b| a.canonical_id.cmp(&b.canonical_id));
    relations.sort_by(|a, b| a.from.cmp(&b.from).then(a.to.cmp(&b.to)));
    Ok(CanonicalSnapshot {
        schema_version: 1,
        artifacts,
        relations,
        evidence: Vec::new(),
    })
}

pub fn import_junit(
    xml: &str,
    source_locator: &str,
    bound_versions: BTreeMap<String, String>,
) -> io::Result<CanonicalSnapshot> {
    let wrapper: JunitSuites = quick_xml::de::from_str(xml)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let suites = if wrapper.suites.is_empty() {
        vec![
            quick_xml::de::from_str(xml)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
        ]
    } else {
        wrapper.suites
    };
    let provenance = Provenance {
        importer: "junit".to_string(),
        importer_version: "1".to_string(),
        source_locator: source_locator.to_string(),
    };
    let mut artifacts = Vec::new();
    let mut evidence = Vec::new();
    let mut relations = Vec::new();
    for suite in suites {
        for case in suite.testcases {
            let condition_ids: Vec<String> = case
                .properties
                .as_ref()
                .into_iter()
                .flat_map(|properties| &properties.properties)
                .filter(|property| property.name == "markharness.condition")
                .map(|property| property.value.clone())
                .collect();
            let external_id = if case.classname.is_empty() {
                case.name
            } else {
                format!("{}:{}", case.classname, case.name)
            };
            let test_id = format!("junit:{external_id}");
            artifacts.push(CanonicalArtifact {
                canonical_id: format!("junit:test_case:{external_id}"),
                source: "junit".to_string(),
                external_id: external_id.clone(),
                kind: ArtifactKind::TestCase,
                version: ArtifactVersion {
                    git_oid: None,
                    canonical_hash: Some(canonical_hash(&format!("junit:test_case:{external_id}"))),
                },
                provenance: provenance.clone(),
                uid: None,
            });
            let result = if case.failure.is_some() {
                EvidenceResult::Fail
            } else if case.skipped.is_some() {
                EvidenceResult::Skip
            } else {
                EvidenceResult::Pass
            };
            evidence.push(CanonicalEvidence {
                test_id,
                result,
                executed_at: suite.timestamp.clone(),
                bound_versions: bound_versions.clone(),
                provenance: provenance.clone(),
            });
            for condition_id in condition_ids {
                relations.push(CanonicalRelation {
                    from: format!("junit:test_case:{external_id}"),
                    relation_type: "verifies".to_string(),
                    to: canonical_id(ArtifactKind::Condition, &condition_id),
                    origin: RelationOrigin {
                        kind: RelationOriginKind::Stored,
                        rule: None,
                        rule_version: None,
                    },
                    confidence: 1.0,
                });
            }
        }
    }
    artifacts.sort_by(|a, b| a.canonical_id.cmp(&b.canonical_id));
    evidence.sort_by(|a, b| a.test_id.cmp(&b.test_id));
    relations.sort_by(|a, b| a.from.cmp(&b.from).then(a.to.cmp(&b.to)));
    Ok(CanonicalSnapshot {
        schema_version: 1,
        artifacts,
        relations,
        evidence,
    })
}
