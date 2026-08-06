use clap::{Parser, Subcommand};
use std::env;
use std::io;
use std::path::PathBuf;

use crate::generate;
use crate::init;
use crate::interactive;

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
    /// Deterministically (re)generate generated/testcases.yaml from knowledge/
    Generate,
}

#[derive(Subcommand)]
pub enum KnowledgeCommand {
    /// Interactively record a Feature/Condition/ExpectedResult
    Add,
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
        Command::Knowledge(KnowledgeCommand::Add) => {
            let root = env::current_dir()?;
            let stdin = io::stdin();
            let mut reader = stdin.lock();
            let mut stdout = io::stdout();
            interactive::run_add(&root, &mut reader, &mut stdout)
        }
        Command::Generate => {
            let root = env::current_dir()?;
            let testcases = generate::generate_testcases(&root.join("knowledge"))?;
            let yaml = generate::serialize_testcases(&testcases);
            let generated_dir = root.join("generated");
            std::fs::create_dir_all(&generated_dir)?;
            std::fs::write(generated_dir.join("testcases.yaml"), yaml)?;
            println!(
                "generated {} testcase(s) into generated/testcases.yaml",
                testcases.len()
            );
            Ok(())
        }
    }
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
