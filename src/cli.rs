use clap::{Parser, Subcommand};
use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process;

use crate::axes;
use crate::backfill;
use crate::changes;
use crate::generate;
use crate::id_cache;
use crate::init;
use crate::interactive;
use crate::knowledge_apply::{self, ApplyError, ApplyOptions};
use crate::knowledge_draft::{self, ValidateOptions, ValidationError};
use crate::knowledge_edit::{self, EditFlowError};
use crate::traceability;
use crate::verify;

#[derive(Parser)]
#[command(name = "markharness")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Initialize the UC1-UC8 physical directory structure
    Init {
        /// Target directory to initialize (created if it does not exist). Defaults to the current directory.
        #[arg(long, short = 'd')]
        dir: Option<PathBuf>,
    },
    /// Manage test knowledge under knowledge/
    #[command(subcommand)]
    Knowledge(KnowledgeCommand),
    /// Deterministically (re)generate generated/testcases/*.yml from knowledge/
    Generate,
    /// Verify that generated/testcases/*.yml matches a fresh regeneration from knowledge/ (UC3 CI check)
    Verify,
    /// List axes/*.yml registry entries
    #[command(subcommand)]
    Axes(AxesCommand),
    /// Manage the id resolution cache under .markharness-cache/
    #[command(subcommand)]
    Cache(CacheCommand),
    /// Compute ChangeEvents between two milestones (UC5)
    #[command(subcommand)]
    Changes(ChangesCommand),
    /// Backfill ChangeEvents across past milestones (UC6)
    #[command(subcommand)]
    Backfill(BackfillCommand),
}

#[derive(Subcommand)]
pub enum BackfillCommand {
    /// Process one batch of unbackfilled milestone pairs, most recent first
    Run {
        /// Target project directory (a git repository). Defaults to the current directory.
        #[arg(long, short = 'd')]
        dir: Option<PathBuf>,
        /// Recompute Feature blob SHAs directly via `git ls-tree` instead of using .markharness-cache/
        #[arg(long)]
        no_cache: bool,
    },
}

#[derive(Subcommand)]
pub enum ChangesCommand {
    /// Diff Feature blob SHAs between two milestone git tags and write changes/<to>.yaml
    Compute {
        /// The earlier milestone (a git tag)
        from: String,
        /// The later milestone (a git tag)
        to: String,
        /// Target project directory (a git repository). Defaults to the current directory.
        #[arg(long, short = 'd')]
        dir: Option<PathBuf>,
        /// Recompute Feature blob SHAs directly via `git ls-tree` instead of using .markharness-cache/
        #[arg(long)]
        no_cache: bool,
    },
}

#[derive(Subcommand)]
pub enum CacheCommand {
    /// Discard .markharness-cache/ (next `changes compute` recomputes lazily)
    Rebuild {
        /// Target project directory containing .markharness-cache/. Defaults to the current directory.
        #[arg(long, short = 'd')]
        dir: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
pub enum AxesCommand {
    /// List all registered axes
    List {
        /// Target project directory containing axes/. Defaults to the current directory.
        #[arg(long, short = 'd')]
        dir: Option<PathBuf>,
        /// Emit machine-readable JSON instead of human-readable text
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum KnowledgeCommand {
    /// Interactively record a Feature/Condition/ExpectedResult
    Add {
        /// Target project directory containing knowledge/. Defaults to the current directory.
        #[arg(long, short = 'd')]
        dir: Option<PathBuf>,
        /// Open a blank draft chain in $VISUAL/$EDITOR instead of prompting on stdin
        #[arg(long)]
        edit: bool,
    },
    /// Validate a draft YAML file without writing anything
    Validate {
        /// Path to the draft YAML file
        draft_file: PathBuf,
        /// Target project directory containing knowledge/. Defaults to the current directory.
        #[arg(long, short = 'd')]
        dir: Option<PathBuf>,
        /// Emit machine-readable JSON instead of human-readable text
        #[arg(long)]
        json: bool,
    },
    /// Validate a draft YAML file and, if valid, write it under knowledge/
    Apply {
        /// Path to the draft YAML file
        draft_file: PathBuf,
        /// Target project directory containing knowledge/. Defaults to the current directory.
        #[arg(long, short = 'd')]
        dir: Option<PathBuf>,
        /// Emit machine-readable JSON instead of human-readable text
        #[arg(long)]
        json: bool,
        /// Strip a condition.id prefix that redundantly repeats behavior.id, instead of erroring
        #[arg(long)]
        strip_redundant_prefix: bool,
        /// Validate only, without writing (alias for `knowledge validate`)
        #[arg(long)]
        dry_run: bool,
    },
}

pub fn run(cli: Cli) -> io::Result<()> {
    match cli.command {
        Command::Init { dir } => {
            let root = match dir {
                Some(dir) => dir,
                None => env::current_dir()?,
            };
            init::run_init(&root)?;
            println!(
                "initialized knowledge/, axes/, generated/, executions/, changes/, schema/, tools/ under {}",
                root.display()
            );
            Ok(())
        }
        Command::Knowledge(KnowledgeCommand::Add { dir, edit }) => {
            let root = match dir {
                Some(dir) => dir,
                None => env::current_dir()?,
            };
            if edit {
                run_knowledge_add_edit(&root)
            } else {
                let stdin = io::stdin();
                let mut reader = stdin.lock();
                let mut stdout = io::stdout();
                interactive::run_add(&root, &mut reader, &mut stdout)
            }
        }
        Command::Knowledge(KnowledgeCommand::Validate {
            draft_file,
            dir,
            json,
        }) => {
            let root = match dir {
                Some(dir) => dir,
                None => env::current_dir()?,
            };
            let draft = read_and_parse_draft(&draft_file);
            let options = ValidateOptions {
                strip_redundant_prefix: false,
            };
            let errors = knowledge_draft::validate_draft(&root, &draft, &options);
            report_validation_outcome(&errors, json);
            Ok(())
        }
        Command::Knowledge(KnowledgeCommand::Apply {
            draft_file,
            dir,
            json,
            strip_redundant_prefix,
            dry_run,
        }) => {
            let root = match dir {
                Some(dir) => dir,
                None => env::current_dir()?,
            };
            let draft = read_and_parse_draft(&draft_file);

            if dry_run {
                let options = ValidateOptions {
                    strip_redundant_prefix,
                };
                let errors = knowledge_draft::validate_draft(&root, &draft, &options);
                report_validation_outcome(&errors, json);
                return Ok(());
            }

            let options = ApplyOptions {
                strip_redundant_prefix,
            };
            match knowledge_apply::apply_draft(&root, &draft, &options) {
                Ok(result) => {
                    if json {
                        println!("{}", apply_result_to_json(&result));
                    }
                    Ok(())
                }
                Err(ApplyError::Validation(errors)) => {
                    report_validation_outcome(&errors, json);
                    unreachable!("report_validation_outcome exits the process on error");
                }
                Err(ApplyError::Io(e)) => {
                    eprintln!("error: filesystem error: {e}");
                    std::process::exit(3);
                }
            }
        }
        Command::Generate => {
            let root = env::current_dir()?;
            let testcases = generate::generate_testcases(&root.join("knowledge"))?;
            let testcases_dir = root.join("generated").join("testcases");
            if testcases_dir.is_dir() {
                std::fs::remove_dir_all(&testcases_dir)?;
            }
            std::fs::create_dir_all(&testcases_dir)?;
            for testcase in &testcases {
                let file_name = format!("{}.yml", testcase.file_stem());
                std::fs::write(
                    testcases_dir.join(file_name),
                    generate::serialize_testcase(testcase),
                )?;
            }
            let index = traceability::build_index(&testcases);
            std::fs::write(
                root.join("generated").join("traceability-index.json"),
                traceability::serialize_index(&index),
            )?;
            println!(
                "generated {} testcase(s) into generated/testcases/",
                testcases.len()
            );
            Ok(())
        }
        Command::Axes(AxesCommand::List { dir, json }) => {
            let root = match dir {
                Some(dir) => dir,
                None => env::current_dir()?,
            };
            let entries = axes::list_axes(&root);
            if json {
                println!("{}", axes_to_json(&entries));
            } else if entries.is_empty() {
                println!("no axes registered under axes/");
            } else {
                for entry in &entries {
                    match &entry.label {
                        Some(label) if label != &entry.id => {
                            println!("{} ({})", entry.id, label)
                        }
                        _ => println!("{}", entry.id),
                    }
                }
            }
            Ok(())
        }
        Command::Cache(CacheCommand::Rebuild { dir }) => {
            let root = match dir {
                Some(dir) => dir,
                None => env::current_dir()?,
            };
            id_cache::rebuild_cache(&root)?;
            println!("removed .markharness-cache/ under {}", root.display());
            Ok(())
        }
        Command::Changes(ChangesCommand::Compute {
            from,
            to,
            dir,
            no_cache,
        }) => {
            let root = match dir {
                Some(dir) => dir,
                None => env::current_dir()?,
            };
            let events = changes::compute_changes(&root, &from, &to, !no_cache)?;
            let changes_dir = root.join("changes");
            std::fs::create_dir_all(&changes_dir)?;
            std::fs::write(
                changes_dir.join(format!("{to}.yaml")),
                changes::serialize_changes(&events),
            )?;
            println!(
                "computed {} change event(s) into changes/{to}.yaml",
                events.len()
            );
            Ok(())
        }
        Command::Backfill(BackfillCommand::Run { dir, no_cache }) => {
            let root = match dir {
                Some(dir) => dir,
                None => env::current_dir()?,
            };
            let report = backfill::backfill_run(&root, !no_cache)?;
            for to_milestone in &report.processed {
                println!("backfilled changes/{to_milestone}.yaml");
            }
            println!(
                "backfill: {} processed, {} already up to date",
                report.processed.len(),
                report.skipped.len()
            );
            Ok(())
        }
        Command::Verify => {
            let root = env::current_dir()?;
            let diffs = verify::diff_generated_testcases(&root)?;
            if diffs.is_empty() {
                println!("generated/testcases/ is up to date with knowledge/");
                Ok(())
            } else {
                for diff in &diffs {
                    let label = match diff.kind {
                        verify::DiffKind::Added => "added",
                        verify::DiffKind::Removed => "removed",
                        verify::DiffKind::Changed => "changed",
                    };
                    println!("{label}: generated/testcases/{}", diff.file_name);
                }
                std::process::exit(1);
            }
        }
    }
}

fn run_knowledge_add_edit(root: &std::path::Path) -> io::Result<()> {
    let Some(editor) = knowledge_edit::resolve_editor_command() else {
        eprintln!(
            "error: $VISUAL または $EDITOR が設定されていません。knowledge add --edit を使うにはどちらかを設定してください。"
        );
        std::process::exit(2);
    };
    let tmp_path = env::temp_dir().join(format!("markharness-knowledge-add-{}.yml", process::id()));
    let mut stdout = io::stdout();

    let invoke_editor = |path: &std::path::Path| -> io::Result<()> {
        let mut parts = editor.split_whitespace();
        let program = parts
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty editor command"))?;
        let status = process::Command::new(program)
            .args(parts)
            .arg(path)
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "editor exited with status {status}"
            )))
        }
    };

    let result = knowledge_edit::run_edit_loop(root, &tmp_path, invoke_editor, &mut stdout);
    let _ = fs::remove_file(&tmp_path);

    match result {
        Ok(apply_result) => {
            for path in &apply_result.written_paths {
                println!("wrote {}", path.display());
            }
            Ok(())
        }
        Err(EditFlowError::Io(e)) => {
            eprintln!("error: {e}");
            std::process::exit(3);
        }
    }
}

fn read_and_parse_draft(draft_file: &std::path::Path) -> knowledge_draft::KnowledgeDraft {
    let yaml = match fs::read_to_string(draft_file) {
        Ok(yaml) => yaml,
        Err(e) => {
            eprintln!(
                "error: cannot read draft file {}: {e}",
                draft_file.display()
            );
            std::process::exit(2);
        }
    };
    match knowledge_draft::parse_draft(&yaml) {
        Ok(draft) => draft,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    }
}

/// Prints the validation outcome and, on failure, exits the process with
/// code 1 (per §3.4 of docs/knowledge-apply-cli-spec.md). Returns normally
/// only when `errors` is empty.
fn report_validation_outcome(errors: &[ValidationError], json: bool) {
    if errors.is_empty() {
        if json {
            println!("{{\"ok\":true}}");
        }
        return;
    }
    if json {
        println!("{}", errors_to_json(errors));
    } else {
        print_errors_human(errors);
    }
    std::process::exit(1);
}

fn print_errors_human(errors: &[ValidationError]) {
    for e in errors {
        let mut detail = String::new();
        if let Some(suggestion) = &e.suggestion {
            detail.push_str(&format!("suggested=\"{suggestion}\", "));
        }
        detail.push_str(&format!("path={}", e.path));
        eprintln!("error: {}: {} ({detail})", e.code.as_str(), e.message);
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn json_string_or_null(value: &Option<String>) -> String {
    match value {
        Some(s) => format!("\"{}\"", json_escape(s)),
        None => "null".to_string(),
    }
}

fn validation_error_to_json(e: &ValidationError) -> String {
    format!(
        "{{\"code\":\"{}\",\"path\":\"{}\",\"value\":{},\"message\":\"{}\",\"suggestion\":{}}}",
        e.code.as_str(),
        json_escape(&e.path),
        json_string_or_null(&e.value),
        json_escape(&e.message),
        json_string_or_null(&e.suggestion),
    )
}

fn errors_to_json(errors: &[ValidationError]) -> String {
    let items: Vec<String> = errors.iter().map(validation_error_to_json).collect();
    format!("{{\"ok\":false,\"errors\":[{}]}}", items.join(","))
}

fn axes_to_json(entries: &[axes::AxisEntry]) -> String {
    let items: Vec<String> = entries
        .iter()
        .map(|e| {
            format!(
                "{{\"id\":\"{}\",\"label\":{}}}",
                json_escape(&e.id),
                json_string_or_null(&e.label)
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

fn apply_result_to_json(result: &knowledge_apply::ApplyResult) -> String {
    let paths: Vec<String> = result
        .written_paths
        .iter()
        .map(|p| {
            format!(
                "\"{}\"",
                json_escape(&p.to_string_lossy().replace('\\', "/"))
            )
        })
        .collect();
    format!("{{\"ok\":true,\"written\":[{}]}}", paths.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_init_dir_option() {
        let cli = Cli::parse_from(["markharness", "init", "--dir", "some/path"]);

        match cli.command {
            Command::Init { dir } => assert_eq!(dir, Some(PathBuf::from("some/path"))),
            _ => panic!("expected Init command"),
        }
    }

    #[test]
    fn parses_init_without_dir_option() {
        let cli = Cli::parse_from(["markharness", "init"]);

        match cli.command {
            Command::Init { dir } => assert_eq!(dir, None),
            _ => panic!("expected Init command"),
        }
    }

    #[test]
    fn parses_knowledge_add_dir_option() {
        let cli = Cli::parse_from([
            "markharness",
            "knowledge",
            "add",
            "--dir",
            "tmp/todo-sample",
        ]);

        match cli.command {
            Command::Knowledge(KnowledgeCommand::Add { dir, edit }) => {
                assert_eq!(dir, Some(PathBuf::from("tmp/todo-sample")));
                assert!(!edit);
            }
            _ => panic!("expected Knowledge Add command"),
        }
    }

    #[test]
    fn parses_knowledge_add_without_dir_option() {
        let cli = Cli::parse_from(["markharness", "knowledge", "add"]);

        match cli.command {
            Command::Knowledge(KnowledgeCommand::Add { dir, edit }) => {
                assert_eq!(dir, None);
                assert!(!edit);
            }
            _ => panic!("expected Knowledge Add command"),
        }
    }

    #[test]
    fn parses_knowledge_add_with_edit_flag() {
        let cli = Cli::parse_from(["markharness", "knowledge", "add", "--edit"]);

        match cli.command {
            Command::Knowledge(KnowledgeCommand::Add { edit, .. }) => assert!(edit),
            _ => panic!("expected Knowledge Add command"),
        }
    }

    #[test]
    fn parses_knowledge_validate_with_all_options() {
        let cli = Cli::parse_from([
            "markharness",
            "knowledge",
            "validate",
            "draft.yml",
            "--dir",
            "tmp/todo-sample",
            "--json",
        ]);

        match cli.command {
            Command::Knowledge(KnowledgeCommand::Validate {
                draft_file,
                dir,
                json,
            }) => {
                assert_eq!(draft_file, PathBuf::from("draft.yml"));
                assert_eq!(dir, Some(PathBuf::from("tmp/todo-sample")));
                assert!(json);
            }
            _ => panic!("expected Knowledge Validate command"),
        }
    }

    #[test]
    fn parses_knowledge_validate_with_only_required_arg() {
        let cli = Cli::parse_from(["markharness", "knowledge", "validate", "draft.yml"]);

        match cli.command {
            Command::Knowledge(KnowledgeCommand::Validate {
                draft_file,
                dir,
                json,
            }) => {
                assert_eq!(draft_file, PathBuf::from("draft.yml"));
                assert_eq!(dir, None);
                assert!(!json);
            }
            _ => panic!("expected Knowledge Validate command"),
        }
    }

    #[test]
    fn parses_knowledge_apply_with_all_options() {
        let cli = Cli::parse_from([
            "markharness",
            "knowledge",
            "apply",
            "draft.yml",
            "--dir",
            "tmp/todo-sample",
            "--json",
            "--strip-redundant-prefix",
            "--dry-run",
        ]);

        match cli.command {
            Command::Knowledge(KnowledgeCommand::Apply {
                draft_file,
                dir,
                json,
                strip_redundant_prefix,
                dry_run,
            }) => {
                assert_eq!(draft_file, PathBuf::from("draft.yml"));
                assert_eq!(dir, Some(PathBuf::from("tmp/todo-sample")));
                assert!(json);
                assert!(strip_redundant_prefix);
                assert!(dry_run);
            }
            _ => panic!("expected Knowledge Apply command"),
        }
    }

    #[test]
    fn parses_knowledge_apply_with_only_required_arg() {
        let cli = Cli::parse_from(["markharness", "knowledge", "apply", "draft.yml"]);

        match cli.command {
            Command::Knowledge(KnowledgeCommand::Apply {
                draft_file,
                dir,
                json,
                strip_redundant_prefix,
                dry_run,
            }) => {
                assert_eq!(draft_file, PathBuf::from("draft.yml"));
                assert_eq!(dir, None);
                assert!(!json);
                assert!(!strip_redundant_prefix);
                assert!(!dry_run);
            }
            _ => panic!("expected Knowledge Apply command"),
        }
    }

    #[test]
    fn parses_axes_list_with_all_options() {
        let cli = Cli::parse_from(["markharness", "axes", "list", "--dir", "sample", "--json"]);

        match cli.command {
            Command::Axes(AxesCommand::List { dir, json }) => {
                assert_eq!(dir, Some(PathBuf::from("sample")));
                assert!(json);
            }
            _ => panic!("expected Axes List command"),
        }
    }

    #[test]
    fn axes_list_prints_no_axes_message_when_registry_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        let cli = Cli::parse_from([
            "markharness",
            "axes",
            "list",
            "--dir",
            dir.path().to_str().unwrap(),
        ]);

        run(cli).unwrap();
    }

    #[test]
    fn parses_cache_rebuild_with_dir_option() {
        let cli = Cli::parse_from(["markharness", "cache", "rebuild", "--dir", "sample"]);

        match cli.command {
            Command::Cache(CacheCommand::Rebuild { dir }) => {
                assert_eq!(dir, Some(PathBuf::from("sample")))
            }
            _ => panic!("expected Cache Rebuild command"),
        }
    }

    #[test]
    fn cache_rebuild_is_a_no_op_when_cache_dir_missing() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        let cli = Cli::parse_from([
            "markharness",
            "cache",
            "rebuild",
            "--dir",
            dir.path().to_str().unwrap(),
        ]);

        run(cli).unwrap();
    }

    #[test]
    fn parses_changes_compute_with_all_options() {
        let cli = Cli::parse_from([
            "markharness",
            "changes",
            "compute",
            "m1",
            "m2",
            "--dir",
            "sample",
            "--no-cache",
        ]);

        match cli.command {
            Command::Changes(ChangesCommand::Compute {
                from,
                to,
                dir,
                no_cache,
            }) => {
                assert_eq!(from, "m1");
                assert_eq!(to, "m2");
                assert_eq!(dir, Some(PathBuf::from("sample")));
                assert!(no_cache);
            }
            _ => panic!("expected Changes Compute command"),
        }
    }

    #[test]
    fn parses_backfill_run_with_all_options() {
        let cli = Cli::parse_from([
            "markharness",
            "backfill",
            "run",
            "--dir",
            "sample",
            "--no-cache",
        ]);

        match cli.command {
            Command::Backfill(BackfillCommand::Run { dir, no_cache }) => {
                assert_eq!(dir, Some(PathBuf::from("sample")));
                assert!(no_cache);
            }
            _ => panic!("expected Backfill Run command"),
        }
    }

    #[test]
    fn backfill_run_reports_nothing_when_no_milestones_exist() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        let cli = Cli::parse_from([
            "markharness",
            "backfill",
            "run",
            "--dir",
            dir.path().to_str().unwrap(),
        ]);

        run(cli).unwrap();
    }

    #[test]
    fn run_init_with_dir_creates_missing_target_and_subdirs() {
        let base = tempfile::tempdir().unwrap();
        let target = base.path().join("nested").join("project");
        let cli = Cli::parse_from(["markharness", "init", "--dir", target.to_str().unwrap()]);

        run(cli).unwrap();

        assert!(target.join("knowledge").is_dir());
        assert!(target.join("axes").is_dir());
        assert!(target.join("generated").is_dir());
        assert!(target.join("executions").is_dir());
        assert!(target.join("changes").is_dir());
        assert!(target.join("schema").is_dir());
        assert!(target.join("tools").is_dir());
    }
}
