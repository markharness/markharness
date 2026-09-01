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

    let knowledge_root = root
        .join(crate::project_root::MARKHARNESS_DIR)
        .join("knowledge");

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
            uid: None,
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
            uid: None,
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
            preconditions: draft.behavior.steps.clone().unwrap_or_default(),
            uid: None,
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
            steps: draft.condition.steps.clone().unwrap_or_default(),
            additional_preconditions: draft
                .condition
                .additional_preconditions
                .clone()
                .unwrap_or_default(),
            uid: None,
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
            results: expected_draft.results.clone().unwrap_or_default(),
            additional_steps: expected_draft.additional_steps.clone(),
            implementation_note: expected_draft.implementation_note.clone(),
            generated_by: None,
            verified_by: None,
            uid: None,
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

/// One `--batch` draft file's validation outcome: `None` when it validated
/// cleanly, `Some` otherwise (parse failure or validation errors).
pub struct DraftValidation {
    pub file: PathBuf,
    pub error: Option<DraftFileError>,
}

pub struct BatchValidateResult {
    pub results: Vec<DraftValidation>,
}

impl BatchValidateResult {
    pub fn ok(&self) -> bool {
        self.results.iter().all(|r| r.error.is_none())
    }
}

/// Validates every `*.yml` file in `draft_paths` (already resolved and
/// sorted by the caller) without writing anything to `root`, and without
/// stopping at the first invalid file.
///
/// Mirrors `apply_batch`'s cumulative semantics: a later draft is validated
/// against the state left by every earlier *valid* draft in this same batch
/// (not against `root`'s state before the batch started), by replaying
/// `apply_draft` against an isolated copy of `root`'s `knowledge/` and
/// `axes/` under a temp directory that is discarded once every file has been
/// checked. A draft that fails to parse or validate contributes nothing to
/// that cumulative state — later drafts are checked as though it were never
/// in the batch — but checking continues through the rest of the batch
/// regardless, so every file's outcome is reported in one call.
pub fn validate_batch(
    root: &Path,
    draft_paths: &[PathBuf],
    options: &ValidateOptions,
) -> io::Result<BatchValidateResult> {
    let scratch = tempfile::tempdir()?;
    copy_dir_if_exists(
        &root
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge"),
        &scratch
            .path()
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge"),
    )?;
    copy_dir_if_exists(
        &root.join(crate::project_root::MARKHARNESS_DIR).join("axes"),
        &scratch
            .path()
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("axes"),
    )?;

    let apply_options = ApplyOptions {
        strip_redundant_prefix: options.strip_redundant_prefix,
    };

    let mut results = Vec::with_capacity(draft_paths.len());
    for draft_path in draft_paths {
        let file_name = draft_path
            .file_name()
            .map(PathBuf::from)
            .unwrap_or_else(|| draft_path.clone());

        let yaml = fs::read_to_string(draft_path)?;
        let draft = match knowledge_draft::parse_draft(&yaml) {
            Ok(draft) => draft,
            Err(e) => {
                results.push(DraftValidation {
                    file: file_name,
                    error: Some(DraftFileError::Parse(e.to_string())),
                });
                continue;
            }
        };

        match apply_draft(scratch.path(), &draft, &apply_options) {
            Ok(_) => results.push(DraftValidation {
                file: file_name,
                error: None,
            }),
            Err(ApplyError::Validation(errors)) => results.push(DraftValidation {
                file: file_name,
                error: Some(DraftFileError::Validation(errors)),
            }),
            Err(ApplyError::Io(e)) => return Err(e),
        }
    }

    Ok(BatchValidateResult { results })
}

/// Recursively copies `from` into `to` when `from` exists, creating `to`.
/// A no-op when `from` is missing (mirrors `load_axis_registry`'s and
/// `apply_draft`'s existing "absent directory means empty" convention).
fn copy_dir_if_exists(from: &Path, to: &Path) -> io::Result<()> {
    if !from.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let entry_path = entry.path();
        let dest_path = to.join(entry.file_name());
        if entry_path.is_dir() {
            copy_dir_if_exists(&entry_path, &dest_path)?;
        } else {
            fs::copy(&entry_path, &dest_path)?;
        }
    }
    Ok(())
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
  steps:
    - Press the jump button.

condition:
  id: ground
  label: ground
  description: Jump from the ground and land
  steps:
    - Do it.

expected:
  - description: lands safely
    results:
      - Confirmed.
";

    fn setup_root_with_axes(axis_ids: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        for id in axis_ids {
            fs::write(
                dir.path()
                    .join(crate::project_root::MARKHARNESS_DIR)
                    .join("axes")
                    .join(format!("{id}.yml")),
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

    fn no_strip_validate() -> ValidateOptions {
        ValidateOptions {
            strip_redundant_prefix: false,
        }
    }

    #[cfg(unix)]
    fn link_dir(link: &Path, target: &Path) {
        if let Some(parent) = link.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::os::unix::fs::symlink(target, link).unwrap();
    }

    #[cfg(windows)]
    fn link_dir(link: &Path, target: &Path) {
        if let Some(parent) = link.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
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
        let knowledge_dir = dir
            .path()
            .join(crate::project_root::MARKHARNESS_DIR)
            .join("knowledge");
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
        assert!(result.written_paths.contains(&PathBuf::from(
            ".markharness/knowledge/controls/requirement.yml"
        )));
        assert_eq!(
            fs::read_to_string(
                dir.path()
                    .join(".markharness/knowledge/controls/requirement.yml")
            )
            .unwrap(),
            "id: controls\nlabel: controls\naxis: [gameplay]\n"
        );
        assert_eq!(
            fs::read_to_string(
                dir.path()
                    .join(".markharness/knowledge/controls/player-jump/feature.yml")
            )
            .unwrap(),
            "id: player-jump\nrequirement: controls\nlabel: player-jump\naxis: [gameplay, animation]\n"
        );
        assert_eq!(
            fs::read_to_string(
                dir.path()
                    .join(".markharness/knowledge/controls/player-jump/jump/behavior.yml")
            )
            .unwrap(),
            "id: jump\nfeature: player-jump\nlabel: jump\naxis: [gameplay]\ndescription: |\n  Player presses jump.\npreconditions:\n  - \"Press the jump button.\"\n"
        );
        assert_eq!(
            fs::read_to_string(
                dir.path()
                    .join(".markharness/knowledge/controls/player-jump/jump/ground/condition.yml")
            )
            .unwrap(),
            "id: ground\nbehavior: jump\nlabel: ground\ndescription: |\n  Jump from the ground and land\nsteps:\n  - \"Do it.\"\nadditional_preconditions: []\n"
        );
        assert_eq!(
            fs::read_to_string(
                dir.path().join(
                    ".markharness/knowledge/controls/player-jump/jump/ground/expected/001.yml"
                )
            )
            .unwrap(),
            "id: ground-001\ncondition: ground\ndescription: |\n  lands safely\nresults:\n  - \"Confirmed.\"\n"
        );
    }

    #[test]
    fn apply_draft_does_not_write_anything_when_validation_fails() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        let mut draft = parse_draft(FULL_DRAFT_YAML).unwrap();
        draft.behavior.description = None;

        let result = apply_draft(dir.path(), &draft, &no_strip());

        assert!(matches!(result, Err(ApplyError::Validation(_))));
        assert!(!dir.path().join(".markharness/knowledge/controls").exists());
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
    results:
      - Confirmed.
";
        let reuse_draft = parse_draft(reuse_yaml).unwrap();
        let result = apply_draft(dir.path(), &reuse_draft, &no_strip()).unwrap();

        assert_eq!(result.written_paths.len(), 1);
        assert_eq!(
            fs::read_to_string(
                dir.path().join(
                    ".markharness/knowledge/controls/player-jump/jump/ground/expected/002.yml"
                )
            )
            .unwrap(),
            "id: ground-002\ncondition: ground\ndescription: |\n  falls over\nresults:\n  - \"Confirmed.\"\n"
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
  steps:
    - Press the jump button.

condition:
  id: ground
  label: ground
  description: Jump from the ground and land
  steps:
    - Do it.

expected:
  - description: lands safely
    results:
      - Confirmed.
  - description: takes fall damage if height > 3m
    results:
      - Confirmed.
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
                dir.path().join(
                    ".markharness/knowledge/controls/player-jump/jump/ground/expected/001.yml"
                )
            )
            .unwrap(),
            "id: ground-001\ncondition: ground\ndescription: |\n  lands safely\nresults:\n  - \"Confirmed.\"\n"
        );
        assert_eq!(
            fs::read_to_string(
                dir.path().join(
                    ".markharness/knowledge/controls/player-jump/jump/ground/expected/002.yml"
                )
            )
            .unwrap(),
            "id: ground-002\ncondition: ground\ndescription: |\n  takes fall damage if height > 3m\nresults:\n  - \"Confirmed.\"\n"
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
                .join(".markharness/knowledge/controls/player-jump/jump/ground/condition.yml")
                .exists()
        );
        assert!(
            !dir.path()
                .join(".markharness/knowledge/controls/player-jump/jump/jump-ground")
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
  steps:
    - Do it.

expected:
  - description: does not take fall damage
    results:
      - Confirmed.
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
                .join(".markharness/knowledge/controls/player-jump/jump/ground/condition.yml")
                .is_file()
        );
        assert!(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/jump/air/condition.yml")
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
                .join(".markharness/knowledge/controls/requirement.yml")
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
                .join(".markharness/knowledge/controls/requirement.yml")
                .exists(),
            "the first draft's files must be rolled back when the second draft fails to parse"
        );
    }

    #[test]
    fn validate_batch_reports_ok_for_a_single_valid_draft_and_does_not_touch_real_root() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        let drafts_dir = dir.path().join("drafts");
        fs::create_dir_all(&drafts_dir).unwrap();
        let first = write_draft_file(&drafts_dir, "01-ground.yml", FULL_DRAFT_YAML);

        let result = validate_batch(dir.path(), &[first], &no_strip_validate()).unwrap();

        assert!(result.ok());
        assert_eq!(result.results.len(), 1);
        assert!(result.results[0].error.is_none());
        assert!(
            !dir.path().join(".markharness/knowledge/controls").exists(),
            "validate_batch must not write into the real root"
        );
    }

    #[test]
    fn validate_batch_lets_a_later_draft_reuse_a_parent_an_earlier_draft_in_the_batch_creates() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        let drafts_dir = dir.path().join("drafts");
        fs::create_dir_all(&drafts_dir).unwrap();
        // Neither `controls`, `player-jump`, nor `jump` exist on disk before
        // this call — the second draft only supplies bare ids for them,
        // relying on the first draft (validated first, within this same
        // batch) to have created them.
        let first = write_draft_file(&drafts_dir, "01-ground.yml", FULL_DRAFT_YAML);
        let second = write_draft_file(
            &drafts_dir,
            "02-air.yml",
            SECOND_CONDITION_REUSING_PARENT_YAML,
        );

        let result = validate_batch(dir.path(), &[first, second], &no_strip_validate()).unwrap();

        assert!(
            result.ok(),
            "expected both drafts to validate, got {:?}",
            result.results.iter().map(|r| &r.error).collect::<Vec<_>>()
        );
    }

    #[test]
    fn validate_batch_reports_every_file_instead_of_stopping_at_the_first_failure() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        let drafts_dir = dir.path().join("drafts");
        fs::create_dir_all(&drafts_dir).unwrap();
        let broken = write_draft_file(&drafts_dir, "01-broken.yml", "not: [valid yaml");
        let valid = write_draft_file(&drafts_dir, "02-ground.yml", FULL_DRAFT_YAML);

        let result = validate_batch(dir.path(), &[broken, valid], &no_strip_validate()).unwrap();

        assert_eq!(
            result.results.len(),
            2,
            "both files must be reported, not just the first"
        );
        assert!(matches!(
            result.results[0].error,
            Some(DraftFileError::Parse(_))
        ));
        assert!(
            result.results[1].error.is_none(),
            "the second, valid file must still be checked and reported as valid"
        );
        assert!(!result.ok());
    }

    #[test]
    fn validate_batch_does_not_let_a_failed_draft_pollute_cumulative_state_for_later_drafts() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        let drafts_dir = dir.path().join("drafts");
        fs::create_dir_all(&drafts_dir).unwrap();
        // A new condition ("air") with no description: condition.description
        // is required whenever the condition doesn't already exist. This
        // draft fails validation, so its parent chain (controls/player-jump/
        // jump) must NOT be treated as already existing by the next draft.
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
        let first = write_draft_file(&drafts_dir, "01-air.yml", invalid_yaml);
        // References the same bare parent ids as the failed draft above.
        // Since that draft never actually applied, `controls`/`player-jump`/
        // `jump` still don't exist, so this draft must fail too (missing
        // axis/description for entries that don't yet exist) rather than
        // succeeding as if the first draft's parents had been created.
        let second = write_draft_file(
            &drafts_dir,
            "02-ground.yml",
            "\
requirement:
  id: controls

feature:
  id: player-jump

behavior:
  id: jump

condition:
  id: ground
  label: ground
  description: Jump from the ground and land

expected:
  - description: lands safely
",
        );

        let result = validate_batch(dir.path(), &[first, second], &no_strip_validate()).unwrap();

        assert!(result.results[0].error.is_some());
        assert!(
            result.results[1].error.is_some(),
            "the second draft must not see the first draft's never-applied parent chain as existing"
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
