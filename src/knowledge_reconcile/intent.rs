//! Knowledge Intent schema (ADR 0027 §2): an authoring-only declaration,
//! distinct from the canonical storage format under `.markharness/knowledge/`
//! (`crate::knowledge`). New elements are referenced by a document-local
//! `key` that is never persisted; existing elements are referenced by UID.

use serde::Deserialize;

/// The only `format` value `knowledge reconcile` accepts (ADR 0027 §2).
pub const INTENT_FORMAT_V1: &str = "markharness/knowledge-intent/v1";

/// A blank Knowledge Intent for non-interactive callers (ADR 0027 §7's
/// "Intent雛形" / 0028 start gate) to start from — one new Requirement,
/// referenced by document-local `key` from one new Feature, which in turn
/// declares one new Behavior and Scenario. Scalars
/// left blank (`id:` with nothing after the colon) parse as YAML `null` →
/// `None`, but fail `validate_static`/`build_plan`'s required-field
/// checks until filled in — `--check` against this file reports exactly
/// what is still missing. `action`/`results` entries cannot be left blank
/// the same way (`StepIntent`/`results` hold plain `String`, not
/// `Option<String>` — nothing being untyped-null there for a step or
/// result to "omit"), so they carry placeholder text to replace instead.
pub const INTENT_TEMPLATE: &str = "\
# knowledge reconcile (ADR 0027)
# Fill in the Knowledge Intent below, then run:
#   markharness knowledge reconcile <this-file> --check
# to preview the resulting mutation plan before writing it for real (drop
# --check once it looks right). `key` is a document-local reference used
# only by `contributes_to` in this same file; it is never saved. Existing
# elements are selected by `uid` instead of `key`/`id` — see
# docs/ja/decisions/0027-declarative-knowledge-reconciliation.md §5.
format: markharness/knowledge-intent/v1
mode: merge

requirements:
  - key: req_example
    id:
    source: native
    label:
    axis: []

features:
  - key: feature_example
    id:
    contributes_to: [req_example]
    label:
    axis: []
    behaviors:
      - id:
        label:
        axis: []
        description:
        scenarios:
          - id:
            label:
            description:
            phases:
              - steps:
                  - action: Describe what happens in this step
                results:
                  - Describe the expected result
";

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
    /// Present only for `source: external`: StrictDoc's own MID (a
    /// machine-generated, always-lowercase-hex identifier), held verbatim
    /// (ADR 0030). Never normalized.
    #[serde(default)]
    pub source_key: Option<String>,
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
    /// The Feature this one is conceptually derived from (ADR 0027 §2's
    /// "情報を失わず" requirement: the canonical model stores it, so the
    /// only authoring interface must be able to set it). Names an
    /// existing Feature's display id, not a UID — this is a hand-recorded
    /// domain fact, validated but never derived.
    #[serde(default)]
    pub forked_from: Option<String>,
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
    /// Common procedures this Behavior declares (ADR 0017 §2), replacing
    /// the whole collection when present, same as `axis`/`contributes_to`
    /// (ADR 0027 §5's value-collection rule). Only meaningful when this
    /// `BehaviorIntent` creates a brand-new Behavior.
    #[serde(default)]
    pub procedures: Option<Vec<ProcedureIntent>>,
    #[serde(default)]
    pub scenarios: Vec<ScenarioIntent>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProcedureIntent {
    pub name: String,
    #[serde(default)]
    pub steps: Vec<String>,
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
    /// Implementation rationale note (ADR 0016). Never consumed by
    /// generation; stored so the canonical model stays expressible
    /// through the Intent.
    #[serde(default)]
    pub implementation_note: Option<String>,
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

    /// A blank scalar (`id:` with nothing after the colon) is YAML `null`,
    /// which every field here accepts as `None` — the template must parse
    /// even before anyone fills it in, so `--check` is what first reports
    /// what is still missing (`missing_required_field`), not a parse
    /// failure.
    #[test]
    fn the_intent_template_parses_as_a_valid_intent_document() {
        let doc = parse_intent(INTENT_TEMPLATE).expect("should parse");
        assert_eq!(doc.mode, IntentMode::Merge);
        assert_eq!(doc.requirements.len(), 1);
        assert_eq!(doc.features.len(), 1);
        assert_eq!(doc.features[0].behaviors.len(), 1);
        assert_eq!(doc.features[0].behaviors[0].scenarios.len(), 1);
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
