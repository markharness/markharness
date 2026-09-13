//! Knowledge Intent schema (ADR 0027 §2): an authoring-only declaration,
//! distinct from the canonical storage format under `.markharness/knowledge/`
//! (`crate::knowledge`). New elements are referenced by a document-local
//! `key` that is never persisted; existing elements are referenced by UID.

use serde::Deserialize;

/// The only `format` value `knowledge reconcile` accepts (ADR 0027 §2).
pub const INTENT_FORMAT_V1: &str = "markharness/knowledge-intent/v1";

/// The initial version supports only non-deleting `merge` (ADR 0027 §4).
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IntentMode {
    Merge,
}

#[derive(Debug, Deserialize)]
pub struct IntentDocument {
    pub format: String,
    pub mode: IntentMode,
    #[serde(default)]
    pub requirements: Vec<RequirementIntent>,
    #[serde(default)]
    pub features: Vec<FeatureIntent>,
}

#[derive(Debug, Deserialize)]
pub struct RequirementIntent {
    /// Document-local reference for a new Requirement (never persisted).
    #[serde(default)]
    pub key: Option<String>,
    /// Selects an existing Requirement to patch or rename.
    #[serde(default)]
    pub uid: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub axis: Option<Vec<String>>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub source_locator: Option<String>,
    /// A pinned blob OID, or the literal `current` instruction (ADR 0027 §5),
    /// never persisted as-is.
    #[serde(default)]
    pub source_revision: Option<String>,
    #[serde(default)]
    pub related_issues: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct FeatureIntent {
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub uid: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    /// Requirement `key`s (new) or UIDs (existing), replacing the whole
    /// `requirement_uids` collection when present (ADR 0027 §5).
    #[serde(default)]
    pub contributes_to: Option<Vec<String>>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub axis: Option<Vec<String>>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub behaviors: Vec<BehaviorIntent>,
}

#[derive(Debug, Deserialize)]
pub struct BehaviorIntent {
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub uid: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub axis: Option<Vec<String>>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub scenarios: Vec<ScenarioIntent>,
}

#[derive(Debug, Deserialize)]
pub struct ScenarioIntent {
    /// Selects an existing Scenario to patch or reparent (ADR 0027 §3). New
    /// Scenarios omit this; there is no document-local `key` for Scenario
    /// because nothing references a Scenario from elsewhere in the Intent.
    #[serde(default)]
    pub uid: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub phases: Option<Vec<PhaseIntent>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PhaseIntent {
    #[serde(default)]
    pub steps: Vec<StepIntent>,
    #[serde(default)]
    pub results: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum StepIntent {
    Action {
        action: String,
    },
    Use {
        #[serde(rename = "use")]
        procedure: String,
    },
}

/// A Knowledge Intent that failed to parse as YAML, or that parsed but
/// carries an unrecognized `format` (ADR 0027 §7 `invalid_format`).
#[derive(Debug)]
pub enum IntentParseError {
    Yaml(serde_yaml_ng::Error),
    UnrecognizedFormat(String),
}

impl std::fmt::Display for IntentParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IntentParseError::Yaml(e) => write!(f, "{e}"),
            IntentParseError::UnrecognizedFormat(got) => {
                write!(
                    f,
                    "unrecognized format '{got}', expected '{INTENT_FORMAT_V1}'"
                )
            }
        }
    }
}

/// Parses a Knowledge Intent document. Malformed YAML and a `format` other
/// than [`INTENT_FORMAT_V1`] both report `IntentParseError`, which the
/// caller turns into an `invalid_format` diagnostic.
pub fn parse_intent(yaml: &str) -> Result<IntentDocument, IntentParseError> {
    let doc: IntentDocument = serde_yaml_ng::from_str(yaml).map_err(IntentParseError::Yaml)?;
    if doc.format != INTENT_FORMAT_V1 {
        return Err(IntentParseError::UnrecognizedFormat(doc.format));
    }
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_MINIMAL: &str = "\
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - key: req_todo
    id: todo
    source: native
    label: TODO management
    axis: [functional]
";

    #[test]
    fn parses_valid_minimal_intent() {
        let doc = parse_intent(VALID_MINIMAL).expect("should parse");
        assert_eq!(doc.mode, IntentMode::Merge);
        assert_eq!(doc.requirements.len(), 1);
        assert_eq!(doc.requirements[0].key, Some("req_todo".to_string()));
        assert_eq!(doc.requirements[0].id, Some("todo".to_string()));
    }

    #[test]
    fn parses_nested_feature_behavior_scenario() {
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: merge

features:
  - key: feature_todo
    id: todo-management
    contributes_to: [req_todo]
    label: TODO management
    axis: [functional]
    behaviors:
      - key: behavior_add
        id: add-todo
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
        let doc = parse_intent(yaml).expect("should parse");
        assert_eq!(doc.features.len(), 1);
        let feature = &doc.features[0];
        assert_eq!(feature.contributes_to, Some(vec!["req_todo".to_string()]));
        assert_eq!(feature.behaviors.len(), 1);
        assert_eq!(feature.behaviors[0].scenarios.len(), 1);
    }

    #[test]
    fn rejects_malformed_yaml_as_invalid_format() {
        let err = parse_intent("not: [valid").unwrap_err();
        assert!(matches!(err, IntentParseError::Yaml(_)));
    }

    #[test]
    fn rejects_unrecognized_format_string() {
        let yaml = "\
format: some/other/v9
mode: merge
";
        let err = parse_intent(yaml).unwrap_err();
        match err {
            IntentParseError::UnrecognizedFormat(got) => assert_eq!(got, "some/other/v9"),
            other => panic!("expected UnrecognizedFormat, got {other:?}"),
        }
    }

    #[test]
    fn rejects_missing_format_field() {
        let yaml = "mode: merge\n";
        let err = parse_intent(yaml).unwrap_err();
        assert!(matches!(err, IntentParseError::Yaml(_)));
    }

    #[test]
    fn rejects_unknown_mode() {
        let yaml = "\
format: markharness/knowledge-intent/v1
mode: exact
";
        let err = parse_intent(yaml).unwrap_err();
        assert!(matches!(err, IntentParseError::Yaml(_)));
    }
}
