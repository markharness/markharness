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
}
