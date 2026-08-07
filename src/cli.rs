use clap::{Parser, Subcommand};
use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;

use crate::generate;
use crate::init;
use crate::interactive;
use crate::knowledge_apply::{self, ApplyError, ApplyOptions};
use crate::knowledge_draft::{self, ValidateOptions, ValidationError};
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
}

#[derive(Subcommand)]
pub enum KnowledgeCommand {
    /// Interactively record a Feature/Condition/ExpectedResult
    Add {
        /// Target project directory containing knowledge/. Defaults to the current directory.
        #[arg(long, short = 'd')]
        dir: Option<PathBuf>,
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
        Command::Knowledge(KnowledgeCommand::Add { dir }) => {
            let root = match dir {
                Some(dir) => dir,
                None => env::current_dir()?,
            };
            let stdin = io::stdin();
            let mut reader = stdin.lock();
            let mut stdout = io::stdout();
            interactive::run_add(&root, &mut reader, &mut stdout)
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
            println!(
                "generated {} testcase(s) into generated/testcases/",
                testcases.len()
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
            Command::Knowledge(KnowledgeCommand::Add { dir }) => {
                assert_eq!(dir, Some(PathBuf::from("tmp/todo-sample")))
            }
            _ => panic!("expected Knowledge Add command"),
        }
    }

    #[test]
    fn parses_knowledge_add_without_dir_option() {
        let cli = Cli::parse_from(["markharness", "knowledge", "add"]);

        match cli.command {
            Command::Knowledge(KnowledgeCommand::Add { dir }) => assert_eq!(dir, None),
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
