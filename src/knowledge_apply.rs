use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::knowledge::{self, Behavior, Condition, ExpectedResult, Feature, Requirement};
use crate::knowledge_draft::{self, KnowledgeDraft, ValidateOptions, ValidationError};

pub struct ApplyOptions {
    pub strip_redundant_prefix: bool,
}

pub struct ApplyResult {
    pub written_paths: Vec<PathBuf>,
}

#[derive(Debug)]
pub enum ApplyError {
    Validation(Vec<ValidationError>),
    Io(io::Error),
}

impl std::fmt::Display for ApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApplyError::Validation(errors) => write!(f, "{} validation error(s)", errors.len()),
            ApplyError::Io(e) => write!(f, "io error: {e}"),
        }
    }
}

impl std::error::Error for ApplyError {}

pub fn apply_draft(
    root: &Path,
    draft: &KnowledgeDraft,
    options: &ApplyOptions,
) -> Result<ApplyResult, ApplyError> {
    let validate_options = ValidateOptions {
        strip_redundant_prefix: options.strip_redundant_prefix,
    };
    let errors = knowledge_draft::validate_draft(root, draft, &validate_options);
    if !errors.is_empty() {
        return Err(ApplyError::Validation(errors));
    }

    let knowledge_root = root.join("knowledge");

    let requirement_dir = knowledge_root.join(&draft.requirement.id);
    let requirement_path = requirement_dir.join("requirement.yml");
    let requirement_exists = requirement_path.is_file();

    let feature_dir = requirement_dir.join(&draft.feature.id);
    let feature_path = feature_dir.join("feature.yml");
    let feature_exists = feature_path.is_file();

    let behavior_dir = feature_dir.join(&draft.behavior.id);
    let behavior_path = behavior_dir.join("behavior.yml");
    let behavior_exists = behavior_path.is_file();

    let effective_condition_id = knowledge_draft::resolve_effective_condition_id(
        &behavior_dir,
        &draft.behavior.id,
        &draft.condition.id,
        options.strip_redundant_prefix,
    );
    let condition_dir = behavior_dir.join(&effective_condition_id);
    let condition_path = condition_dir.join("condition.yml");
    let condition_exists = condition_path.is_file();

    let expected_dir = condition_dir.join("expected");
    let existing_expected_count = fs::read_dir(&expected_dir)
        .map(|entries| entries.filter(|e| e.is_ok()).count())
        .unwrap_or(0);

    let mut pending: Vec<(PathBuf, String)> = Vec::new();

    if !requirement_exists {
        let requirement = Requirement {
            id: draft.requirement.id.clone(),
            label: draft
                .requirement
                .label
                .clone()
                .unwrap_or_else(|| draft.requirement.id.clone()),
            axis: draft.requirement.axis.clone().unwrap_or_default(),
            description: draft.requirement.description.clone(),
        };
        pending.push((
            requirement_path,
            knowledge::serialize_requirement(&requirement),
        ));
    }

    if !feature_exists {
        let feature = Feature {
            id: draft.feature.id.clone(),
            requirement: draft.requirement.id.clone(),
            label: draft
                .feature
                .label
                .clone()
                .unwrap_or_else(|| draft.feature.id.clone()),
            axis: draft.feature.axis.clone().unwrap_or_default(),
            description: draft.feature.description.clone(),
        };
        pending.push((feature_path, knowledge::serialize_feature(&feature)));
    }

    if !behavior_exists {
        let behavior = Behavior {
            id: draft.behavior.id.clone(),
            feature: draft.feature.id.clone(),
            label: draft
                .behavior
                .label
                .clone()
                .unwrap_or_else(|| draft.behavior.id.clone()),
            axis: draft.behavior.axis.clone().unwrap_or_default(),
            description: draft.behavior.description.clone().unwrap_or_default(),
        };
        pending.push((behavior_path, knowledge::serialize_behavior(&behavior)));
    }

    if !condition_exists {
        let condition = Condition {
            id: effective_condition_id.clone(),
            behavior: draft.behavior.id.clone(),
            label: draft
                .condition
                .label
                .clone()
                .unwrap_or_else(|| effective_condition_id.clone()),
            description: draft.condition.description.clone().unwrap_or_default(),
        };
        pending.push((condition_path, knowledge::serialize_condition(&condition)));
    }

    for (i, expected_draft) in draft.expected.iter().enumerate() {
        let seq = existing_expected_count + i + 1;
        let expected_id = format!("{effective_condition_id}-{seq:03}");
        let expected = ExpectedResult {
            id: expected_id,
            condition: effective_condition_id.clone(),
            description: expected_draft.description.clone(),
        };
        let expected_path = expected_dir.join(format!("{seq:03}.yml"));
        pending.push((
            expected_path,
            knowledge::serialize_expected_result(&expected),
        ));
    }

    write_all_atomically(&pending).map_err(ApplyError::Io)?;

    Ok(ApplyResult {
        written_paths: pending
            .into_iter()
            .map(|(path, _)| {
                path.strip_prefix(root)
                    .map(|relative| relative.to_path_buf())
                    .unwrap_or(path)
            })
            .collect(),
    })
}

fn write_all_atomically(pending: &[(PathBuf, String)]) -> io::Result<()> {
    let mut written: Vec<PathBuf> = Vec::new();

    for (path, content) in pending {
        if let Err(e) = write_one(path, content) {
            for written_path in &written {
                let _ = fs::remove_file(written_path);
            }
            return Err(e);
        }
        written.push(path.clone());
    }

    Ok(())
}

fn write_one(path: &Path, content: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp_path = path.with_extension("yml.tmp");
    fs::write(&tmp_path, content)?;
    fs::rename(&tmp_path, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge_draft::parse_draft;

    const FULL_DRAFT_YAML: &str = "\
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

    fn setup_root_with_axes(axis_ids: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        for id in axis_ids {
            fs::write(
                dir.path().join("axes").join(format!("{id}.yml")),
                format!("id: {id}\nlabel: {id}\n"),
            )
            .unwrap();
        }
        dir
    }

    fn no_strip() -> ApplyOptions {
        ApplyOptions {
            strip_redundant_prefix: false,
        }
    }

    #[test]
    fn apply_draft_creates_new_requirement_feature_behavior_condition_and_expected_from_scratch() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        let draft = parse_draft(FULL_DRAFT_YAML).unwrap();

        let result = apply_draft(dir.path(), &draft, &no_strip()).unwrap();

        assert_eq!(result.written_paths.len(), 5);
        assert!(
            result
                .written_paths
                .contains(&PathBuf::from("knowledge/controls/requirement.yml"))
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("knowledge/controls/requirement.yml")).unwrap(),
            "id: controls\nlabel: controls\naxis: [gameplay]\n"
        );
        assert_eq!(
            fs::read_to_string(
                dir.path()
                    .join("knowledge/controls/player-jump/feature.yml")
            )
            .unwrap(),
            "id: player-jump\nrequirement: controls\nlabel: player-jump\naxis: [gameplay, animation]\n"
        );
        assert_eq!(
            fs::read_to_string(
                dir.path()
                    .join("knowledge/controls/player-jump/jump/behavior.yml")
            )
            .unwrap(),
            "id: jump\nfeature: player-jump\nlabel: jump\naxis: [gameplay]\ndescription: |\n  Player presses jump.\n"
        );
        assert_eq!(
            fs::read_to_string(
                dir.path()
                    .join("knowledge/controls/player-jump/jump/ground/condition.yml")
            )
            .unwrap(),
            "id: ground\nbehavior: jump\nlabel: ground\ndescription: |\n  Jump from the ground and land\n"
        );
        assert_eq!(
            fs::read_to_string(
                dir.path()
                    .join("knowledge/controls/player-jump/jump/ground/expected/001.yml")
            )
            .unwrap(),
            "id: ground-001\ncondition: ground\ndescription: |\n  lands safely\n"
        );
    }

    #[test]
    fn apply_draft_does_not_write_anything_when_validation_fails() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        let mut draft = parse_draft(FULL_DRAFT_YAML).unwrap();
        draft.behavior.description = None;

        let result = apply_draft(dir.path(), &draft, &no_strip());

        assert!(matches!(result, Err(ApplyError::Validation(_))));
        assert!(!dir.path().join("knowledge/controls").exists());
    }

    #[test]
    fn apply_draft_reuses_existing_files_and_only_appends_new_expected() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        let draft = parse_draft(FULL_DRAFT_YAML).unwrap();
        apply_draft(dir.path(), &draft, &no_strip()).unwrap();

        let reuse_yaml = "\
requirement:
  id: controls

feature:
  id: player-jump

behavior:
  id: jump

condition:
  id: ground

expected:
  - description: falls over
";
        let reuse_draft = parse_draft(reuse_yaml).unwrap();
        let result = apply_draft(dir.path(), &reuse_draft, &no_strip()).unwrap();

        assert_eq!(result.written_paths.len(), 1);
        assert_eq!(
            fs::read_to_string(
                dir.path()
                    .join("knowledge/controls/player-jump/jump/ground/expected/002.yml")
            )
            .unwrap(),
            "id: ground-002\ncondition: ground\ndescription: |\n  falls over\n"
        );
    }

    #[test]
    fn apply_draft_numbers_multiple_expected_entries_sequentially() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        let yaml = "\
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
  - description: takes fall damage if height > 3m
";
        let draft = parse_draft(yaml).unwrap();

        let result = apply_draft(dir.path(), &draft, &no_strip()).unwrap();

        let expected_paths: Vec<_> = result
            .written_paths
            .iter()
            .filter(|p| p.to_string_lossy().contains("expected"))
            .collect();
        assert_eq!(expected_paths.len(), 2);
        assert_eq!(
            fs::read_to_string(
                dir.path()
                    .join("knowledge/controls/player-jump/jump/ground/expected/001.yml")
            )
            .unwrap(),
            "id: ground-001\ncondition: ground\ndescription: |\n  lands safely\n"
        );
        assert_eq!(
            fs::read_to_string(
                dir.path()
                    .join("knowledge/controls/player-jump/jump/ground/expected/002.yml")
            )
            .unwrap(),
            "id: ground-002\ncondition: ground\ndescription: |\n  takes fall damage if height > 3m\n"
        );
    }

    #[test]
    fn apply_draft_strips_redundant_condition_prefix_when_flag_set() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        let mut draft = parse_draft(FULL_DRAFT_YAML).unwrap();
        draft.condition.id = "jump-ground".to_string();

        let result = apply_draft(
            dir.path(),
            &draft,
            &ApplyOptions {
                strip_redundant_prefix: true,
            },
        )
        .unwrap();

        assert!(
            dir.path()
                .join("knowledge/controls/player-jump/jump/ground/condition.yml")
                .exists()
        );
        assert!(
            !dir.path()
                .join("knowledge/controls/player-jump/jump/jump-ground")
                .exists()
        );
        assert!(
            result
                .written_paths
                .iter()
                .any(|p| p.ends_with("ground/condition.yml"))
        );
    }
}
