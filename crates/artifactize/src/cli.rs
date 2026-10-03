//! Command-line parsing, projections, and exit codes.

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, CommandFactory, Parser, Subcommand};
use serde_json::json;

use crate::{
    config::read_workspace_config,
    project::selection::{ProfileSelection, Selection, read_selection_file},
};

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
    /// Execute selected runtime Critics in the foreground.
    Verify {
        #[command(flatten)]
        selection: SelectionArgs,
        /// Use a declared profile variant for every selected Critic.
        #[arg(long, value_name = "NAME")]
        profile: Option<String>,
        /// Wait for completion (currently always foreground).
        #[arg(long)]
        wait: bool,
        /// Include the full runtime audit JSON.
        #[arg(long)]
        full: bool,
    },
    /// Read recorded Runs without discovering or executing project code.
    Run {
        #[command(subcommand)]
        command: RunCommand,
    },
    /// Inspect static folder declarations without executing hooks or reviews.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

#[derive(Debug, Args)]
#[group(required = true, multiple = false)]
pub struct SelectionArgs {
    /// Select one Artifact.
    artifact: Option<String>,
    /// Select one qualified Critic ID.
    #[arg(long, value_name = "ID")]
    critic: Option<String>,
    /// Select comma-separated qualified Critic IDs.
    #[arg(long, value_name = "CSV")]
    critics: Option<String>,
    /// Select comma-separated Artifact names.
    #[arg(long, value_name = "CSV")]
    artifacts: Option<String>,
    /// Read Critic IDs from a JSON array or one ID per line.
    #[arg(long, value_name = "PATH")]
    critics_file: Option<PathBuf>,
    /// Read Artifact names from a JSON array or one ID per line.
    #[arg(long, value_name = "PATH")]
    artifacts_file: Option<PathBuf>,
    /// Select every Artifact.
    #[arg(long)]
    all: bool,
}

impl SelectionArgs {
    fn resolve(self) -> Result<Selection, String> {
        if let Some(artifact_id) = self.artifact {
            Ok(Selection::Artifact { artifact_id })
        } else if let Some(critic_id) = self.critic {
            Ok(Selection::Critic { critic_id })
        } else if let Some(ids) = self.critics {
            Ok(Selection::Critics {
                critic_ids: ids.split(',').map(str::to_owned).collect(),
            })
        } else if let Some(ids) = self.artifacts {
            Ok(Selection::Artifacts {
                artifact_ids: ids.split(',').map(str::to_owned).collect(),
            })
        } else if let Some(path) = self.critics_file {
            Ok(Selection::Critics {
                critic_ids: read_selection_file(&path)?,
            })
        } else if let Some(path) = self.artifacts_file {
            Ok(Selection::Artifacts {
                artifact_ids: read_selection_file(&path)?,
            })
        } else {
            Ok(Selection::All)
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Validate declarations and unique Artifact/Critic identities.
    Check,
}

#[derive(Debug, Subcommand)]
pub enum RunCommand {
    /// Read the full saved audit as JSON, even without --json.
    Show { run_id: String },
}

async fn execute(cli: Cli) -> Result<u8, String> {
    match cli.command {
        None => {
            Cli::command()
                .write_help(&mut io::stdout().lock())
                .map_err(|error| error.to_string())?;
            Ok(0)
        }
        Some(Command::Verify {
            selection,
            profile,
            full,
            ..
        }) => {
            let selection = selection.resolve()?;
            let profile = profile.map(ProfileSelection::Named);
            let cancellation = tokio_util::sync::CancellationToken::new();
            let mut interrupt =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
                    .map_err(|e| e.to_string())?;
            let mut terminate =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .map_err(|e| e.to_string())?;
            let token = cancellation.clone();
            let listener = tokio::spawn(async move {
                tokio::select! { _ = interrupt.recv() => {}, _ = terminate.recv() => {} }
                token.cancel();
            });
            let result = crate::project::verify(
                &cli.repo.unwrap_or_else(|| PathBuf::from(".")),
                cli.state_dir.as_deref(),
                &selection,
                profile.as_ref(),
                cancellation,
            )
            .await;
            listener.abort();
            let view = result?;
            if full {
                print_json(&view)?;
            } else if cli.json {
                print_json(&crate::query::requester_run(&view))?;
            } else {
                let mut stdout = io::stdout().lock();
                writeln!(
                    stdout,
                    "Run: {}\nExecution: {}\nState: {}",
                    view.run.id,
                    view.run.status,
                    view.run.state_dir.display()
                )
                .map_err(|e| e.to_string())?;
                for request in &view.requests {
                    writeln!(
                        stdout,
                        "  {}: {}{}",
                        request.critic_id,
                        request.status,
                        request
                            .error
                            .as_ref()
                            .or(request.blocked_reason.as_ref())
                            .map_or(String::new(), |reason| format!(" — {reason}"))
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
            Ok(match view.run.status.as_str() {
                "GREEN" => 0,
                "RED" => 1,
                "INCOMPLETE" => 4,
                _ => 2,
            })
        }
        Some(Command::Run {
            command: RunCommand::Show { run_id },
        }) => {
            let repo = cli
                .repo
                .map(|path| path.canonicalize().map_err(|e| e.to_string()))
                .transpose()?;
            let default_repo = if cli.state_dir.is_none() && repo.is_none() {
                Some(std::fs::canonicalize(".").map_err(|e| e.to_string())?)
            } else {
                None
            };
            let repo = repo.as_deref().or(default_repo.as_deref());
            let state = crate::store::receipts_dir(
                repo.unwrap_or(std::path::Path::new("")),
                cli.state_dir.as_deref(),
            )?;
            let view = crate::store::read_run(&state, repo, &run_id).await?;
            print_json(&view)?;
            Ok(0)
        }
        Some(Command::Config {
            command: ConfigCommand::Check,
        }) => {
            let repo = cli.repo.unwrap_or_else(|| PathBuf::from("."));
            let config = read_workspace_config(&repo).map_err(|error| error.to_string())?;
            let mut stdout = io::stdout().lock();
            if cli.json {
                writeln!(stdout, "{}", json!({ "ok": true, "artifacts": config.artifacts.len(), "critics": config.critics.len() }))
            } else {
                writeln!(stdout, "Folder configuration is valid.")
            }.map_err(|error| error.to_string())?;
            Ok(0)
        }
    }
}

fn print_json(value: &impl serde::Serialize) -> Result<(), String> {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer_pretty(&mut stdout, value).map_err(|e| e.to_string())?;
    writeln!(stdout).map_err(|e| e.to_string())
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
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => return failure(&error.to_string(), json),
    };
    match runtime.block_on(execute(cli)) {
        Ok(code) => ExitCode::from(code),
        Err(error) => failure(&error, json),
    }
}
