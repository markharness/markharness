use clap::{Parser, Subcommand};
use std::env;
use std::io;

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
    /// Initialize the knowledge/generated/changes directory structure
    Init {
        #[arg(long)]
        force: bool,
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
        Command::Init { force } => {
            let root = env::current_dir()?;
            init::run_init(&root, force)?;
            println!(
                "initialized knowledge/, generated/, changes/ under {}",
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
