use std::fs;
use std::io;
use std::path::Path;

/// The default JSON Schema files (§3.5/§3.6), embedded at compile time from
/// this repo's own `schema/` and written into a freshly initialized
/// project's `schema/` by `markharness init` — after which the project's
/// copy is what actually governs validation (`load_schema` reads from disk,
/// not these constants), so a project can customize its own rules.
pub const DEFAULT_SCHEMA_FILES: &[(&str, &str)] = &[
    (
        "requirement.schema.json",
        include_str!("../schema/requirement.schema.json"),
    ),
    (
        "feature.schema.json",
        include_str!("../schema/feature.schema.json"),
    ),
    (
        "behavior.schema.json",
        include_str!("../schema/behavior.schema.json"),
    ),
    (
        "condition.schema.json",
        include_str!("../schema/condition.schema.json"),
    ),
    (
        "expected_result.schema.json",
        include_str!("../schema/expected_result.schema.json"),
    ),
    (
        "axis.schema.json",
        include_str!("../schema/axis.schema.json"),
    ),
    (
        "execution_result.schema.json",
        include_str!("../schema/execution_result.schema.json"),
    ),
    (
        "canonical_snapshot.schema.json",
        include_str!("../schema/canonical_snapshot.schema.json"),
    ),
    (
        "verification_plan.schema.json",
        include_str!("../schema/verification_plan.schema.json"),
    ),
];

/// Reads and parses `<root>/schema/<file_name>` as a JSON Schema document.
pub fn load_schema(root: &Path, file_name: &str) -> io::Result<serde_json::Value> {
    let content = fs::read_to_string(
        root.join(crate::project_root::MARKHARNESS_DIR)
            .join("schema")
            .join(file_name),
    )?;
    serde_json::from_str(&content).map_err(io::Error::other)
}

/// Validates `yaml` against `schema` (a parsed JSON Schema document),
/// returning every violation's human-readable message, or `Ok(())` when
/// none. YAML is converted to a `serde_json::Value` via `serde_json::to_value`
/// (any `Serialize` source, not just JSON, works through it) since
/// `jsonschema` validates against `serde_json::Value` instances.
pub fn validate_yaml(schema: &serde_json::Value, yaml: &str) -> Result<(), Vec<String>> {
    let parsed: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(yaml).map_err(|e| vec![format!("invalid YAML: {e}")])?;
    let instance = serde_json::to_value(&parsed)
        .map_err(|e| vec![format!("YAML→JSON conversion failed: {e}")])?;

    let validator =
        jsonschema::validator_for(schema).map_err(|e| vec![format!("invalid schema: {e}")])?;

    let errors: Vec<String> = validator
        .iter_errors(&instance)
        .map(|e| format!("{} at {}", e, e.instance_path()))
        .collect();

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feature_schema() -> serde_json::Value {
        serde_json::from_str(
            r#"{
                "type": "object",
                "properties": {
                    "id": {"type": "string", "minLength": 1},
                    "requirement": {"type": "string", "minLength": 1},
                    "label": {"type": "string", "minLength": 1},
                    "axis": {"type": "array", "items": {"type": "string"}}
                },
                "required": ["id", "requirement", "label", "axis"],
                "additionalProperties": false
            }"#,
        )
        .unwrap()
    }

    /// `docs/knowledge_draft.schema.json` is a reference-only artifact (not
    /// loaded by `knowledge_draft::validate_draft` — see the file's own
    /// `$comment`), so its only automated check is that it stays valid JSON
    /// Schema and accepts the same draft shape `knowledge scaffold` prints.
    #[test]
    fn knowledge_draft_reference_schema_is_valid_and_accepts_the_scaffold_template() {
        let schema_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/docs/knowledge_draft.schema.json"
        );
        let schema_text = std::fs::read_to_string(schema_path)
            .expect("docs/knowledge_draft.schema.json must exist");
        let schema: serde_json::Value =
            serde_json::from_str(&schema_text).expect("must be valid JSON");

        let draft_yaml = "\
requirement:
  id: controls
  label: controls
  axis: [gameplay]

feature:
  id: player-jump
  label: player-jump
  axis: [gameplay, animation]

behavior:
  id: jump
  label: jump
  axis: [gameplay]
  description: Player presses jump.

condition:
  id: ground
  label: ground
  description: Jump from the ground and land

expected:
  - description: lands safely
";

        assert_eq!(validate_yaml(&schema, draft_yaml), Ok(()));
    }

    #[test]
    fn validate_yaml_accepts_a_conforming_document() {
        let schema = feature_schema();
        let yaml = "id: player-jump\nrequirement: controls\nlabel: player-jump\naxis: [gameplay]\n";

        assert!(validate_yaml(&schema, yaml).is_ok());
    }

    #[test]
    fn validate_yaml_rejects_a_missing_required_field() {
        let schema = feature_schema();
        let yaml = "id: player-jump\nlabel: player-jump\naxis: [gameplay]\n";

        let result = validate_yaml(&schema, yaml);

        assert!(result.is_err());
    }

    #[test]
    fn validate_yaml_rejects_an_unknown_field() {
        let schema = feature_schema();
        let yaml = "id: player-jump\nrequirement: controls\nlabel: player-jump\naxis: [gameplay]\nbogus: true\n";

        let result = validate_yaml(&schema, yaml);

        assert!(result.is_err());
    }

    #[test]
    fn validate_yaml_rejects_wrong_field_type() {
        let schema = feature_schema();
        let yaml =
            "id: player-jump\nrequirement: controls\nlabel: player-jump\naxis: not-an-array\n";

        let result = validate_yaml(&schema, yaml);

        assert!(result.is_err());
    }

    #[test]
    fn load_schema_reads_and_parses_a_project_schema_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(
            dir.path()
                .join(crate::project_root::MARKHARNESS_DIR)
                .join("schema"),
        )
        .unwrap();
        fs::write(
            dir.path().join(".markharness/schema/feature.schema.json"),
            r#"{"type": "object"}"#,
        )
        .unwrap();

        let schema = load_schema(dir.path(), "feature.schema.json").unwrap();

        assert_eq!(schema["type"], "object");
    }

    #[test]
    fn default_schema_files_are_all_valid_json() {
        for (name, content) in DEFAULT_SCHEMA_FILES {
            let parsed: Result<serde_json::Value, _> = serde_json::from_str(content);
            assert!(parsed.is_ok(), "{name} is not valid JSON: {content}");
        }
    }

    /// design doc §10: no schema-generation crate is used, so a hand-written
    /// `feature.schema.json` and the hand-written `Feature` struct
    /// (`src/knowledge.rs`) are two independent authorities that must be
    /// kept in sync by hand. This test is the mechanical check that catches
    /// drift between them (a field added to one but not the other) without
    /// needing runtime reflection: it serializes a fully-populated `Feature`
    /// (every `Option` set to `Some`, so every field name appears) and
    /// compares its key set against the schema's `properties` key set.
    #[test]
    fn feature_struct_fields_match_feature_schema_properties() {
        let feature = crate::knowledge::Feature {
            id: "player-jump".to_string(),
            requirement: "player-controls".to_string(),
            label: "player-jump".to_string(),
            axis: vec!["gameplay".to_string()],
            description: Some("d".to_string()),
            forked_from: Some("player-jump-old".to_string()),
            uid: Some("01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string()),
        };
        let instance = serde_json::to_value(&feature).unwrap();
        let struct_fields: std::collections::BTreeSet<String> =
            instance.as_object().unwrap().keys().cloned().collect();

        let (_, schema_content) = DEFAULT_SCHEMA_FILES
            .iter()
            .find(|(name, _)| *name == "feature.schema.json")
            .expect("feature.schema.json must be a registered default schema file");
        let schema: serde_json::Value = serde_json::from_str(schema_content).unwrap();
        let schema_fields: std::collections::BTreeSet<String> = schema["properties"]
            .as_object()
            .expect("feature.schema.json must declare properties")
            .keys()
            .cloned()
            .collect();

        assert_eq!(
            struct_fields, schema_fields,
            "Feature struct fields and feature.schema.json properties have drifted apart"
        );
    }

    /// One per-`EntityKind` fixture for the contract test suite (design doc
    /// §3.3): a fully-populated instance (every `Option` field `Some`, so
    /// every field name appears once serialized) paired with the schema
    /// file it should match. Written as an exhaustive `match`, not a
    /// lookup table with a fallback arm — adding an `EntityKind` variant
    /// without adding its fixture here is a **compile error**, not a
    /// silently-skipped test case.
    fn fixture_for_kind(kind: crate::identity::EntityKind) -> (&'static str, serde_json::Value) {
        use crate::identity::EntityKind;
        const UID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        match kind {
            EntityKind::Requirement => (
                "requirement.schema.json",
                serde_json::to_value(crate::knowledge::Requirement {
                    id: "account-management".to_string(),
                    label: "account-management".to_string(),
                    axis: vec!["security".to_string()],
                    description: Some("d".to_string()),
                    source: Some("issue-1".to_string()),
                    related_issues: vec!["issue-2".to_string()],
                    uid: Some(UID.to_string()),
                })
                .unwrap(),
            ),
            EntityKind::Feature => (
                "feature.schema.json",
                serde_json::to_value(crate::knowledge::Feature {
                    id: "player-jump".to_string(),
                    requirement: "player-controls".to_string(),
                    label: "player-jump".to_string(),
                    axis: vec!["gameplay".to_string()],
                    description: Some("d".to_string()),
                    forked_from: Some("player-jump-old".to_string()),
                    uid: Some(UID.to_string()),
                })
                .unwrap(),
            ),
            EntityKind::Behavior => (
                "behavior.schema.json",
                serde_json::to_value(crate::knowledge::Behavior {
                    id: "player-jump-jump".to_string(),
                    feature: "player-jump".to_string(),
                    label: "jump".to_string(),
                    axis: vec!["gameplay".to_string()],
                    description: "Player presses jump.".to_string(),
                    uid: Some(UID.to_string()),
                })
                .unwrap(),
            ),
            EntityKind::Condition => (
                "condition.schema.json",
                serde_json::to_value(crate::knowledge::Condition {
                    id: "player-jump-jump-ground".to_string(),
                    behavior: "player-jump-jump".to_string(),
                    label: "ground".to_string(),
                    description: "Jump from the ground and land.".to_string(),
                    uid: Some(UID.to_string()),
                })
                .unwrap(),
            ),
            EntityKind::ExpectedResult => (
                "expected_result.schema.json",
                serde_json::to_value(crate::knowledge::ExpectedResult {
                    id: "player-jump-jump-ground-001".to_string(),
                    condition: "player-jump-jump-ground".to_string(),
                    description: "Lands safely.".to_string(),
                    generated_by: Some(crate::knowledge::GeneratedBy::Manual),
                    verified_by: Some(crate::knowledge::VerifiedBy { human_review: true }),
                    uid: Some(UID.to_string()),
                })
                .unwrap(),
            ),
        }
    }

    /// design doc §3.3's exhaustiveness test: cross-checks
    /// `EntityKind::ALL`, `DESCRIPTORS` (via `identity::descriptor`),
    /// `DEFAULT_SCHEMA_FILES`, and this module's `fixture_for_kind` against
    /// each other by key-set and count, not just by existence in one
    /// direction. Catches, for every kind:
    /// - a schema file two different kinds' descriptors both name (a
    ///   duplicate/misdirected registration `DEFAULT_SCHEMA_FILES.iter().any`
    ///   alone can't see, since it only asks "does *a* match exist");
    /// - a descriptor-named schema registered more than once (or not at
    ///   all) in `DEFAULT_SCHEMA_FILES`;
    /// - drift between the fixture's serialized fields and the schema's
    ///   declared `properties`.
    ///
    /// A kind missing its `fixture_for_kind` arm fails to compile before
    /// any of this runs.
    #[test]
    fn entity_kind_descriptors_schemas_and_fixtures_are_mutually_consistent() {
        use crate::identity::EntityKind;

        let schema_names: Vec<&str> = EntityKind::ALL
            .iter()
            .map(|&kind| crate::identity::descriptor(kind).schema_name)
            .collect();
        let unique_schema_names: std::collections::BTreeSet<&str> =
            schema_names.iter().copied().collect();
        assert_eq!(
            schema_names.len(),
            unique_schema_names.len(),
            "two EntityKinds' descriptors name the same schema file: {schema_names:?}"
        );
        assert_eq!(
            unique_schema_names.len(),
            EntityKind::ALL.len(),
            "expected one distinct schema per EntityKind"
        );

        for &schema_name in &unique_schema_names {
            let registrations = DEFAULT_SCHEMA_FILES
                .iter()
                .filter(|(name, _)| *name == schema_name)
                .count();
            assert_eq!(
                registrations, 1,
                "'{schema_name}' must be registered exactly once in DEFAULT_SCHEMA_FILES, found {registrations}"
            );
        }

        for kind in EntityKind::ALL {
            let (schema_name, instance) = fixture_for_kind(kind);
            assert_eq!(
                schema_name,
                crate::identity::descriptor(kind).schema_name,
                "{kind:?}'s fixture and descriptor name different schema files"
            );
            let struct_fields: std::collections::BTreeSet<String> =
                instance.as_object().unwrap().keys().cloned().collect();

            let (_, schema_content) = DEFAULT_SCHEMA_FILES
                .iter()
                .find(|(name, _)| *name == schema_name)
                .unwrap();
            let schema: serde_json::Value = serde_json::from_str(schema_content).unwrap();
            let schema_fields: std::collections::BTreeSet<String> = schema["properties"]
                .as_object()
                .unwrap_or_else(|| panic!("{schema_name} must declare properties"))
                .keys()
                .cloned()
                .collect();

            assert_eq!(
                struct_fields, schema_fields,
                "{kind:?}'s fixture fields and {schema_name} properties have drifted apart"
            );
        }
    }
}
