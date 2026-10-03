//! Command-line parsing, projections, and exit codes.

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{CommandFactory, Parser, Subcommand};
use serde_json::json;

use crate::config::read_workspace_config;

#[derive(Debug, Parser)]
#[command(
    name = "artifactize",
    version,
    about = "Let verification define the project."
)]
pub struct Cli {
    /// Repository input path.
    #[arg(long, global = true, value_name = "PATH")]
    pub repo: Option<PathBuf>,

    /// Run receipts and history directory; global services stay in the state home.
    #[arg(long, global = true, value_name = "PATH")]
    pub state_dir: Option<PathBuf>,

    /// Use JSON for command results.
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Inspect static folder declarations without executing hooks or reviews.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Validate declarations and unique Artifact/Critic identities.
    Check,
}

fn execute(cli: Cli) -> Result<(), String> {
    let mut stdout = io::stdout().lock();
    match cli.command {
        None => Cli::command()
            .write_help(&mut stdout)
            .map_err(|error| error.to_string()),
        Some(Command::Config {
            command: ConfigCommand::Check,
        }) => {
            let repo = cli.repo.unwrap_or_else(|| PathBuf::from("."));
            let config = read_workspace_config(&repo).map_err(|error| error.to_string())?;
            if cli.json {
                writeln!(stdout, "{}", json!({ "ok": true, "artifacts": config.artifacts.len(), "critics": config.critics.len() }))
            } else {
                writeln!(stdout, "Folder configuration is valid.")
            }.map_err(|error| error.to_string())
        }
    }
}

fn failure(message: &str, json: bool) -> ExitCode {
    if json {
        let _ = writeln!(io::stdout().lock(), "{}", json!({ "error": message }));
    } else {
        let _ = writeln!(io::stderr().lock(), "{message}");
    }
    ExitCode::from(2)
}

pub fn run() -> ExitCode {
    let args: Vec<_> = std::env::args_os().collect();
    let json = args.iter().any(|arg| arg == "--json");
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) if error.use_stderr() => return failure(&error.to_string(), json),
        Err(error) => {
            return if error.print().is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(2)
            };
        }
    };
    match execute(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => failure(&error, json),
    }
}
