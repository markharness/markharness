use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

use crate::knowledge::{self, is_valid_slug, strip_redundant_condition_prefix};

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct RequirementDraft {
    pub id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub axis: Option<Vec<String>>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct FeatureDraft {
    pub id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub axis: Option<Vec<String>>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub forked_from: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct BehaviorDraft {
    pub id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub axis: Option<Vec<String>>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct ConditionDraft {
    pub id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct ExpectedDraft {
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct KnowledgeDraft {
    pub requirement: RequirementDraft,
    pub feature: FeatureDraft,
    pub behavior: BehaviorDraft,
    pub condition: ConditionDraft,
    #[serde(default)]
    pub expected: Vec<ExpectedDraft>,
}

#[derive(Debug)]
pub struct DraftParseError(pub serde_yaml_ng::Error);

impl std::fmt::Display for DraftParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "failed to parse draft: {}", self.0)
    }
}

impl std::error::Error for DraftParseError {}

pub fn parse_draft(yaml: &str) -> Result<KnowledgeDraft, DraftParseError> {
    serde_yaml_ng::from_str(yaml).map_err(DraftParseError)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationErrorCode {
    InvalidSlug,
    MissingAxis,
    MissingDescription,
    UnknownAxis,
    RedundantPrefix,
    ConflictingExistingValue,
    ParentNotFound,
    UnknownForkedFrom,
}

impl ValidationErrorCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            ValidationErrorCode::InvalidSlug => "invalid_slug",
            ValidationErrorCode::MissingAxis => "missing_axis",
            ValidationErrorCode::MissingDescription => "missing_description",
            ValidationErrorCode::UnknownAxis => "unknown_axis",
            ValidationErrorCode::RedundantPrefix => "redundant_prefix",
            ValidationErrorCode::ConflictingExistingValue => "conflicting_existing_value",
            ValidationErrorCode::ParentNotFound => "parent_not_found",
            ValidationErrorCode::UnknownForkedFrom => "unknown_forked_from",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    pub code: ValidationErrorCode,
    pub path: String,
    pub value: Option<String>,
    pub message: String,
    pub suggestion: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AxisEntry {
    id: String,
}

pub fn load_axis_registry(root: &Path) -> HashSet<String> {
    let axes_dir = root.join("axes");
    let Ok(entries) = fs::read_dir(&axes_dir) else {
        return HashSet::new();
    };

    entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("yml"))
        .filter_map(|path| fs::read_to_string(&path).ok())
        .filter_map(|yaml| serde_yaml_ng::from_str::<AxisEntry>(&yaml).ok())
        .map(|axis| axis.id)
        .collect()
}

pub struct ValidateOptions {
    pub strip_redundant_prefix: bool,
}

/// Searches every `knowledge/<requirement>/<feature>/feature.yml` for one
/// whose `id` matches `feature_id`. Feature ids are unique across the whole
/// tree even though they are nested under a requirement directory, so a
/// `forked_from` reference cannot be resolved by path alone.
fn feature_id_exists(knowledge_root: &Path, feature_id: &str) -> bool {
    let Ok(requirement_entries) = fs::read_dir(knowledge_root) else {
        return false;
    };
    for requirement_entry in requirement_entries.filter_map(|e| e.ok()) {
        let requirement_dir = requirement_entry.path();
        if !requirement_dir.is_dir() {
            continue;
        }
        let Ok(feature_entries) = fs::read_dir(&requirement_dir) else {
            continue;
        };
        for feature_entry in feature_entries.filter_map(|e| e.ok()) {
            let feature_dir = feature_entry.path();
            if feature_dir.join("feature.yml").is_file()
                && feature_dir.file_name().and_then(|n| n.to_str()) == Some(feature_id)
            {
                return true;
            }
        }
    }
    false
}

/// Determines which directory a condition should be read from / written to,
/// mirroring the legacy-directory-wins-over-stripping behavior of the
/// interactive CLI (`interactive.rs::run_add`).
pub fn resolve_effective_condition_id(
    behavior_dir: &Path,
    behavior_id: &str,
    raw_condition_id: &str,
    strip_redundant_prefix: bool,
) -> String {
    let legacy_path = behavior_dir.join(raw_condition_id).join("condition.yml");
    if legacy_path.is_file() {
        return raw_condition_id.to_string();
    }
    if strip_redundant_prefix
        && let Some(stripped) = strip_redundant_condition_prefix(behavior_id, raw_condition_id)
    {
        return stripped;
    }
    raw_condition_id.to_string()
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut prev = row[0];
        row[0] = i;
        for j in 1..=b.len() {
            let cur = row[j];
            row[j] = if a[i - 1] == b[j - 1] {
                prev
            } else {
                1 + prev.min(row[j]).min(row[j - 1])
            };
            prev = cur;
        }
    }
    row[b.len()]
}

pub(crate) fn nearest_axis_suggestion(
    value: &str,
    axis_registry: &HashSet<String>,
) -> Option<String> {
    axis_registry
        .iter()
        .map(|candidate| (candidate, levenshtein(value, candidate)))
        .filter(|(_, distance)| *distance <= 2)
        .min_by_key(|(_, distance)| *distance)
        .map(|(candidate, _)| candidate.clone())
}

fn push_invalid_slug(errors: &mut Vec<ValidationError>, path: &str, value: &str) {
    if !is_valid_slug(value) {
        errors.push(ValidationError {
            code: ValidationErrorCode::InvalidSlug,
            path: path.to_string(),
            value: Some(value.to_string()),
            message: format!(
                "\"{value}\" is not a valid slug (lowercase alphanumeric and hyphen only)"
            ),
            suggestion: None,
        });
    }
}

fn push_axis_checks(
    errors: &mut Vec<ValidationError>,
    path_prefix: &str,
    axis: &Option<Vec<String>>,
    exists: bool,
    axis_registry: &HashSet<String>,
) {
    match axis {
        None => {
            if !exists {
                errors.push(ValidationError {
                    code: ValidationErrorCode::MissingAxis,
                    path: format!("{path_prefix}.axis"),
                    value: None,
                    message: format!("{path_prefix}.axis is required when creating a new entry"),
                    suggestion: None,
                });
            }
        }
        Some(values) => {
            if !exists && values.is_empty() {
                errors.push(ValidationError {
                    code: ValidationErrorCode::MissingAxis,
                    path: format!("{path_prefix}.axis"),
                    value: None,
                    message: format!("{path_prefix}.axis is required when creating a new entry"),
                    suggestion: None,
                });
            }
            for (i, value) in values.iter().enumerate() {
                if !axis_registry.contains(value) {
                    errors.push(ValidationError {
                        code: ValidationErrorCode::UnknownAxis,
                        path: format!("{path_prefix}.axis[{i}]"),
                        value: Some(value.clone()),
                        message: format!("axis \"{value}\" is not registered"),
                        suggestion: nearest_axis_suggestion(value, axis_registry),
                    });
                }
            }
        }
    }
}

fn push_missing_description(
    errors: &mut Vec<ValidationError>,
    path: &str,
    description: &Option<String>,
    exists: bool,
) {
    if exists {
        return;
    }
    let is_empty = description
        .as_ref()
        .map(|d| d.trim().is_empty())
        .unwrap_or(true);
    if is_empty {
        errors.push(ValidationError {
            code: ValidationErrorCode::MissingDescription,
            path: path.to_string(),
            value: None,
            message: format!("{path} must not be empty"),
            suggestion: None,
        });
    }
}

fn push_conflicting_value(
    errors: &mut Vec<ValidationError>,
    path: &str,
    provided: &str,
    existing: &str,
) {
    if provided.trim() != existing.trim() {
        errors.push(ValidationError {
            code: ValidationErrorCode::ConflictingExistingValue,
            path: path.to_string(),
            value: Some(provided.to_string()),
            message: format!(
                "{path} \"{provided}\" conflicts with existing value \"{}\"",
                existing.trim()
            ),
            suggestion: Some(existing.trim().to_string()),
        });
    }
}

fn push_conflicting_axis(
    errors: &mut Vec<ValidationError>,
    path: &str,
    provided: &[String],
    existing: &[String],
) {
    let mut provided_sorted = provided.to_vec();
    provided_sorted.sort();
    let mut existing_sorted = existing.to_vec();
    existing_sorted.sort();
    if provided_sorted != existing_sorted {
        errors.push(ValidationError {
            code: ValidationErrorCode::ConflictingExistingValue,
            path: path.to_string(),
            value: Some(provided.join(", ")),
            message: format!(
                "{path} [{}] conflicts with existing value [{}]",
                provided.join(", "),
                existing.join(", ")
            ),
            suggestion: Some(existing.join(", ")),
        });
    }
}

fn push_parent_mismatch(
    errors: &mut Vec<ValidationError>,
    path: &str,
    expected: &str,
    actual: &str,
) {
    if expected != actual {
        errors.push(ValidationError {
            code: ValidationErrorCode::ParentNotFound,
            path: path.to_string(),
            value: Some(actual.to_string()),
            message: format!(
                "{path} refers to \"{actual}\" but the draft chain expects \"{expected}\""
            ),
            suggestion: Some(expected.to_string()),
        });
    }
}

pub fn validate_draft(
    root: &Path,
    draft: &KnowledgeDraft,
    options: &ValidateOptions,
) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    let axis_registry = load_axis_registry(root);
    let knowledge_root = root.join("knowledge");

    push_invalid_slug(&mut errors, "requirement.id", &draft.requirement.id);
    push_invalid_slug(&mut errors, "feature.id", &draft.feature.id);
    push_invalid_slug(&mut errors, "behavior.id", &draft.behavior.id);
    push_invalid_slug(&mut errors, "condition.id", &draft.condition.id);

    let requirement_dir = knowledge_root.join(&draft.requirement.id);
    let requirement_path = requirement_dir.join("requirement.yml");
    let requirement_exists = requirement_path.is_file();

    let feature_dir = requirement_dir.join(&draft.feature.id);
    let feature_path = feature_dir.join("feature.yml");
    let feature_exists = feature_path.is_file();

    let behavior_dir = feature_dir.join(&draft.behavior.id);
    let behavior_path = behavior_dir.join("behavior.yml");
    let behavior_exists = behavior_path.is_file();

    let legacy_condition_path = behavior_dir.join(&draft.condition.id).join("condition.yml");
    if !legacy_condition_path.is_file()
        && let Some(stripped) =
            strip_redundant_condition_prefix(&draft.behavior.id, &draft.condition.id)
        && !options.strip_redundant_prefix
    {
        errors.push(ValidationError {
            code: ValidationErrorCode::RedundantPrefix,
            path: "condition.id".to_string(),
            value: Some(draft.condition.id.clone()),
            message: format!(
                "condition.id \"{}\" starts with behavior.id \"{}-\" prefix",
                draft.condition.id, draft.behavior.id
            ),
            suggestion: Some(stripped),
        });
    }

    let effective_condition_id = resolve_effective_condition_id(
        &behavior_dir,
        &draft.behavior.id,
        &draft.condition.id,
        options.strip_redundant_prefix,
    );
    let condition_dir = behavior_dir.join(&effective_condition_id);
    let condition_path = condition_dir.join("condition.yml");
    let condition_exists = condition_path.is_file();

    if let Some(forked_from) = &draft.feature.forked_from
        && !feature_id_exists(&knowledge_root, forked_from)
    {
        errors.push(ValidationError {
            code: ValidationErrorCode::UnknownForkedFrom,
            path: "feature.forked_from".to_string(),
            value: Some(forked_from.clone()),
            message: format!("forked_from feature \"{forked_from}\" does not exist"),
            suggestion: None,
        });
    }

    push_axis_checks(
        &mut errors,
        "requirement",
        &draft.requirement.axis,
        requirement_exists,
        &axis_registry,
    );
    push_axis_checks(
        &mut errors,
        "feature",
        &draft.feature.axis,
        feature_exists,
        &axis_registry,
    );
    push_axis_checks(
        &mut errors,
        "behavior",
        &draft.behavior.axis,
        behavior_exists,
        &axis_registry,
    );
    push_missing_description(
        &mut errors,
        "behavior.description",
        &draft.behavior.description,
        behavior_exists,
    );
    push_missing_description(
        &mut errors,
        "condition.description",
        &draft.condition.description,
        condition_exists,
    );
    for (i, expected) in draft.expected.iter().enumerate() {
        if expected.description.trim().is_empty() {
            errors.push(ValidationError {
                code: ValidationErrorCode::MissingDescription,
                path: format!("expected[{i}].description"),
                value: None,
                message: format!("expected[{i}].description must not be empty"),
                suggestion: None,
            });
        }
    }

    if requirement_exists
        && let Ok(yaml) = fs::read_to_string(&requirement_path)
        && let Ok(existing) = knowledge::parse_requirement(&yaml)
    {
        if let Some(label) = &draft.requirement.label {
            push_conflicting_value(&mut errors, "requirement.label", label, &existing.label);
        }
        if let Some(axis) = &draft.requirement.axis {
            push_conflicting_axis(&mut errors, "requirement.axis", axis, &existing.axis);
        }
        if let Some(description) = &draft.requirement.description {
            push_conflicting_value(
                &mut errors,
                "requirement.description",
                description,
                existing.description.as_deref().unwrap_or(""),
            );
        }
    }

    if feature_exists
        && let Ok(yaml) = fs::read_to_string(&feature_path)
        && let Ok(existing) = knowledge::parse_feature(&yaml)
    {
        push_parent_mismatch(
            &mut errors,
            "feature.requirement",
            &draft.requirement.id,
            &existing.requirement,
        );
        if let Some(label) = &draft.feature.label {
            push_conflicting_value(&mut errors, "feature.label", label, &existing.label);
        }
        if let Some(axis) = &draft.feature.axis {
            push_conflicting_axis(&mut errors, "feature.axis", axis, &existing.axis);
        }
        if let Some(description) = &draft.feature.description {
            push_conflicting_value(
                &mut errors,
                "feature.description",
                description,
                existing.description.as_deref().unwrap_or(""),
            );
        }
    }

    if behavior_exists
        && let Ok(yaml) = fs::read_to_string(&behavior_path)
        && let Ok(existing) = knowledge::parse_behavior(&yaml)
    {
        push_parent_mismatch(
            &mut errors,
            "behavior.feature",
            &draft.feature.id,
            &existing.feature,
        );
        if let Some(label) = &draft.behavior.label {
            push_conflicting_value(&mut errors, "behavior.label", label, &existing.label);
        }
        if let Some(axis) = &draft.behavior.axis {
            push_conflicting_axis(&mut errors, "behavior.axis", axis, &existing.axis);
        }
        if let Some(description) = &draft.behavior.description {
            push_conflicting_value(
                &mut errors,
                "behavior.description",
                description,
                &existing.description,
            );
        }
    }

    if condition_exists
        && let Ok(yaml) = fs::read_to_string(&condition_path)
        && let Ok(existing) = knowledge::parse_condition(&yaml)
    {
        push_parent_mismatch(
            &mut errors,
            "condition.behavior",
            &draft.behavior.id,
            &existing.behavior,
        );
        if let Some(label) = &draft.condition.label {
            push_conflicting_value(&mut errors, "condition.label", label, &existing.label);
        }
        if let Some(description) = &draft.condition.description {
            push_conflicting_value(
                &mut errors,
                "condition.description",
                description,
                &existing.description,
            );
        }
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL_DRAFT_YAML: &str = "\
requirement:
  id: controls
  label: controls
  axis: [gameplay]
  description: null

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
  - description: takes fall damage if height > 3m
";

    #[test]
    fn parses_full_draft_with_all_fields_present() {
        let draft = parse_draft(FULL_DRAFT_YAML).unwrap();

        assert_eq!(draft.requirement.id, "controls");
        assert_eq!(draft.requirement.label, Some("controls".to_string()));
        assert_eq!(draft.requirement.axis, Some(vec!["gameplay".to_string()]));
        assert_eq!(draft.requirement.description, None);

        assert_eq!(draft.feature.id, "player-jump");
        assert_eq!(
            draft.feature.axis,
            Some(vec!["gameplay".to_string(), "animation".to_string()])
        );

        assert_eq!(draft.behavior.id, "jump");
        assert_eq!(
            draft.behavior.description,
            Some("Player presses jump.".to_string())
        );

        assert_eq!(draft.condition.id, "ground");
        assert_eq!(
            draft.condition.description,
            Some("Jump from the ground and land".to_string())
        );

        assert_eq!(draft.expected.len(), 2);
        assert_eq!(draft.expected[0].description, "lands safely");
        assert_eq!(
            draft.expected[1].description,
            "takes fall damage if height > 3m"
        );
    }

    #[test]
    fn parses_draft_with_omitted_axis_description_and_label_for_existing_id_reuse() {
        let yaml = "\
requirement:
  id: controls

feature:
  id: player-jump

behavior:
  id: jump

condition:
  id: ground

expected:
  - description: lands safely
";

        let draft = parse_draft(yaml).unwrap();

        assert_eq!(draft.requirement.label, None);
        assert_eq!(draft.requirement.axis, None);
        assert_eq!(draft.requirement.description, None);
        assert_eq!(draft.feature.axis, None);
        assert_eq!(draft.behavior.axis, None);
        assert_eq!(draft.behavior.description, None);
        assert_eq!(draft.condition.label, None);
        assert_eq!(draft.condition.description, None);
    }

    #[test]
    fn returns_parse_error_for_invalid_yaml() {
        let result = parse_draft("requirement: [this is not a mapping");

        assert!(result.is_err());
    }

    #[test]
    fn load_axis_registry_reads_ids_from_axes_yml_files() {
        let dir = tempfile::tempdir().unwrap();
        let axes_dir = dir.path().join("axes");
        fs::create_dir_all(&axes_dir).unwrap();
        fs::write(
            axes_dir.join("gameplay.yml"),
            "id: gameplay\nlabel: Gameplay\n",
        )
        .unwrap();

        let registry = load_axis_registry(dir.path());

        assert!(registry.contains("gameplay"));
    }

    #[test]
    fn load_axis_registry_returns_empty_set_when_axes_dir_missing() {
        let dir = tempfile::tempdir().unwrap();

        let registry = load_axis_registry(dir.path());

        assert!(registry.is_empty());
    }

    fn setup_root_with_axes(axis_ids: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        fs::create_dir_all(dir.path().join("knowledge")).unwrap();
        for id in axis_ids {
            fs::write(
                dir.path().join("axes").join(format!("{id}.yml")),
                format!("id: {id}\nlabel: {id}\n"),
            )
            .unwrap();
        }
        dir
    }

    fn full_new_draft() -> KnowledgeDraft {
        parse_draft(FULL_DRAFT_YAML).unwrap()
    }

    fn no_strip() -> ValidateOptions {
        ValidateOptions {
            strip_redundant_prefix: false,
        }
    }

    #[test]
    fn validate_draft_returns_no_errors_for_a_fully_valid_new_chain() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        let draft = full_new_draft();

        let errors = validate_draft(dir.path(), &draft, &no_strip());

        assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
    }

    #[test]
    fn validate_draft_reports_invalid_slug_for_bad_id() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        let mut draft = full_new_draft();
        draft.condition.id = "Ground!".to_string();

        let errors = validate_draft(dir.path(), &draft, &no_strip());

        assert!(
            errors
                .iter()
                .any(|e| e.code == ValidationErrorCode::InvalidSlug && e.path == "condition.id")
        );
    }

    #[test]
    fn validate_draft_reports_missing_axis_for_new_behavior_without_axis() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        let mut draft = full_new_draft();
        draft.behavior.axis = None;

        let errors = validate_draft(dir.path(), &draft, &no_strip());

        assert!(
            errors
                .iter()
                .any(|e| e.code == ValidationErrorCode::MissingAxis && e.path == "behavior.axis")
        );
    }

    #[test]
    fn validate_draft_reports_missing_axis_for_new_behavior_with_empty_axis_list() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        let mut draft = full_new_draft();
        draft.behavior.axis = Some(vec![]);

        let errors = validate_draft(dir.path(), &draft, &no_strip());

        assert!(
            errors
                .iter()
                .any(|e| e.code == ValidationErrorCode::MissingAxis && e.path == "behavior.axis")
        );
    }

    #[test]
    fn validate_draft_reports_missing_description_for_new_behavior_without_description() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        let mut draft = full_new_draft();
        draft.behavior.description = None;

        let errors = validate_draft(dir.path(), &draft, &no_strip());

        assert!(
            errors
                .iter()
                .any(|e| e.code == ValidationErrorCode::MissingDescription
                    && e.path == "behavior.description")
        );
    }

    #[test]
    fn validate_draft_reports_missing_description_for_new_condition_without_description() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        let mut draft = full_new_draft();
        draft.condition.description = None;

        let errors = validate_draft(dir.path(), &draft, &no_strip());

        assert!(
            errors
                .iter()
                .any(|e| e.code == ValidationErrorCode::MissingDescription
                    && e.path == "condition.description")
        );
    }

    #[test]
    fn validate_draft_reports_missing_description_for_empty_expected_entry() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        let mut draft = full_new_draft();
        draft.expected.push(ExpectedDraft {
            description: "   ".to_string(),
        });
        let last = draft.expected.len() - 1;

        let errors = validate_draft(dir.path(), &draft, &no_strip());

        assert!(
            errors
                .iter()
                .any(|e| e.code == ValidationErrorCode::MissingDescription
                    && e.path == format!("expected[{last}].description"))
        );
    }

    #[test]
    fn validate_draft_reports_unknown_axis_with_suggestion() {
        let dir = setup_root_with_axes(&["gameplay", "animation", "validation"]);
        let mut draft = full_new_draft();
        draft.behavior.axis = Some(vec!["validdation".to_string()]);

        let errors = validate_draft(dir.path(), &draft, &no_strip());

        let err = errors
            .iter()
            .find(|e| e.code == ValidationErrorCode::UnknownAxis)
            .expect("expected an unknown_axis error");
        assert_eq!(err.path, "behavior.axis[0]");
        assert_eq!(err.suggestion, Some("validation".to_string()));
    }

    #[test]
    fn validate_draft_reports_redundant_prefix_without_strip_flag() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        let mut draft = full_new_draft();
        draft.condition.id = "jump-ground".to_string();

        let errors = validate_draft(dir.path(), &draft, &no_strip());

        let err = errors
            .iter()
            .find(|e| e.code == ValidationErrorCode::RedundantPrefix)
            .expect("expected a redundant_prefix error");
        assert_eq!(err.suggestion, Some("ground".to_string()));
    }

    #[test]
    fn validate_draft_allows_redundant_prefix_when_strip_flag_set() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        let mut draft = full_new_draft();
        draft.condition.id = "jump-ground".to_string();

        let errors = validate_draft(
            dir.path(),
            &draft,
            &ValidateOptions {
                strip_redundant_prefix: true,
            },
        );

        assert!(
            !errors
                .iter()
                .any(|e| e.code == ValidationErrorCode::RedundantPrefix)
        );
    }

    #[test]
    fn validate_draft_reuses_legacy_condition_dir_without_stripping() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        // Pre-create requirement/feature/behavior and a legacy condition dir
        // whose literal name still carries the redundant prefix.
        fs::create_dir_all(
            dir.path()
                .join("knowledge/controls/player-jump/jump/jump-ground"),
        )
        .unwrap();
        fs::write(
            dir.path().join("knowledge/controls/requirement.yml"),
            "id: controls\nlabel: controls\naxis: [gameplay]\n",
        )
        .unwrap();
        fs::write(
            dir.path()
                .join("knowledge/controls/player-jump/feature.yml"),
            "id: player-jump\nrequirement: controls\nlabel: player-jump\naxis: [gameplay, animation]\n",
        )
        .unwrap();
        fs::write(
            dir.path()
                .join("knowledge/controls/player-jump/jump/behavior.yml"),
            "id: jump\nfeature: player-jump\nlabel: jump\naxis: [gameplay]\ndescription: |\n  Player presses jump.\n",
        )
        .unwrap();
        fs::write(
            dir.path()
                .join("knowledge/controls/player-jump/jump/jump-ground/condition.yml"),
            "id: jump-ground\nbehavior: jump\nlabel: jump-ground\ndescription: |\n  legacy\n",
        )
        .unwrap();

        let mut draft = full_new_draft();
        draft.condition.id = "jump-ground".to_string();
        draft.condition.description = None;
        draft.condition.label = None;

        let errors = validate_draft(dir.path(), &draft, &no_strip());

        assert!(
            !errors
                .iter()
                .any(|e| e.code == ValidationErrorCode::RedundantPrefix)
        );
        assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
    }

    #[test]
    fn validate_draft_reports_parent_not_found_when_existing_feature_has_different_requirement() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        fs::create_dir_all(dir.path().join("knowledge/controls/player-jump")).unwrap();
        fs::write(
            dir.path().join("knowledge/controls/requirement.yml"),
            "id: controls\nlabel: controls\naxis: [gameplay]\n",
        )
        .unwrap();
        fs::write(
            dir.path()
                .join("knowledge/controls/player-jump/feature.yml"),
            "id: player-jump\nrequirement: some-other-requirement\nlabel: player-jump\naxis: [gameplay, animation]\n",
        )
        .unwrap();

        let mut draft = full_new_draft();
        draft.feature.axis = None;
        draft.feature.label = None;

        let errors = validate_draft(dir.path(), &draft, &no_strip());

        assert!(
            errors
                .iter()
                .any(|e| e.code == ValidationErrorCode::ParentNotFound
                    && e.path == "feature.requirement")
        );
    }

    #[test]
    fn validate_draft_reports_unknown_forked_from_when_referenced_feature_missing() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        let mut draft = full_new_draft();
        draft.feature.forked_from = Some("player-jump".to_string());

        let errors = validate_draft(dir.path(), &draft, &no_strip());

        let err = errors
            .iter()
            .find(|e| e.code == ValidationErrorCode::UnknownForkedFrom)
            .expect("expected an unknown_forked_from error");
        assert_eq!(err.path, "feature.forked_from");
        assert_eq!(err.value, Some("player-jump".to_string()));
    }

    #[test]
    fn validate_draft_allows_forked_from_when_referenced_feature_exists() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        fs::create_dir_all(dir.path().join("knowledge/controls/player-jump")).unwrap();
        fs::write(
            dir.path().join("knowledge/controls/requirement.yml"),
            "id: controls\nlabel: controls\naxis: [gameplay]\n",
        )
        .unwrap();
        fs::write(
            dir.path()
                .join("knowledge/controls/player-jump/feature.yml"),
            "id: player-jump\nrequirement: controls\nlabel: player-jump\naxis: [gameplay, animation]\n",
        )
        .unwrap();
        let mut draft = full_new_draft();
        draft.feature.id = "player-double-jump".to_string();
        draft.feature.forked_from = Some("player-jump".to_string());

        let errors = validate_draft(dir.path(), &draft, &no_strip());

        assert!(
            !errors
                .iter()
                .any(|e| e.code == ValidationErrorCode::UnknownForkedFrom)
        );
    }

    #[test]
    fn validate_draft_reports_conflicting_existing_value_when_label_differs() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        fs::create_dir_all(dir.path().join("knowledge/controls")).unwrap();
        fs::write(
            dir.path().join("knowledge/controls/requirement.yml"),
            "id: controls\nlabel: controls\naxis: [gameplay]\n",
        )
        .unwrap();

        let mut draft = full_new_draft();
        draft.requirement.label = Some("different-label".to_string());
        draft.requirement.axis = None;

        let errors = validate_draft(dir.path(), &draft, &no_strip());

        assert!(
            errors
                .iter()
                .any(|e| e.code == ValidationErrorCode::ConflictingExistingValue
                    && e.path == "requirement.label")
        );
    }

    #[test]
    fn validate_draft_succeeds_when_existing_id_reused_with_omitted_fields() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        fs::create_dir_all(dir.path().join("knowledge/controls")).unwrap();
        fs::write(
            dir.path().join("knowledge/controls/requirement.yml"),
            "id: controls\nlabel: controls\naxis: [gameplay]\n",
        )
        .unwrap();

        let mut draft = full_new_draft();
        draft.requirement.label = None;
        draft.requirement.axis = None;

        let errors = validate_draft(dir.path(), &draft, &no_strip());

        assert!(
            !errors.iter().any(|e| e.path.starts_with("requirement.")),
            "expected no requirement-level errors, got: {errors:?}"
        );
    }
}
