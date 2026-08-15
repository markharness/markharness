use clap::{Parser, Subcommand};
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process;

use crate::axes;
use crate::backfill;
use crate::changes;
use crate::execution::{self, ExecutionResult, RecordArgs, RecordError};
use crate::fs_safety::{remove_dir_all_no_follow, replace_file};
use crate::generate;
use crate::id_cache;
use crate::init;
use crate::interactive;
use crate::knowledge_apply::{self, ApplyError, ApplyOptions, DraftFileError, DraftValidation};
use crate::knowledge_draft::{self, ValidateOptions, ValidationError};
use crate::knowledge_edit::{self, EditFlowError};
use crate::lineage;
use crate::milestone::{self, MilestoneInitError, MilestoneInitOutcome};
use crate::traceability;
use crate::validate;
use crate::verify;

#[derive(Parser)]
#[command(name = "markharness", version)]
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
    Generate {
        /// Target project directory. Defaults to the current directory.
        #[arg(long, short = 'd')]
        dir: Option<PathBuf>,
        /// Emit machine-readable JSON instead of human-readable text
        #[arg(long)]
        json: bool,
    },
    /// Verify generated output against knowledge/, or trace/audit execution results against ChangeEvents
    Verify(VerifyArgs),
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
    /// Manage executions/<tag>/milestone.yml (UC4 support)
    #[command(subcommand)]
    Milestone(MilestoneCommand),
    /// Record test execution results under executions/<milestone>/results.yml
    #[command(subcommand)]
    Execution(ExecutionCommand),
    /// Validate knowledge/ and axes/ against schema/*.schema.json plus axis/forked_from cross-references (§3.5/§3.6)
    Validate {
        /// Target project directory. Defaults to the current directory.
        #[arg(long, short = 'd')]
        dir: Option<PathBuf>,
        /// Emit machine-readable JSON instead of human-readable text
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum MilestoneCommand {
    /// Create executions/<tag>/milestone.yml for an existing git tag
    Init {
        /// The milestone name, matching an existing `git tag`
        tag: String,
        /// Target project directory (a git repository). Defaults to the current directory.
        #[arg(long, short = 'd')]
        dir: Option<PathBuf>,
        /// Emit machine-readable JSON instead of human-readable text
        #[arg(long)]
        json: bool,
    },
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultArg {
    Pass,
    Fail,
    Skip,
}

impl From<ResultArg> for ExecutionResult {
    fn from(value: ResultArg) -> Self {
        match value {
            ResultArg::Pass => ExecutionResult::Pass,
            ResultArg::Fail => ExecutionResult::Fail,
            ResultArg::Skip => ExecutionResult::Skip,
        }
    }
}

#[derive(Subcommand)]
pub enum ExecutionCommand {
    /// Append one TestCase execution result to executions/<milestone>/results.yml
    Record {
        /// The TestCase's case_id (as generated into generated/testcases/*.yml)
        case_id: String,
        /// The milestone this result belongs to, matching an existing executions/<name>/milestone.yml
        #[arg(long)]
        milestone: String,
        /// The outcome of this execution
        #[arg(long, value_enum)]
        result: ResultArg,
        /// Free-text identifier of who or what ran this (a person's name, or e.g. "ci-github-actions")
        #[arg(long)]
        executor: String,
        /// Optional free-text note
        #[arg(long)]
        note: Option<String>,
        /// Target project directory. Defaults to the current directory.
        #[arg(long, short = 'd')]
        dir: Option<PathBuf>,
        /// Emit machine-readable JSON instead of human-readable text
        #[arg(long)]
        json: bool,
    },
}

#[derive(clap::Args)]
pub struct VerifyArgs {
    /// Target project directory. Defaults to the current directory. Only used when no subcommand is given (bare `verify`'s diff mode); `trace`/`pending` carry their own --dir.
    #[arg(long, short = 'd')]
    pub dir: Option<PathBuf>,
    /// Emit machine-readable JSON instead of human-readable text. Only used when no subcommand is given.
    #[arg(long)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Option<VerifySubcommand>,
}

#[derive(Subcommand)]
pub enum VerifySubcommand {
    /// Q1: which ChangeEvent a TestExecution's verified_feature_tree_shas reflects
    Trace {
        /// The TestCase's case_id
        case_id: String,
        /// The milestone the execution result was recorded under
        #[arg(long)]
        milestone: String,
        /// Target project directory. Defaults to the current directory.
        #[arg(long, short = 'd')]
        dir: Option<PathBuf>,
        /// Emit machine-readable JSON instead of human-readable text
        #[arg(long)]
        json: bool,
    },
    /// Q2: impacted TestCases not yet re-executed against the new blob (pending/stale)
    Pending {
        /// The earlier milestone (defaults to the most recent adjacent pair with --to)
        #[arg(long, requires = "to")]
        from: Option<String>,
        /// The later milestone (defaults to the most recent adjacent pair with --from)
        #[arg(long, requires = "from")]
        to: Option<String>,
        /// Target project directory (a git repository). Defaults to the current directory.
        #[arg(long, short = 'd')]
        dir: Option<PathBuf>,
        /// Emit machine-readable JSON instead of human-readable text
        #[arg(long)]
        json: bool,
        /// Exit with a non-zero status if any TestCase is pending
        #[arg(long)]
        fail_on_pending: bool,
        /// Recompute Feature blob SHAs directly via `git ls-tree` instead of using .markharness-cache/
        #[arg(long)]
        no_cache: bool,
    },
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

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeTypeArg {
    SpecChange,
    BugFix,
    Refactor,
    Other,
}

impl From<ChangeTypeArg> for changes::ChangeType {
    fn from(value: ChangeTypeArg) -> Self {
        match value {
            ChangeTypeArg::SpecChange => changes::ChangeType::SpecChange,
            ChangeTypeArg::BugFix => changes::ChangeType::BugFix,
            ChangeTypeArg::Refactor => changes::ChangeType::Refactor,
            ChangeTypeArg::Other => changes::ChangeType::Other,
        }
    }
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
        /// Derive impacted_testcases from the current knowledge/ working tree instead of the `to` milestone's committed tree (legacy behavior; recomputing the same past interval later can then yield a different result)
        #[arg(long)]
        current_tree: bool,
    },
    /// Set change_type and/or related_events on an existing ChangeEvent under changes/ (§3.5, filled in by a human after compute)
    Annotate {
        /// The ChangeEvent's event_id (as written by `changes compute`)
        event_id: String,
        /// The kind of change this event represents. Required unless --related is given (the two fields are independently additive)
        #[arg(long, value_enum, required_unless_present = "related")]
        r#type: Option<ChangeTypeArg>,
        /// event_id(s) of other ChangeEvents to record as related (§3.5, repeatable)
        #[arg(long)]
        related: Vec<String>,
        /// Target project directory (a git repository). Defaults to the current directory.
        #[arg(long, short = 'd')]
        dir: Option<PathBuf>,
    },
    /// Audit-only: reconstruct per-Feature lineage across a merge commit's two parents via git merge-base (§3.2, secondary; does not write changes/*.yaml)
    Lineage {
        /// The merge commit to inspect (must have exactly two parents)
        #[arg(long)]
        commit: String,
        /// Target project directory (a git repository). Defaults to the current directory.
        #[arg(long, short = 'd')]
        dir: Option<PathBuf>,
        /// Emit machine-readable JSON instead of human-readable text
        #[arg(long)]
        json: bool,
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
    /// Register a new axis under axes/ (errors if it already exists)
    Add {
        /// The axis id (a slug: lowercase alphanumeric and hyphen only)
        id: String,
        /// Display label. Defaults to the id when omitted.
        #[arg(long)]
        label: Option<String>,
        /// Target project directory containing axes/. Defaults to the current directory.
        #[arg(long, short = 'd')]
        dir: Option<PathBuf>,
        /// Emit machine-readable JSON instead of human-readable text
        #[arg(long)]
        json: bool,
    },
    /// Report axes/*.yml entries not referenced by any requirement/feature/behavior's axis: list
    Prune {
        /// Also delete each unused axis's axes/<id>.yml (default is report-only)
        #[arg(long)]
        delete: bool,
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
    /// Print a blank draft YAML chain (the same template `knowledge add --edit` opens) for non-interactive callers
    Scaffold {
        /// Write to this path instead of stdout. Refuses to overwrite an existing file.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Validate a draft YAML file without writing anything
    Validate {
        /// Path to the draft YAML file. Required unless --batch is given.
        #[arg(required_unless_present = "batch")]
        draft_file: Option<PathBuf>,
        /// Directory whose direct *.yml children are all treated as draft files and validated in file-name order, cumulatively (a later draft may reuse a Requirement/Feature/Behavior an earlier, valid draft in the same batch would create), without writing anything. Every file is checked and reported, even after an earlier one fails.
        #[arg(long, conflicts_with = "draft_file")]
        batch: Option<PathBuf>,
        /// Target project directory containing knowledge/. Defaults to the current directory.
        #[arg(long, short = 'd')]
        dir: Option<PathBuf>,
        /// Emit machine-readable JSON instead of human-readable text
        #[arg(long)]
        json: bool,
    },
    /// Validate a draft YAML file and, if valid, write it under knowledge/
    Apply {
        /// Path to the draft YAML file. Required unless --batch is given.
        #[arg(required_unless_present = "batch")]
        draft_file: Option<PathBuf>,
        /// Directory whose direct *.yml children are all treated as draft files and applied in file-name order. If any fails to parse or validate, every file this call wrote (including by earlier, successful drafts in the same batch) is removed before it returns.
        #[arg(long, conflicts_with = "draft_file")]
        batch: Option<PathBuf>,
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

/// Shared error handling for `changes annotate`'s two writers
/// (`annotate_change_type` / `annotate_related_events`) and its
/// pre-validation (`validate_annotate_ids`): a missing `event_id` (target
/// or `--related`) exits 3 with a message naming the offending id, an I/O
/// error propagates to `main`'s top-level error handling.
fn handle_annotate_error(e: changes::AnnotateError) -> io::Result<()> {
    match e {
        changes::AnnotateError::NotFound(id) => {
            eprintln!("error: no ChangeEvent with event_id '{id}' found under changes/");
            process::exit(3);
        }
        changes::AnnotateError::Io(e) => Err(e),
    }
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
                "initialized knowledge/, axes/, generated/, executions/, changes/, schema/ under {}",
                root.display()
            );
            Ok(())
        }
        Command::Knowledge(KnowledgeCommand::Scaffold { out }) => match out {
            Some(out) => match knowledge_edit::write_scaffold(&out) {
                Ok(()) => Ok(()),
                Err(e) => {
                    eprintln!("error: cannot write {}: {e}", out.display());
                    std::process::exit(2);
                }
            },
            None => {
                print!("{}", knowledge_edit::EDIT_TEMPLATE);
                Ok(())
            }
        },
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
            batch,
            dir,
            json,
        }) => {
            let root = match dir {
                Some(dir) => dir,
                None => env::current_dir()?,
            };

            if let Some(batch_dir) = batch {
                return run_knowledge_validate_batch(&root, &batch_dir, json);
            }
            let draft_file =
                draft_file.expect("clap requires draft_file when --batch is not given");
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
            batch,
            dir,
            json,
            strip_redundant_prefix,
            dry_run,
        }) => {
            let root = match dir {
                Some(dir) => dir,
                None => env::current_dir()?,
            };

            if let Some(batch_dir) = batch {
                return run_knowledge_apply_batch(
                    &root,
                    &batch_dir,
                    json,
                    strip_redundant_prefix,
                    dry_run,
                );
            }
            let draft_file =
                draft_file.expect("clap requires draft_file when --batch is not given");
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
                        println!("{}", written_paths_to_json(&result.written_paths));
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
        Command::Generate { dir, json } => {
            let root = match dir {
                Some(dir) => dir,
                None => env::current_dir()?,
            };
            let testcases = generate::generate_testcases(&root.join("knowledge"))?;
            let testcases_dir = root.join("generated").join("testcases");
            remove_dir_all_no_follow(&root, &testcases_dir)?;
            std::fs::create_dir_all(&testcases_dir)?;
            let mut written = Vec::new();
            for testcase in &testcases {
                let testcase_path = safe_testcase_path(&testcases_dir, &testcase.relative_path())?;
                replace_file(
                    &root,
                    &testcase_path,
                    generate::serialize_testcase(testcase).as_bytes(),
                )?;
                written.push(testcase_path);
            }
            let index = traceability::build_index(&testcases);
            let index_path = root.join("generated").join("traceability-index.json");
            replace_file(
                &root,
                &index_path,
                traceability::serialize_index(&index).as_bytes(),
            )?;
            written.push(index_path);
            if json {
                println!("{}", generate_result_to_json(testcases.len(), &written));
            } else {
                println!(
                    "generated {} testcase(s) into generated/testcases/",
                    testcases.len()
                );
            }
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
        Command::Axes(AxesCommand::Add {
            id,
            label,
            dir,
            json,
        }) => {
            let root = match dir {
                Some(dir) => dir,
                None => env::current_dir()?,
            };
            match axes::add_axis(&root, &id, label.as_deref()) {
                Ok(path) => {
                    if json {
                        println!("{}", axes_add_result_to_json(&path));
                    } else {
                        println!("created {}", path.display());
                    }
                    Ok(())
                }
                Err(axes::AddAxisError::InvalidId) => {
                    eprintln!(
                        "error: axis id \"{id}\" is not a valid slug (lowercase alphanumeric and hyphen only)"
                    );
                    std::process::exit(2);
                }
                Err(axes::AddAxisError::AlreadyExists) => {
                    eprintln!("error: axis '{id}' already exists under axes/");
                    std::process::exit(2);
                }
                Err(axes::AddAxisError::Io(e)) => {
                    eprintln!("error: filesystem error: {e}");
                    std::process::exit(3);
                }
            }
        }
        Command::Axes(AxesCommand::Prune { delete, dir, json }) => {
            let root = match dir {
                Some(dir) => dir,
                None => env::current_dir()?,
            };
            let unused = axes::prune(&root, delete)?;
            if json {
                println!("{}", axes_prune_result_to_json(&unused, delete));
            } else if unused.is_empty() {
                println!("no unused axes found under axes/");
            } else {
                for id in &unused {
                    if delete {
                        println!("deleted axes/{id}.yml (unused)");
                    } else {
                        println!("{id} (unused)");
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
            current_tree,
        }) => {
            let root = match dir {
                Some(dir) => dir,
                None => env::current_dir()?,
            };
            let events = changes::compute_changes(&root, &from, &to, !no_cache, current_tree)?;
            let changes_dir = root.join("changes");
            replace_file(
                &root,
                &changes_dir.join(format!("{to}.yaml")),
                changes::serialize_changes(&events).as_bytes(),
            )?;
            println!(
                "computed {} change event(s) into changes/{to}.yaml",
                events.len()
            );
            Ok(())
        }
        Command::Changes(ChangesCommand::Annotate {
            event_id,
            r#type,
            related,
            dir,
        }) => {
            let root = match dir {
                Some(dir) => dir,
                None => env::current_dir()?,
            };
            // Validate up front (before any write) so a typo'd `--related`
            // id can't leave `change_type` written while `related_events`
            // isn't: the whole command either writes everything or nothing.
            if !related.is_empty()
                && let Err(e) = changes::validate_annotate_ids(&root, &event_id, &related)
            {
                return handle_annotate_error(e);
            }
            if let Some(r#type) = r#type {
                match changes::annotate_change_type(&root, &event_id, r#type.into()) {
                    Ok(()) => println!("set change_type on {event_id}"),
                    Err(e) => return handle_annotate_error(e),
                }
            }
            if related.is_empty() {
                return Ok(());
            }
            match changes::annotate_related_events(&root, &event_id, &related) {
                Ok(()) => {
                    println!("set related_events on {event_id}");
                    Ok(())
                }
                Err(e) => handle_annotate_error(e),
            }
        }
        Command::Changes(ChangesCommand::Lineage { commit, dir, json }) => {
            let root = match dir {
                Some(dir) => dir,
                None => env::current_dir()?,
            };
            match lineage::compute_lineage(&root, &commit) {
                Ok(entries) => {
                    if json {
                        println!("{}", lineage_to_json(&entries));
                    } else if entries.is_empty() {
                        println!("no Features found at {commit} or its parents");
                    } else {
                        for entry in &entries {
                            let kind = match entry.kind {
                                lineage::LineageKind::Linear => "linear",
                                lineage::LineageKind::TrueDivergence => "true_divergence",
                                lineage::LineageKind::SingleParent => "single_parent",
                            };
                            println!("{}: {kind}", entry.feature_id);
                        }
                    }
                    Ok(())
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    process::exit(2);
                }
            }
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
        Command::Milestone(MilestoneCommand::Init { tag, dir, json }) => {
            let root = match dir {
                Some(dir) => dir,
                None => env::current_dir()?,
            };
            match milestone::milestone_init(&root, &tag) {
                Ok(MilestoneInitOutcome::Created) => {
                    if json {
                        println!("{{\"ok\":true,\"status\":\"created\"}}");
                    } else {
                        println!("initialized executions/{tag}/milestone.yml");
                    }
                    Ok(())
                }
                Ok(MilestoneInitOutcome::AlreadyInitialized) => {
                    if json {
                        println!("{{\"ok\":true,\"status\":\"already_initialized\"}}");
                    } else {
                        println!("executions/{tag}/milestone.yml is already initialized");
                    }
                    Ok(())
                }
                Err(MilestoneInitError::TagNotFound) => {
                    eprintln!(
                        "error: git tag '{tag}' not found. Run `git tag {tag}` first, then retry."
                    );
                    std::process::exit(2);
                }
                Err(MilestoneInitError::Io(e)) => {
                    eprintln!("error: filesystem error: {e}");
                    std::process::exit(3);
                }
            }
        }
        Command::Execution(ExecutionCommand::Record {
            case_id,
            milestone,
            result,
            executor,
            note,
            dir,
            json,
        }) => {
            let root = match dir {
                Some(dir) => dir,
                None => env::current_dir()?,
            };
            let args = RecordArgs {
                milestone: &milestone,
                case_id: &case_id,
                result: ExecutionResult::from(result),
                executor: &executor,
                note: note.as_deref(),
            };
            match execution::record_execution(&root, &args) {
                Ok(()) => {
                    if json {
                        println!("{{\"ok\":true}}");
                    } else {
                        println!(
                            "recorded {} for {case_id} into executions/{milestone}/results.yml",
                            args.result.as_str()
                        );
                    }
                    Ok(())
                }
                Err(RecordError::MilestoneNotFound) => {
                    eprintln!(
                        "error: milestone '{milestone}' not found. Run `markharness milestone init {milestone}` first."
                    );
                    std::process::exit(2);
                }
                Err(RecordError::CaseNotFound) => {
                    eprintln!(
                        "error: case_id '{case_id}' not found in generated/testcases/. Run `markharness generate` first."
                    );
                    std::process::exit(2);
                }
                Err(RecordError::Io(e)) => {
                    eprintln!("error: filesystem error: {e}");
                    std::process::exit(3);
                }
            }
        }
        Command::Validate { dir, json } => {
            let root = match dir {
                Some(dir) => dir,
                None => env::current_dir()?,
            };
            let issues = validate::validate_all(&root)?;
            if issues.is_empty() {
                if json {
                    println!("{{\"ok\":true}}");
                } else {
                    println!("knowledge/ and axes/ are valid");
                }
                Ok(())
            } else {
                if json {
                    println!("{}", validation_issues_to_json(&issues));
                } else {
                    for issue in &issues {
                        println!("{issue}");
                    }
                }
                std::process::exit(1);
            }
        }
        Command::Verify(VerifyArgs {
            dir,
            json,
            command: None,
        }) => {
            let root = match dir {
                Some(dir) => dir,
                None => env::current_dir()?,
            };
            let diffs = verify::diff_generated_testcases(&root)?;
            if json {
                println!("{}", verify_diffs_to_json(&diffs));
            } else if diffs.is_empty() {
                println!("generated/testcases/ is up to date with knowledge/");
            } else {
                for diff in &diffs {
                    let label = match diff.kind {
                        verify::DiffKind::Added => "added",
                        verify::DiffKind::Removed => "removed",
                        verify::DiffKind::Changed => "changed",
                    };
                    println!("{label}: generated/{}", diff.file_name);
                }
            }
            if diffs.is_empty() {
                Ok(())
            } else {
                std::process::exit(1);
            }
        }
        Command::Verify(VerifyArgs {
            command:
                Some(VerifySubcommand::Trace {
                    case_id,
                    milestone,
                    dir,
                    json,
                }),
            ..
        }) => {
            let root = match dir {
                Some(dir) => dir,
                None => env::current_dir()?,
            };
            match verify::trace(&root, &case_id, &milestone) {
                Ok(result) => {
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string(&result)
                                .expect("TraceResult serialization is infallible")
                        );
                    } else {
                        println!("case_id: {}", result.case_id);
                        for entry in &result.entries {
                            println!("feature: {}", entry.feature_id);
                            println!("executed_at: {}", result.executed_at);
                            match &entry.reflects_change {
                                Some(change) => {
                                    println!("reflects_change: {}", change.event_id);
                                    println!("  from_milestone: {}", change.from_milestone);
                                    println!("  to_milestone: {}", change.to_milestone);
                                    println!("  change_type: (未記録)");
                                }
                                None => println!("reflects_change: (不明)"),
                            }
                        }
                    }
                    Ok(())
                }
                Err(verify::TraceError::NoVerifiedBlobs) => {
                    eprintln!(
                        "error: no verified_feature_tree_shas recorded for case_id '{case_id}' at milestone '{milestone}'."
                    );
                    std::process::exit(2);
                }
                Err(verify::TraceError::Io(e)) => {
                    eprintln!("error: filesystem error: {e}");
                    std::process::exit(3);
                }
            }
        }
        Command::Verify(VerifyArgs {
            command:
                Some(VerifySubcommand::Pending {
                    from,
                    to,
                    dir,
                    json,
                    fail_on_pending,
                    no_cache,
                }),
            ..
        }) => {
            let root = match dir {
                Some(dir) => dir,
                None => env::current_dir()?,
            };
            let range = match (&from, &to) {
                (Some(from), Some(to)) => Some((from.as_str(), to.as_str())),
                _ => None,
            };
            match verify::pending(&root, range, !no_cache) {
                Ok(report) => {
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string(&report)
                                .expect("PendingReport serialization is infallible")
                        );
                    } else {
                        println!("pending (再実行なし):");
                        if report.pending.is_empty() {
                            println!("  (なし)");
                        } else {
                            for entry in &report.pending {
                                println!(
                                    "  - {}  ({} の変更 {} の影響、未実行)",
                                    entry.case_id, entry.feature_id, entry.event_id
                                );
                            }
                        }
                        println!();
                        println!("stale (影響範囲がさらに変更済み):");
                        if report.stale.is_empty() {
                            println!("  (なし)");
                        } else {
                            for entry in &report.stale {
                                let current = match &entry.current_event {
                                    Some(c) => c.event_id.clone(),
                                    None => "(不明)".to_string(),
                                };
                                println!(
                                    "  - {}  ({} の変更 {} は陳腐化、現在の確認対象は {})",
                                    entry.case_id,
                                    entry.feature_id,
                                    entry.original_event_id,
                                    current
                                );
                            }
                        }
                    }
                    if fail_on_pending && !report.pending.is_empty() {
                        std::process::exit(1);
                    }
                    Ok(())
                }
                Err(verify::PendingError::NoMilestonePair) => {
                    eprintln!(
                        "error: --from/--to omitted and fewer than two milestones exist to pair."
                    );
                    std::process::exit(2);
                }
                Err(verify::PendingError::MilestoneNotFound) => {
                    eprintln!("error: --from/--to milestone not found.");
                    std::process::exit(2);
                }
                Err(verify::PendingError::InvalidRange) => {
                    eprintln!("error: --to must be strictly newer than --from.");
                    std::process::exit(2);
                }
                Err(verify::PendingError::Io(e)) => {
                    eprintln!("error: filesystem error: {e}");
                    std::process::exit(3);
                }
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
    // Created via tempfile::Builder rather than a PID-derived name so the
    // path is both unpredictable and opened with O_EXCL-equivalent
    // exclusive creation: an attacker sharing the temp directory can't
    // pre-plant a symlink or file at a name they can't guess.
    let tmp_file = tempfile::Builder::new()
        .prefix("markharness-knowledge-add-")
        .suffix(".yml")
        .tempfile()?;
    let tmp_path = tmp_file.path().to_path_buf();
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
    // tmp_file's Drop removes the file; it's outside root so it isn't a
    // managed write and fs_safety's root-scoped guards don't apply here.
    drop(tmp_file);

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

/// Lists `dir`'s direct `*.yml` children, sorted by name for a deterministic
/// application order. A later draft in a `--batch` run is validated against
/// `knowledge/`'s state *after* every earlier one in this same listing has
/// been applied (see `knowledge_apply::apply_batch`), so file naming (e.g.
/// `01-...yml`, `02-...yml`) controls which drafts can reuse a parent
/// another draft in the batch creates.
fn sorted_yaml_files_in(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("yml"))
        .collect();
    files.sort();
    Ok(files)
}

/// Prints one draft file's validation errors, prefixed with the file name so
/// they're distinguishable within a `--batch` run, then exits with code 1
/// (the same code single-draft validation failure uses, via
/// `report_validation_outcome`).
fn report_batch_validation_error(file: &Path, errors: &[ValidationError], json: bool) -> ! {
    if json {
        println!(
            "{{\"ok\":false,\"file\":\"{}\",\"errors\":{}}}",
            json_escape(&file.to_string_lossy()),
            validation_errors_to_json_array(errors)
        );
    } else {
        for e in errors {
            let mut detail = String::new();
            if let Some(suggestion) = &e.suggestion {
                detail.push_str(&format!("suggested=\"{suggestion}\", "));
            }
            detail.push_str(&format!("path={}", e.path));
            eprintln!(
                "error: {}: {}: {} ({detail})",
                file.display(),
                e.code.as_str(),
                e.message
            );
        }
    }
    std::process::exit(1);
}

/// Prints one draft file's parse error, prefixed with the file name, then
/// exits with code 2 (the same code a single-draft parse failure uses, via
/// `read_and_parse_draft`).
fn report_batch_parse_error(file: &Path, message: &str, json: bool) -> ! {
    if json {
        println!(
            "{{\"ok\":false,\"file\":\"{}\",\"error\":\"{}\"}}",
            json_escape(&file.to_string_lossy()),
            json_escape(message)
        );
    } else {
        eprintln!("error: {}: {message}", file.display());
    }
    std::process::exit(2);
}

/// Converts one failing `DraftValidation` (parse or validation error) to a
/// JSON object, mirroring `report_batch_parse_error`/`report_batch_validation_error`'s
/// single-file shapes but as an array element rather than the whole payload.
fn draft_validation_failure_to_json(result: &DraftValidation) -> String {
    let file = json_escape(&result.file.to_string_lossy());
    match result.error.as_ref().expect("caller filters to failures only") {
        DraftFileError::Parse(message) => {
            format!("{{\"file\":\"{file}\",\"error\":\"{}\"}}", json_escape(message))
        }
        DraftFileError::Validation(errors) => {
            format!(
                "{{\"file\":\"{file}\",\"errors\":{}}}",
                validation_errors_to_json_array(errors)
            )
        }
    }
}

/// Reports a `validate_batch` outcome the same way for both `knowledge
/// validate --batch` and `apply --batch --dry-run` (they share this one
/// underlying check, see `knowledge_apply::validate_batch`'s doc comment):
/// every failing file is listed, not just the first, then exits 1 if any
/// failed.
fn report_batch_validate_result(result: &knowledge_apply::BatchValidateResult, json: bool) {
    if result.ok() {
        if json {
            println!("{{\"ok\":true}}");
        }
        return;
    }

    let failures: Vec<&DraftValidation> = result
        .results
        .iter()
        .filter(|r| r.error.is_some())
        .collect();

    if json {
        let items: Vec<String> = failures
            .iter()
            .map(|r| draft_validation_failure_to_json(r))
            .collect();
        println!("{{\"ok\":false,\"failures\":[{}]}}", items.join(","));
    } else {
        for result in &failures {
            match result.error.as_ref().expect("filtered to failures only") {
                DraftFileError::Parse(message) => {
                    eprintln!("error: {}: {message}", result.file.display());
                }
                DraftFileError::Validation(errors) => {
                    for e in errors {
                        let mut detail = String::new();
                        if let Some(suggestion) = &e.suggestion {
                            detail.push_str(&format!("suggested=\"{suggestion}\", "));
                        }
                        detail.push_str(&format!("path={}", e.path));
                        eprintln!(
                            "error: {}: {}: {} ({detail})",
                            result.file.display(),
                            e.code.as_str(),
                            e.message
                        );
                    }
                }
            }
        }
    }
    std::process::exit(1);
}

fn run_knowledge_validate_batch(root: &Path, batch_dir: &Path, json: bool) -> io::Result<()> {
    let draft_paths = match sorted_yaml_files_in(batch_dir) {
        Ok(paths) => paths,
        Err(e) => {
            eprintln!(
                "error: cannot read batch directory {}: {e}",
                batch_dir.display()
            );
            std::process::exit(2);
        }
    };

    let options = ValidateOptions {
        strip_redundant_prefix: false,
    };
    let result = knowledge_apply::validate_batch(root, &draft_paths, &options)?;
    report_batch_validate_result(&result, json);
    Ok(())
}

fn run_knowledge_apply_batch(
    root: &Path,
    batch_dir: &Path,
    json: bool,
    strip_redundant_prefix: bool,
    dry_run: bool,
) -> io::Result<()> {
    let draft_paths = match sorted_yaml_files_in(batch_dir) {
        Ok(paths) => paths,
        Err(e) => {
            eprintln!(
                "error: cannot read batch directory {}: {e}",
                batch_dir.display()
            );
            std::process::exit(2);
        }
    };

    if dry_run {
        let options = ValidateOptions {
            strip_redundant_prefix,
        };
        let result = knowledge_apply::validate_batch(root, &draft_paths, &options)?;
        report_batch_validate_result(&result, json);
        return Ok(());
    }

    let options = ApplyOptions {
        strip_redundant_prefix,
    };
    match knowledge_apply::apply_batch(root, &draft_paths, &options) {
        Ok(result) => {
            if json {
                println!("{}", written_paths_to_json(&result.written_paths));
            }
            Ok(())
        }
        Err(knowledge_apply::BatchApplyError::Draft { file, error }) => match error {
            knowledge_apply::DraftFileError::Parse(message) => {
                report_batch_parse_error(&file, &message, json)
            }
            knowledge_apply::DraftFileError::Validation(errors) => {
                report_batch_validation_error(&file, &errors, json)
            }
        },
        Err(knowledge_apply::BatchApplyError::Io(e)) => {
            eprintln!("error: filesystem error: {e}");
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
/// code 1 (per §3.4 of docs/en/design/knowledge-apply-cli-spec.md). Returns normally
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

fn validation_errors_to_json_array(errors: &[ValidationError]) -> String {
    let items: Vec<String> = errors.iter().map(validation_error_to_json).collect();
    format!("[{}]", items.join(","))
}

fn errors_to_json(errors: &[ValidationError]) -> String {
    format!(
        "{{\"ok\":false,\"errors\":{}}}",
        validation_errors_to_json_array(errors)
    )
}

fn lineage_to_json(entries: &[lineage::FeatureLineage]) -> String {
    let items: Vec<String> = entries
        .iter()
        .map(|e| {
            let kind = match e.kind {
                lineage::LineageKind::Linear => "linear",
                lineage::LineageKind::TrueDivergence => "true_divergence",
                lineage::LineageKind::SingleParent => "single_parent",
            };
            format!(
                "{{\"feature_id\":\"{}\",\"base_tree_sha\":{},\"parent1_tree_sha\":{},\"parent2_tree_sha\":{},\"kind\":\"{kind}\"}}",
                json_escape(&e.feature_id),
                json_string_or_null(&e.base_tree_sha),
                json_string_or_null(&e.parent1_tree_sha),
                json_string_or_null(&e.parent2_tree_sha),
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

fn validation_issues_to_json(issues: &[validate::ValidationIssue]) -> String {
    let items: Vec<String> = issues
        .iter()
        .map(|i| {
            format!(
                "{{\"path\":\"{}\",\"message\":\"{}\"}}",
                json_escape(&i.path),
                json_escape(&i.message)
            )
        })
        .collect();
    format!("{{\"ok\":false,\"issues\":[{}]}}", items.join(","))
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

fn axes_add_result_to_json(path: &Path) -> String {
    format!(
        "{{\"ok\":true,\"written\":[\"{}\"]}}",
        json_escape(&path.to_string_lossy().replace('\\', "/"))
    )
}

/// Same key structure (`axes`/`deleted`) for both a dry-run report and an
/// actual `--delete` run, so a caller doesn't need separate parsing logic
/// for the two modes.
fn axes_prune_result_to_json(unused: &[String], deleted: bool) -> String {
    let ids: Vec<String> = unused
        .iter()
        .map(|id| format!("\"{}\"", json_escape(id)))
        .collect();
    format!("{{\"axes\":[{}],\"deleted\":{deleted}}}", ids.join(","))
}

/// `--json` output for bare `verify`'s dry-run diff (Step C): reports
/// whether `generated/testcases/` would change if `generate` were run, and
/// which files fall into each of the three buckets, so a CI caller doesn't
/// have to parse the human-readable `added:`/`changed:`/`removed:` lines.
fn verify_diffs_to_json(diffs: &[verify::DiffEntry]) -> String {
    let bucket = |kind: verify::DiffKind| -> String {
        let items: Vec<String> = diffs
            .iter()
            .filter(|d| d.kind == kind)
            .map(|d| format!("\"{}\"", json_escape(&d.file_name)))
            .collect();
        format!("[{}]", items.join(","))
    };
    format!(
        "{{\"would_change\":{},\"added\":{},\"changed\":{},\"removed\":{}}}",
        !diffs.is_empty(),
        bucket(verify::DiffKind::Added),
        bucket(verify::DiffKind::Changed),
        bucket(verify::DiffKind::Removed),
    )
}

/// Joins `testcases_dir` and a testcase's `relative_path()` (the
/// `{requirement}/{feature}/{behavior}/{condition}.yml` mirror of
/// `knowledge/`), refusing any result that would escape `testcases_dir` via
/// a `..`/root/prefix component. Defense in depth: `generate::generate_testcases`
/// already rejects non-slug requirement/feature/behavior/condition ids, but
/// this guards the write site itself against any other source of a
/// malformed relative path.
fn safe_testcase_path(testcases_dir: &Path, relative_path: &Path) -> io::Result<PathBuf> {
    for component in relative_path.components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "refusing to write testcase outside {}: {}",
                    testcases_dir.display(),
                    relative_path.display()
                ),
            ));
        }
    }
    Ok(testcases_dir.join(relative_path))
}

fn written_paths_to_json(written_paths: &[PathBuf]) -> String {
    let paths: Vec<String> = written_paths
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

fn generate_result_to_json(generated: usize, written: &[PathBuf]) -> String {
    let paths: Vec<String> = written
        .iter()
        .map(|p| {
            format!(
                "\"{}\"",
                json_escape(&p.to_string_lossy().replace('\\', "/"))
            )
        })
        .collect();
    format!(
        "{{\"ok\":true,\"generated\":{generated},\"written\":[{}]}}",
        paths.join(",")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;
    use clap::CommandFactory;

    #[test]
    fn version_flag_reports_display_version() {
        let result = Cli::command().try_get_matches_from(["markharness", "--version"]);

        let err = result.expect_err("--version should short-circuit with an error");
        assert_eq!(err.kind(), ErrorKind::DisplayVersion);
    }

    #[test]
    fn safe_testcase_path_joins_a_nested_relative_path() {
        let testcases_dir = PathBuf::from("generated/testcases");
        let relative_path = PathBuf::from("req-todo/todo/todo-add-task/ground.yml");

        let path = safe_testcase_path(&testcases_dir, &relative_path).unwrap();

        assert_eq!(path, testcases_dir.join(&relative_path));
    }

    #[test]
    fn safe_testcase_path_rejects_a_relative_path_escaping_the_testcases_dir() {
        let testcases_dir = PathBuf::from("generated/testcases");
        let relative_path = PathBuf::from("../../../../evil.yml");

        let result = safe_testcase_path(&testcases_dir, &relative_path);

        assert!(
            result.is_err(),
            "expected an error for a relative_path escaping testcases_dir, got: {result:?}"
        );
    }

    #[test]
    fn parses_generate_dir_and_json_options() {
        let cli = Cli::parse_from(["markharness", "generate", "--dir", "some/path", "--json"]);

        match cli.command {
            Command::Generate { dir, json } => {
                assert_eq!(dir, Some(PathBuf::from("some/path")));
                assert!(json);
            }
            _ => panic!("expected Generate command"),
        }
    }

    #[test]
    fn parses_generate_without_dir_or_json_options() {
        let cli = Cli::parse_from(["markharness", "generate"]);

        match cli.command {
            Command::Generate { dir, json } => {
                assert_eq!(dir, None);
                assert!(!json);
            }
            _ => panic!("expected Generate command"),
        }
    }

    #[test]
    fn parses_bare_verify_dir_and_json_options() {
        let cli = Cli::parse_from(["markharness", "verify", "--dir", "some/path", "--json"]);

        match cli.command {
            Command::Verify(VerifyArgs { dir, json, command }) => {
                assert_eq!(dir, Some(PathBuf::from("some/path")));
                assert!(json);
                assert!(command.is_none());
            }
            _ => panic!("expected Verify command"),
        }
    }

    #[test]
    fn parses_bare_verify_without_dir_or_json_options() {
        let cli = Cli::parse_from(["markharness", "verify"]);

        match cli.command {
            Command::Verify(VerifyArgs { dir, json, command }) => {
                assert_eq!(dir, None);
                assert!(!json);
                assert!(command.is_none());
            }
            _ => panic!("expected Verify command"),
        }
    }

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
                batch,
                dir,
                json,
            }) => {
                assert_eq!(draft_file, Some(PathBuf::from("draft.yml")));
                assert_eq!(batch, None);
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
                batch,
                dir,
                json,
            }) => {
                assert_eq!(draft_file, Some(PathBuf::from("draft.yml")));
                assert_eq!(batch, None);
                assert_eq!(dir, None);
                assert!(!json);
            }
            _ => panic!("expected Knowledge Validate command"),
        }
    }

    #[test]
    fn parses_knowledge_validate_with_batch_option() {
        let cli = Cli::parse_from([
            "markharness",
            "knowledge",
            "validate",
            "--batch",
            "tmp/drafts",
        ]);

        match cli.command {
            Command::Knowledge(KnowledgeCommand::Validate {
                draft_file, batch, ..
            }) => {
                assert_eq!(draft_file, None);
                assert_eq!(batch, Some(PathBuf::from("tmp/drafts")));
            }
            _ => panic!("expected Knowledge Validate command"),
        }
    }

    #[test]
    fn knowledge_validate_batch_and_draft_file_are_mutually_exclusive() {
        let result = Cli::try_parse_from([
            "markharness",
            "knowledge",
            "validate",
            "draft.yml",
            "--batch",
            "tmp/drafts",
        ]);

        assert!(result.is_err());
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
                batch,
                dir,
                json,
                strip_redundant_prefix,
                dry_run,
            }) => {
                assert_eq!(draft_file, Some(PathBuf::from("draft.yml")));
                assert_eq!(batch, None);
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
                batch,
                dir,
                json,
                strip_redundant_prefix,
                dry_run,
            }) => {
                assert_eq!(draft_file, Some(PathBuf::from("draft.yml")));
                assert_eq!(batch, None);
                assert_eq!(dir, None);
                assert!(!json);
                assert!(!strip_redundant_prefix);
                assert!(!dry_run);
            }
            _ => panic!("expected Knowledge Apply command"),
        }
    }

    #[test]
    fn parses_knowledge_apply_with_batch_option() {
        let cli = Cli::parse_from([
            "markharness",
            "knowledge",
            "apply",
            "--batch",
            "tmp/drafts",
            "--dir",
            "tmp/todo-sample",
        ]);

        match cli.command {
            Command::Knowledge(KnowledgeCommand::Apply {
                draft_file,
                batch,
                dir,
                ..
            }) => {
                assert_eq!(draft_file, None);
                assert_eq!(batch, Some(PathBuf::from("tmp/drafts")));
                assert_eq!(dir, Some(PathBuf::from("tmp/todo-sample")));
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
    fn parses_axes_add_with_all_options() {
        let cli = Cli::parse_from([
            "markharness",
            "axes",
            "add",
            "state",
            "--label",
            "State",
            "--dir",
            "sample",
            "--json",
        ]);

        match cli.command {
            Command::Axes(AxesCommand::Add {
                id,
                label,
                dir,
                json,
            }) => {
                assert_eq!(id, "state");
                assert_eq!(label, Some("State".to_string()));
                assert_eq!(dir, Some(PathBuf::from("sample")));
                assert!(json);
            }
            _ => panic!("expected Axes Add command"),
        }
    }

    #[test]
    fn parses_axes_add_with_only_required_arg() {
        let cli = Cli::parse_from(["markharness", "axes", "add", "state"]);

        match cli.command {
            Command::Axes(AxesCommand::Add {
                id,
                label,
                dir,
                json,
            }) => {
                assert_eq!(id, "state");
                assert_eq!(label, None);
                assert_eq!(dir, None);
                assert!(!json);
            }
            _ => panic!("expected Axes Add command"),
        }
    }

    #[test]
    fn axes_add_writes_the_axis_file() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        let cli = Cli::parse_from([
            "markharness",
            "axes",
            "add",
            "state",
            "--dir",
            dir.path().to_str().unwrap(),
        ]);

        run(cli).unwrap();

        assert_eq!(
            fs::read_to_string(dir.path().join("axes/state.yml")).unwrap(),
            "id: state\nlabel: state\n"
        );
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
            "--current-tree",
        ]);

        match cli.command {
            Command::Changes(ChangesCommand::Compute {
                from,
                to,
                dir,
                no_cache,
                current_tree,
            }) => {
                assert_eq!(from, "m1");
                assert_eq!(to, "m2");
                assert_eq!(dir, Some(PathBuf::from("sample")));
                assert!(no_cache);
                assert!(current_tree);
            }
            _ => panic!("expected Changes Compute command"),
        }
    }

    #[test]
    fn parses_milestone_init_with_all_options() {
        let cli = Cli::parse_from([
            "markharness",
            "milestone",
            "init",
            "m1",
            "--dir",
            "sample",
            "--json",
        ]);

        match cli.command {
            Command::Milestone(MilestoneCommand::Init { tag, dir, json }) => {
                assert_eq!(tag, "m1");
                assert_eq!(dir, Some(PathBuf::from("sample")));
                assert!(json);
            }
            _ => panic!("expected Milestone Init command"),
        }
    }

    #[test]
    fn parses_execution_record_with_all_options() {
        let cli = Cli::parse_from([
            "markharness",
            "execution",
            "record",
            "tc-ground-001",
            "--milestone",
            "m1",
            "--result",
            "pass",
            "--executor",
            "yamada",
            "--note",
            "looked fine",
            "--dir",
            "sample",
            "--json",
        ]);

        match cli.command {
            Command::Execution(ExecutionCommand::Record {
                case_id,
                milestone,
                result,
                executor,
                note,
                dir,
                json,
            }) => {
                assert_eq!(case_id, "tc-ground-001");
                assert_eq!(milestone, "m1");
                assert_eq!(result, ResultArg::Pass);
                assert_eq!(executor, "yamada");
                assert_eq!(note, Some("looked fine".to_string()));
                assert_eq!(dir, Some(PathBuf::from("sample")));
                assert!(json);
            }
            _ => panic!("expected Execution Record command"),
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

    fn run_git_for_test(root: &std::path::Path, args: &[&str]) {
        let status = process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    fn init_git_repo_for_test() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        run_git_for_test(dir.path(), &["init", "-q"]);
        run_git_for_test(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git_for_test(dir.path(), &["config", "user.name", "Test"]);
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        run_git_for_test(dir.path(), &["add", "-A"]);
        run_git_for_test(dir.path(), &["commit", "-q", "-m", "init"]);
        dir
    }

    #[test]
    fn milestone_init_writes_milestone_yml_when_tag_exists() {
        let dir = init_git_repo_for_test();
        run_git_for_test(dir.path(), &["tag", "m1"]);
        let cli = Cli::parse_from([
            "markharness",
            "milestone",
            "init",
            "m1",
            "--dir",
            dir.path().to_str().unwrap(),
        ]);

        run(cli).unwrap();

        assert!(dir.path().join("executions/m1/milestone.yml").is_file());
    }

    #[test]
    fn milestone_init_is_idempotent_on_second_run() {
        let dir = init_git_repo_for_test();
        run_git_for_test(dir.path(), &["tag", "m1"]);
        let cli = Cli::parse_from([
            "markharness",
            "milestone",
            "init",
            "m1",
            "--dir",
            dir.path().to_str().unwrap(),
        ]);
        run(cli).unwrap();

        let cli_again = Cli::parse_from([
            "markharness",
            "milestone",
            "init",
            "m1",
            "--dir",
            dir.path().to_str().unwrap(),
        ]);
        run(cli_again).unwrap();

        let content = fs::read_to_string(dir.path().join("executions/m1/milestone.yml")).unwrap();
        assert_eq!(content, "id: m1\n");
    }

    fn write_generated_testcase_for_test(
        root: &std::path::Path,
        condition_id: &str,
        case_id: &str,
        feature_id: &str,
    ) {
        let dir = root.join("generated/testcases");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(format!("{condition_id}.yml")),
            format!("case_id: {case_id}\ngenerated_from:\n  feature: {feature_id}\n"),
        )
        .unwrap();
    }

    #[test]
    fn execution_record_writes_results_yml_when_milestone_and_case_exist() {
        let dir = init_git_repo_for_test();
        fs::create_dir_all(dir.path().join("knowledge/controls/player-jump")).unwrap();
        fs::write(
            dir.path()
                .join("knowledge/controls/player-jump/feature.yml"),
            "id: player-jump\nrequirement: controls\nlabel: player-jump\naxis: []\n",
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("executions/m1")).unwrap();
        fs::write(dir.path().join("executions/m1/milestone.yml"), "id: m1\n").unwrap();
        run_git_for_test(dir.path(), &["add", "-A"]);
        run_git_for_test(dir.path(), &["commit", "-q", "-m", "add feature"]);
        run_git_for_test(dir.path(), &["tag", "m1"]);
        write_generated_testcase_for_test(dir.path(), "ground", "tc-ground-001", "player-jump");
        let cli = Cli::parse_from([
            "markharness",
            "execution",
            "record",
            "tc-ground-001",
            "--milestone",
            "m1",
            "--result",
            "pass",
            "--executor",
            "yamada",
            "--dir",
            dir.path().to_str().unwrap(),
        ]);

        run(cli).unwrap();

        let content = fs::read_to_string(dir.path().join("executions/m1/results.yml")).unwrap();
        assert!(content.contains("case_id: tc-ground-001"));
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
        assert!(!target.join("tools").exists());
    }
}
