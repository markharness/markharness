use std::collections::HashSet;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

use crate::knowledge::is_valid_slug;
use crate::knowledge_apply::{self, ApplyError, ApplyOptions, ApplyResult};
use crate::knowledge_draft::{self, KnowledgeDraft, ValidationError};

/// Blank draft chain written to the temp file before the editor is first
/// opened. Mirrors the draft YAML shape documented in
/// docs/cli-manual.md §1.3 (`knowledge validate`).
pub const EDIT_TEMPLATE: &str = "\
# knowledge add --edit
# Fill in the chain below (existing ids may omit label/axis/description),
# then save and exit the editor to apply. Validation errors reopen the editor.
requirement:
  id:
  label:
  axis: []

feature:
  id:
  label:
  axis: []

behavior:
  id:
  label:
  axis: []
  description:

condition:
  id:
  label:
  description:

expected:
  - description:
";

/// Reads `VISUAL` then `EDITOR`, mirroring common CLI editor precedence
/// (e.g. git). Returns `None` when neither is set or both are blank.
pub fn resolve_editor_command() -> Option<String> {
    env::var("VISUAL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| env::var("EDITOR").ok().filter(|v| !v.trim().is_empty()))
}

#[derive(Debug)]
pub enum EditFlowError {
    Io(io::Error),
}

impl From<io::Error> for EditFlowError {
    fn from(e: io::Error) -> Self {
        EditFlowError::Io(e)
    }
}

/// Collects axis values referenced by `draft` (across Requirement/Feature/
/// Behavior) that are safe to auto-register: not already in `axis_registry`,
/// no near-match candidate already registered (a likely typo, per
/// `knowledge_draft::nearest_axis_suggestion`), and a valid slug. Values
/// failing either safety check are left out so the normal `unknown_axis`
/// validation error still surfaces them.
pub fn new_axis_candidates(draft: &KnowledgeDraft, axis_registry: &HashSet<String>) -> Vec<String> {
    let mut referenced: Vec<&String> = Vec::new();
    for axis in [
        &draft.requirement.axis,
        &draft.feature.axis,
        &draft.behavior.axis,
    ]
    .into_iter()
    .flatten()
    {
        referenced.extend(axis.iter());
    }

    let mut candidates: Vec<String> = referenced
        .into_iter()
        .filter(|value| !axis_registry.contains(*value))
        .filter(|value| is_valid_slug(value))
        .filter(|value| knowledge_draft::nearest_axis_suggestion(value, axis_registry).is_none())
        .cloned()
        .collect();
    candidates.sort();
    candidates.dedup();
    candidates
}

fn print_parse_error<W: Write>(writer: &mut W, message: impl std::fmt::Display) -> io::Result<()> {
    writeln!(writer, "error: {message}")?;
    writeln!(
        writer,
        "編集内容を確認し、保存してエディタを終了してください。"
    )
}

fn print_validation_errors<W: Write>(writer: &mut W, errors: &[ValidationError]) -> io::Result<()> {
    for e in errors {
        let mut detail = String::new();
        if let Some(suggestion) = &e.suggestion {
            detail.push_str(&format!("suggested=\"{suggestion}\", "));
        }
        detail.push_str(&format!("path={}", e.path));
        writeln!(
            writer,
            "error: {}: {} ({detail})",
            e.code.as_str(),
            e.message
        )?;
    }
    writeln!(writer, "エディタを再度開いて修正してください。")
}

/// Writes the blank template to `tmp_path`, then repeatedly invokes
/// `invoke_editor` and validates/applies the result until it succeeds.
/// `invoke_editor` is injected so a real `$EDITOR` spawn (production) can be
/// swapped for a deterministic double (tests) that simulates a user editing
/// the file across multiple passes.
pub fn run_edit_loop<W: Write>(
    root: &Path,
    tmp_path: &Path,
    mut invoke_editor: impl FnMut(&Path) -> io::Result<()>,
    writer: &mut W,
) -> Result<ApplyResult, EditFlowError> {
    fs::write(tmp_path, EDIT_TEMPLATE)?;
    loop {
        invoke_editor(tmp_path)?;
        let yaml = fs::read_to_string(tmp_path)?;
        let draft = match knowledge_draft::parse_draft(&yaml) {
            Ok(draft) => draft,
            Err(e) => {
                print_parse_error(writer, e)?;
                continue;
            }
        };

        let axis_registry = knowledge_draft::load_axis_registry(root);
        for axis_id in new_axis_candidates(&draft, &axis_registry) {
            crate::axes::create_axis(root, &axis_id)?;
            writeln!(
                writer,
                "axis '{axis_id}' を新規登録しました (axes/{axis_id}.yml)"
            )?;
        }

        let options = ApplyOptions {
            strip_redundant_prefix: false,
        };
        match knowledge_apply::apply_draft(root, &draft, &options) {
            Ok(result) => return Ok(result),
            Err(ApplyError::Validation(errors)) => {
                print_validation_errors(writer, &errors)?;
                continue;
            }
            Err(ApplyError::Io(e)) => return Err(EditFlowError::Io(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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

    fn registry(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    fn draft_with_axes(
        requirement_axis: &[&str],
        feature_axis: &[&str],
        behavior_axis: &[&str],
    ) -> KnowledgeDraft {
        let to_vec = |values: &[&str]| Some(values.iter().map(|s| s.to_string()).collect());
        KnowledgeDraft {
            requirement: knowledge_draft::RequirementDraft {
                id: "controls".to_string(),
                label: None,
                axis: to_vec(requirement_axis),
                description: None,
            },
            feature: knowledge_draft::FeatureDraft {
                id: "player-jump".to_string(),
                label: None,
                axis: to_vec(feature_axis),
                description: None,
                forked_from: None,
            },
            behavior: knowledge_draft::BehaviorDraft {
                id: "jump".to_string(),
                label: None,
                axis: to_vec(behavior_axis),
                description: None,
            },
            condition: knowledge_draft::ConditionDraft {
                id: "ground".to_string(),
                label: None,
                description: None,
            },
            expected: Vec::new(),
        }
    }

    #[test]
    fn new_axis_candidates_includes_unregistered_value_with_no_near_match() {
        let draft = draft_with_axes(&["state"], &[], &[]);

        let candidates = new_axis_candidates(&draft, &registry(&["gameplay"]));

        assert_eq!(candidates, vec!["state".to_string()]);
    }

    #[test]
    fn new_axis_candidates_excludes_already_registered_value() {
        let draft = draft_with_axes(&["gameplay"], &[], &[]);

        let candidates = new_axis_candidates(&draft, &registry(&["gameplay"]));

        assert!(candidates.is_empty());
    }

    #[test]
    fn new_axis_candidates_excludes_value_with_near_match_suggestion() {
        let draft = draft_with_axes(&["validaton"], &[], &[]);

        let candidates = new_axis_candidates(&draft, &registry(&["validation"]));

        assert!(candidates.is_empty());
    }

    #[test]
    fn new_axis_candidates_excludes_value_with_invalid_slug_format() {
        let draft = draft_with_axes(&["UI"], &[], &[]);

        let candidates = new_axis_candidates(&draft, &registry(&[]));

        assert!(candidates.is_empty());
    }

    #[test]
    fn new_axis_candidates_deduplicates_across_hierarchy_levels_and_sorts() {
        let draft = draft_with_axes(&["state", "ui"], &["ui"], &["network"]);

        let candidates = new_axis_candidates(&draft, &registry(&[]));

        assert_eq!(
            candidates,
            vec!["network".to_string(), "state".to_string(), "ui".to_string()]
        );
    }

    #[test]
    fn new_axis_candidates_partially_creates_only_the_genuinely_new_values() {
        let draft = draft_with_axes(&["state", "validaton"], &[], &[]);

        let candidates = new_axis_candidates(&draft, &registry(&["validation"]));

        assert_eq!(candidates, vec!["state".to_string()]);
    }

    const VALID_DRAFT: &str = "\
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

    /// Returns a closure that writes each entry of `contents` to the edited
    /// path in order, one per invocation (simulating successive editor
    /// sessions), and records how many times it was called.
    fn scripted_editor(
        contents: Vec<&'static str>,
    ) -> (
        impl FnMut(&Path) -> io::Result<()>,
        std::rc::Rc<std::cell::Cell<usize>>,
    ) {
        let call_count = std::rc::Rc::new(std::cell::Cell::new(0));
        let counter = call_count.clone();
        let mut remaining = contents.into_iter();
        let editor = move |path: &Path| -> io::Result<()> {
            counter.set(counter.get() + 1);
            let content = remaining
                .next()
                .expect("editor invoked more times than scripted");
            fs::write(path, content)
        };
        (editor, call_count)
    }

    #[test]
    fn writes_blank_template_before_first_editor_invocation() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        let tmp_path = dir.path().join("edit.yml");
        let mut seen_on_first_open = String::new();
        let editor = |path: &Path| -> io::Result<()> {
            seen_on_first_open = fs::read_to_string(path)?;
            fs::write(path, VALID_DRAFT)
        };
        let mut writer = Vec::new();

        run_edit_loop(dir.path(), &tmp_path, editor, &mut writer).unwrap();

        assert_eq!(seen_on_first_open, EDIT_TEMPLATE);
    }

    #[test]
    fn applies_successfully_when_first_edit_is_valid() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        let tmp_path: PathBuf = dir.path().join("edit.yml");
        let (editor, call_count) = scripted_editor(vec![VALID_DRAFT]);
        let mut writer = Vec::new();

        let result = run_edit_loop(dir.path(), &tmp_path, editor, &mut writer).unwrap();

        assert_eq!(call_count.get(), 1);
        assert!(
            dir.path()
                .join("knowledge/controls/player-jump/jump/ground/condition.yml")
                .exists()
        );
        assert!(!result.written_paths.is_empty());
    }

    #[test]
    fn reopens_editor_after_unparsable_yaml() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        let tmp_path: PathBuf = dir.path().join("edit.yml");
        let (editor, call_count) =
            scripted_editor(vec!["requirement: [not a mapping", VALID_DRAFT]);
        let mut writer = Vec::new();

        run_edit_loop(dir.path(), &tmp_path, editor, &mut writer).unwrap();

        assert_eq!(call_count.get(), 2);
        let output = String::from_utf8(writer).unwrap();
        assert!(output.contains("error:"));
    }

    #[test]
    fn reopens_editor_after_validation_error_then_succeeds() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        let tmp_path: PathBuf = dir.path().join("edit.yml");
        let missing_description = VALID_DRAFT.replace("description: Player presses jump.\n", "");
        let (editor, call_count) = scripted_editor(vec![
            Box::leak(missing_description.into_boxed_str()),
            VALID_DRAFT,
        ]);
        let mut writer = Vec::new();

        run_edit_loop(dir.path(), &tmp_path, editor, &mut writer).unwrap();

        assert_eq!(call_count.get(), 2);
        let output = String::from_utf8(writer).unwrap();
        assert!(output.contains("missing_description"));
    }

    #[test]
    fn run_edit_loop_auto_creates_new_axis_with_no_near_match_and_succeeds_first_try() {
        let dir = setup_root_with_axes(&["animation"]);
        let tmp_path: PathBuf = dir.path().join("edit.yml");
        let (editor, call_count) = scripted_editor(vec![VALID_DRAFT]);
        let mut writer = Vec::new();

        run_edit_loop(dir.path(), &tmp_path, editor, &mut writer).unwrap();

        assert_eq!(call_count.get(), 1);
        assert_eq!(
            fs::read_to_string(dir.path().join("axes/gameplay.yml")).unwrap(),
            "id: gameplay\nlabel: gameplay\n"
        );
        let output = String::from_utf8(writer).unwrap();
        assert!(output.contains("axis 'gameplay' を新規登録しました"));
    }

    #[test]
    fn run_edit_loop_does_not_auto_create_axis_with_near_match_suggestion() {
        let dir = setup_root_with_axes(&["gameplay", "animation", "validation"]);
        let tmp_path: PathBuf = dir.path().join("edit.yml");
        let typo_draft = VALID_DRAFT.replace(
            "axis: [gameplay]\n\nfeature",
            "axis: [validaton]\n\nfeature",
        );
        let fixed_draft = VALID_DRAFT.replace(
            "axis: [gameplay]\n\nfeature",
            "axis: [validation]\n\nfeature",
        );
        let (editor, call_count) = scripted_editor(vec![
            Box::leak(typo_draft.into_boxed_str()),
            Box::leak(fixed_draft.into_boxed_str()),
        ]);
        let mut writer = Vec::new();

        run_edit_loop(dir.path(), &tmp_path, editor, &mut writer).unwrap();

        assert_eq!(call_count.get(), 2);
        assert!(!dir.path().join("axes/validaton.yml").exists());
        let output = String::from_utf8(writer).unwrap();
        assert!(output.contains("unknown_axis"));
        assert!(
            output.contains("suggested=\"validation\""),
            "expected a suggestion pointing at the near-match, got: {output}"
        );
    }

    #[test]
    fn run_edit_loop_partially_creates_new_axis_while_leaving_near_match_as_error() {
        let dir = setup_root_with_axes(&["gameplay", "animation", "validation"]);
        let tmp_path: PathBuf = dir.path().join("edit.yml");
        let mixed_draft = VALID_DRAFT.replace(
            "axis: [gameplay]\n\nfeature",
            "axis: [state, validaton]\n\nfeature",
        );
        let fixed_draft = VALID_DRAFT.replace(
            "axis: [gameplay]\n\nfeature",
            "axis: [state, validation]\n\nfeature",
        );
        let (editor, call_count) = scripted_editor(vec![
            Box::leak(mixed_draft.into_boxed_str()),
            Box::leak(fixed_draft.into_boxed_str()),
        ]);
        let mut writer = Vec::new();

        run_edit_loop(dir.path(), &tmp_path, editor, &mut writer).unwrap();

        assert_eq!(call_count.get(), 2);
        assert!(dir.path().join("axes/state.yml").is_file());
        assert!(!dir.path().join("axes/validaton.yml").exists());
    }

    #[test]
    fn run_edit_loop_does_not_auto_create_invalid_slug_axis() {
        let dir = setup_root_with_axes(&[]);
        let tmp_path: PathBuf = dir.path().join("edit.yml");
        let invalid_draft =
            VALID_DRAFT.replace("axis: [gameplay]\n\nfeature", "axis: [UI]\n\nfeature");
        let fixed_draft =
            VALID_DRAFT.replace("axis: [gameplay]\n\nfeature", "axis: [ui]\n\nfeature");
        let (editor, call_count) = scripted_editor(vec![
            Box::leak(invalid_draft.into_boxed_str()),
            Box::leak(fixed_draft.into_boxed_str()),
        ]);
        let mut writer = Vec::new();

        run_edit_loop(dir.path(), &tmp_path, editor, &mut writer).unwrap();

        assert_eq!(call_count.get(), 2);
        // NTFS is case-insensitive, so "axes/UI.yml".exists() would spuriously
        // pass even if only "ui.yml" (from the corrected second attempt) was
        // ever written; assert on content instead to prove "UI" itself was
        // never auto-created as-is.
        assert_eq!(
            fs::read_to_string(dir.path().join("axes/ui.yml")).unwrap(),
            "id: ui\nlabel: ui\n"
        );
        let output = String::from_utf8(writer).unwrap();
        assert!(!output.contains("axis 'UI' を新規登録しました"));
    }

    #[test]
    fn propagates_io_error_from_editor_invocation_without_retry() {
        let dir = setup_root_with_axes(&["gameplay", "animation"]);
        let tmp_path: PathBuf = dir.path().join("edit.yml");
        let editor =
            |_: &Path| -> io::Result<()> { Err(io::Error::other("editor exited with error")) };
        let mut writer = Vec::new();

        let result = run_edit_loop(dir.path(), &tmp_path, editor, &mut writer);

        assert!(matches!(result, Err(EditFlowError::Io(_))));
    }

    #[test]
    fn resolve_editor_command_prefers_visual_over_editor() {
        // SAFETY: test-only, single-threaded within this process's test harness slot.
        unsafe {
            env::set_var("VISUAL", "code --wait");
            env::set_var("EDITOR", "vi");
        }

        let resolved = resolve_editor_command();

        unsafe {
            env::remove_var("VISUAL");
            env::remove_var("EDITOR");
        }
        assert_eq!(resolved, Some("code --wait".to_string()));
    }
}
