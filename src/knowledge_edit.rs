use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

use crate::knowledge_apply::{self, ApplyError, ApplyOptions, ApplyResult};
use crate::knowledge_draft::{self, ValidationError};

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

fn print_parse_error<W: Write>(writer: &mut W, message: impl std::fmt::Display) -> io::Result<()> {
    writeln!(writer, "error: {message}")?;
    writeln!(
        writer,
        "編集内容を確認し、保存してエディタを終了してください。"
    )
}

fn print_validation_errors<W: Write>(writer: &mut W, errors: &[ValidationError]) -> io::Result<()> {
    for e in errors {
        writeln!(
            writer,
            "error: {}: {} (path={})",
            e.code.as_str(),
            e.message,
            e.path
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
