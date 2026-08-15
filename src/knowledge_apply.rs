use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::fs_safety::replace_file;
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
            source: None,
            related_issues: Vec::new(),
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
            forked_from: draft.feature.forked_from.clone(),
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
            generated_by: None,
            verified_by: None,
        };
        let expected_path = expected_dir.join(format!("{seq:03}.yml"));
        pending.push((
            expected_path,
            knowledge::serialize_expected_result(&expected),
        ));
    }

    write_all_atomically(root, &pending).map_err(ApplyError::Io)?;

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

fn write_all_atomically(root: &Path, pending: &[(PathBuf, String)]) -> io::Result<()> {
    let mut written: Vec<PathBuf> = Vec::new();

    for (path, content) in pending {
        if let Err(e) = write_one(root, path, content) {
            for written_path in &written {
                // written_path was just written by write_one (replace_file)
                // in this same loop, so it's a known-good file we produced,
                // not attacker-controlled input; rolling it back here
                // doesn't need the symlink guards replace_file already
                // applied when creating it.
                #[allow(clippy::disallowed_methods)]
                let _ = fs::remove_file(written_path);
            }
            return Err(e);
        }
        written.push(path.clone());
    }

    Ok(())
}

fn write_one(root: &Path, path: &Path, content: &str) -> io::Result<()> {
    replace_file(root, path, content.as_bytes())
}

#[derive(Debug)]
pub struct BatchApplyResult {
    pub written_paths: Vec<PathBuf>,
}

/// Why one draft file within a `--batch` directory could not be applied.
#[derive(Debug)]
pub enum DraftFileError {
    Parse(String),
    Validation(Vec<ValidationError>),
}

#[derive(Debug)]
pub enum BatchApplyError {
    /// `file` (its name within the batch directory) failed to parse or
    /// validate. Every file this batch call had already written for earlier
    /// drafts has been removed before this is returned, so the call has zero
    /// net effect on `knowledge/` — see `apply_batch`'s doc comment for what
    /// "atomic" means here precisely.
    Draft {
        file: PathBuf,
        error: DraftFileError,
    },
    Io(io::Error),
}

/// Validates and applies every `*.yml` file in `draft_paths` (already
/// resolved and sorted by the caller — typically a batch directory's direct
/// children in file-name order), in order.
///
/// Each draft is validated against `knowledge/`'s state as it stands *after*
/// every earlier draft in this same batch has been applied — not against the
/// state before the batch started. This lets a later draft in the batch
/// reuse a Requirement/Feature/Behavior an earlier draft in the same batch
/// just created (the common case this exists for: many small Condition
/// drafts sharing one new parent chain), the same way it could reuse one
/// that already existed on disk before the batch ran.
///
/// If any draft fails to parse or validate, every file already written by
/// this batch call (by earlier, successful drafts) is deleted before
/// returning the error, so the directory tree ends up exactly as it started
/// — an all-or-nothing outcome for the whole batch, even though each draft
/// is validated incrementally rather than all at once against a single
/// snapshot.
pub fn apply_batch(
    root: &Path,
    draft_paths: &[PathBuf],
    options: &ApplyOptions,
) -> Result<BatchApplyResult, BatchApplyError> {
    let mut all_written: Vec<PathBuf> = Vec::new();

    for draft_path in draft_paths {
        let file_name = draft_path
            .file_name()
            .map(PathBuf::from)
            .unwrap_or_else(|| draft_path.clone());

        let yaml = match fs::read_to_string(draft_path) {
            Ok(yaml) => yaml,
            Err(e) => {
                rollback(root, &all_written);
                return Err(BatchApplyError::Io(e));
            }
        };
        let draft = match knowledge_draft::parse_draft(&yaml) {
            Ok(draft) => draft,
            Err(e) => {
                rollback(root, &all_written);
                return Err(BatchApplyError::Draft {
                    file: file_name,
                    error: DraftFileError::Parse(e.to_string()),
                });
            }
        };

        match apply_draft(root, &draft, options) {
            Ok(result) => all_written.extend(result.written_paths),
            Err(ApplyError::Validation(errors)) => {
                rollback(root, &all_written);
                return Err(BatchApplyError::Draft {
                    file: file_name,
                    error: DraftFileError::Validation(errors),
                });
            }
            Err(ApplyError::Io(e)) => {
                rollback(root, &all_written);
                return Err(BatchApplyError::Io(e));
            }
        }
    }

    Ok(BatchApplyResult {
        written_paths: all_written,
    })
}

/// Removes every file this batch call wrote, undoing a partially-applied
/// batch. `written` holds paths relative to `root` (as returned by
/// `apply_draft`), all of which this same call just created via
/// `replace_file`, so no symlink guard is needed to remove them (mirrors
/// `write_all_atomically`'s single-draft rollback).
fn rollback(root: &Path, written: &[PathBuf]) {
    for path in written {
        #[allow(clippy::disallowed_methods)]
        let _ = fs::remove_file(root.join(path));
    }
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

    #[cfg(unix)]
    fn link_dir(link: &Path, target: &Path) {
        std::os::unix::fs::symlink(target, link).unwrap();
    }

    #[cfg(windows)]
    fn link_dir(link: &Path, target: &Path) {
        let status = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/j"])
            .arg(link)
            .arg(target)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "mklink /j failed");
    }

    #[test]
    fn apply_draft_refuses_to_follow_a_symlinked_knowledge_dir() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        let draft = parse_draft(FULL_DRAFT_YAML).unwrap();
        let outside = tempfile::tempdir().unwrap();
        let knowledge_dir = dir.path().join("knowledge");
        fs::remove_dir_all(&knowledge_dir).unwrap();
        link_dir(&knowledge_dir, outside.path());

        let result = apply_draft(dir.path(), &draft, &no_strip());

        assert!(
            result.is_err(),
            "expected apply_draft to refuse a symlinked knowledge/ dir"
        );
        assert!(
            !outside.path().join("controls").exists(),
            "apply_draft must not write through the symlinked ancestor"
        );
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

    const SECOND_CONDITION_REUSING_PARENT_YAML: &str = "\
requirement:
  id: controls

feature:
  id: player-jump

behavior:
  id: jump

condition:
  id: air
  label: air
  description: Jump in the air.

expected:
  - description: does not take fall damage
";

    fn write_draft_file(dir: &std::path::Path, name: &str, yaml: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, yaml).unwrap();
        path
    }

    #[test]
    fn apply_batch_applies_every_draft_and_returns_their_combined_written_paths() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        let drafts_dir = dir.path().join("drafts");
        fs::create_dir_all(&drafts_dir).unwrap();
        let first = write_draft_file(&drafts_dir, "01-ground.yml", FULL_DRAFT_YAML);
        let second = write_draft_file(
            &drafts_dir,
            "02-air.yml",
            SECOND_CONDITION_REUSING_PARENT_YAML,
        );

        let result = apply_batch(dir.path(), &[first, second], &no_strip()).unwrap();

        assert_eq!(result.written_paths.len(), 7); // 5 from the first draft + 2 (condition.yml, expected/001.yml) from the second
        assert!(
            dir.path()
                .join("knowledge/controls/player-jump/jump/ground/condition.yml")
                .is_file()
        );
        assert!(
            dir.path()
                .join("knowledge/controls/player-jump/jump/air/condition.yml")
                .is_file()
        );
    }

    #[test]
    fn apply_batch_lets_a_later_draft_reuse_a_parent_an_earlier_draft_in_the_batch_just_created() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        let drafts_dir = dir.path().join("drafts");
        fs::create_dir_all(&drafts_dir).unwrap();
        // Neither `controls`, `player-jump`, nor `jump` exist on disk before
        // this call — the second draft only supplies bare ids for them,
        // relying on the first draft (applied first, within this same
        // batch) to have created them.
        let first = write_draft_file(&drafts_dir, "01-ground.yml", FULL_DRAFT_YAML);
        let second = write_draft_file(
            &drafts_dir,
            "02-air.yml",
            SECOND_CONDITION_REUSING_PARENT_YAML,
        );

        let result = apply_batch(dir.path(), &[first, second], &no_strip());

        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    #[test]
    fn apply_batch_rolls_back_every_file_when_a_later_draft_fails_validation() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        let drafts_dir = dir.path().join("drafts");
        fs::create_dir_all(&drafts_dir).unwrap();
        let first = write_draft_file(&drafts_dir, "01-ground.yml", FULL_DRAFT_YAML);
        // A new condition ("air") with no description: condition.description
        // is required whenever the condition doesn't already exist.
        let invalid_yaml = "\
requirement:
  id: controls

feature:
  id: player-jump

behavior:
  id: jump

condition:
  id: air
  label: air
";
        let second = write_draft_file(&drafts_dir, "02-air.yml", invalid_yaml);

        let result = apply_batch(dir.path(), &[first, second], &no_strip());

        assert!(matches!(
            result,
            Err(BatchApplyError::Draft {
                error: DraftFileError::Validation(_),
                ..
            })
        ));
        assert!(
            !dir.path()
                .join("knowledge/controls/requirement.yml")
                .exists(),
            "the first draft's files must be rolled back when the second draft is invalid"
        );
    }

    #[test]
    fn apply_batch_rolls_back_every_file_when_a_later_draft_fails_to_parse() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        let drafts_dir = dir.path().join("drafts");
        fs::create_dir_all(&drafts_dir).unwrap();
        let first = write_draft_file(&drafts_dir, "01-ground.yml", FULL_DRAFT_YAML);
        let second = write_draft_file(&drafts_dir, "02-broken.yml", "not: [valid yaml");

        let result = apply_batch(dir.path(), &[first, second], &no_strip());

        assert!(matches!(
            result,
            Err(BatchApplyError::Draft {
                error: DraftFileError::Parse(_),
                ..
            })
        ));
        assert!(
            !dir.path()
                .join("knowledge/controls/requirement.yml")
                .exists(),
            "the first draft's files must be rolled back when the second draft fails to parse"
        );
    }

    #[test]
    fn apply_batch_reports_which_file_failed() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        let drafts_dir = dir.path().join("drafts");
        fs::create_dir_all(&drafts_dir).unwrap();
        let bad = write_draft_file(&drafts_dir, "broken.yml", "not: [valid yaml");

        let result = apply_batch(dir.path(), &[bad], &no_strip());

        match result {
            Err(BatchApplyError::Draft { file, .. }) => {
                assert_eq!(file, PathBuf::from("broken.yml"))
            }
            other => panic!("expected Draft error, got {other:?}"),
        }
    }
}
