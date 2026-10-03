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

    /// State database and Run output directory.
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
    /// Execute selected runtime Evals in the foreground.
    #[command(group(clap::ArgGroup::new("required_selection")
        .args(["artifact", "eval", "evals", "artifacts", "evals_file", "artifacts_file", "all"])
        .required(true)))]
    Verify {
        #[command(flatten)]
        selection: SelectionArgs,
        /// Use a declared profile variant for every included Eval.
        #[arg(long, value_name = "NAME")]
        profile: Option<String>,
        /// Include all Evals in the required dependency scope, including cycle peers.
        #[arg(long)]
        recursive: bool,
        /// Force explicitly selected Evals only; dependency gates still apply.
        #[arg(long)]
        force: bool,
        /// Bypass execution gates, never final validation obligations.
        #[arg(long)]
        ignore_gates: bool,
        /// Wait for completion (currently always foreground).
        #[arg(long)]
        wait: bool,
    },
    /// Inspect current validation, saved Run audit, and what verify would do.
    Status {
        #[command(flatten)]
        selection: SelectionArgs,
        #[arg(long, value_name = "NAME")]
        profile: Option<String>,
        #[arg(long)]
        recursive: bool,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        ignore_gates: bool,
    },
    /// Inspect full static definitions, relations, cycles, and families.
    Graph {
        /// Select one Artifact or family and its required closure; defaults to all.
        artifact: Option<String>,
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
#[group(multiple = false)]
pub struct SelectionArgs {
    /// Select one Artifact or every instance of a family.
    artifact: Option<String>,
    /// Select one qualified Eval ID.
    #[arg(long, value_name = "ID")]
    eval: Option<String>,
    /// Select comma-separated qualified Eval IDs.
    #[arg(long, value_name = "CSV")]
    evals: Option<String>,
    /// Select comma-separated Artifact or family names.
    #[arg(long, value_name = "CSV")]
    artifacts: Option<String>,
    /// Read Eval IDs from a JSON array or one ID per line.
    #[arg(long, value_name = "PATH")]
    evals_file: Option<PathBuf>,
    /// Read Artifact or family names from a JSON array or one ID per line.
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
        } else if let Some(eval_id) = self.eval {
            Ok(Selection::Eval { eval_id })
        } else if let Some(ids) = self.evals {
            Ok(Selection::Evals {
                eval_ids: ids.split(',').map(str::to_owned).collect(),
            })
        } else if let Some(ids) = self.artifacts {
            Ok(Selection::Artifacts {
                artifact_ids: ids.split(',').map(str::to_owned).collect(),
            })
        } else if let Some(path) = self.evals_file {
            Ok(Selection::Evals {
                eval_ids: read_selection_file(&path)?,
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
    /// Validate declarations and unique Artifact/Eval identities.
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
            recursive,
            force,
            ignore_gates,
            ..
        }) => {
            let selection = selection.resolve()?;
            let options = crate::project::VerifyOptions {
                profile: profile.map(ProfileSelection::Named),
                recursive,
                force,
                ignore_gates: ignore_gates.then_some(true),
            };
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
                &options,
                cancellation,
            )
            .await;
            listener.abort();
            let view = result?;
            if cli.json {
                print_json(&view)?;
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
                        request.eval_id,
                        request.status,
                        request
                            .error
                            .as_ref()
                            .or(request.blocked_reason.as_ref())
                            .map_or(String::new(), |reason| format!(" — {reason}"))
                    )
                    .map_err(|e| e.to_string())?;
                }
                writeln!(
                    stdout,
                    "Validation: {}",
                    if view.run.validation["satisfied"] == true {
                        "SATISFIED"
                    } else {
                        "NOT SATISFIED"
                    }
                )
                .map_err(|e| e.to_string())?;
                if let Some(obligations) = view.run.validation["obligations"].as_array() {
                    for artifact in obligations.iter().filter_map(serde_json::Value::as_str) {
                        writeln!(stdout, "  Unmet obligation: {artifact}")
                            .map_err(|e| e.to_string())?;
                    }
                }
            }
            Ok(match view.run.status.as_str() {
                "GREEN" => 0,
                "RED" => 1,
                "INCOMPLETE" => 4,
                _ => 2,
            })
        }
        Some(Command::Status {
            selection,
            profile,
            recursive,
            force,
            ignore_gates,
        }) => {
            let view = crate::project::status(
                &cli.repo.unwrap_or_else(|| PathBuf::from(".")),
                cli.state_dir.as_deref(),
                &selection.resolve()?,
                &crate::project::VerifyOptions {
                    profile: profile.map(ProfileSelection::Named),
                    recursive,
                    force,
                    ignore_gates: ignore_gates.then_some(true),
                },
            )
            .await?;
            if cli.json {
                print_json(&view)?;
            } else {
                print_status(&view).map_err(|error| error.to_string())?;
            }
            Ok(u8::from(!view.satisfied))
        }
        Some(Command::Graph { artifact }) => {
            let config = read_workspace_config(&cli.repo.unwrap_or_else(|| PathBuf::from(".")))
                .map_err(|error| error.to_string())?;
            let selection = artifact.map_or(Selection::All, |artifact_id| Selection::Artifact {
                artifact_id,
            });
            let view = crate::query::graph(&config, &selection)?;
            if cli.json {
                print_json(&view)?;
            } else {
                print_graph(&view).map_err(|error| error.to_string())?;
            }
            Ok(0)
        }
        Some(Command::Run {
            command: RunCommand::Show { run_id },
        }) => {
            let state = crate::store::state_dir(cli.state_dir.as_deref())?;
            let view = crate::store::read_run(&state, &run_id).await?;
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
                writeln!(stdout, "{}", json!({ "ok": true, "artifacts": config.artifacts.len(), "evals": config.evals.len() }))
            } else {
                writeln!(stdout, "Folder configuration is valid.")
            }.map_err(|error| error.to_string())?;
            Ok(0)
        }
    }
}

fn print_status(view: &crate::project::StatusView) -> io::Result<()> {
    let mut out = io::stdout().lock();
    writeln!(
        out,
        "Current validation: {}",
        if view.satisfied {
            "SATISFIED"
        } else {
            "NOT SATISFIED"
        }
    )?;
    for artifact in &view.artifacts {
        writeln!(
            out,
            "  Artifact {}: {} ({}/{} Evals){}",
            artifact.id,
            artifact.state,
            artifact.passed,
            artifact.total,
            artifact
                .family
                .as_ref()
                .map_or(String::new(), |family| format!(" [family {family}]"))
        )?;
    }
    for eval in &view.evals {
        writeln!(
            out,
            "  {}: {} — {}{}\n    {}",
            eval.id,
            eval.state,
            eval.action,
            if eval.included {
                ""
            } else {
                " (not included; use --recursive)"
            },
            eval.reason
        )?;
        if let Some(last) = &eval.last {
            writeln!(
                out,
                "    Last: {} (Run {}; historical, not current evidence)",
                last.verdict, last.run_id
            )?;
        }
    }
    for artifact in &view.obligations {
        writeln!(out, "  Unmet obligation: {artifact}")?;
    }
    writeln!(
        out,
        "Verify actions: execute {}, reuse {}, wait {}, blocked {}",
        view.counts.execute, view.counts.reuse, view.counts.wait, view.counts.blocked
    )?;
    Ok(())
}

fn print_graph(view: &crate::query::GraphView<'_>) -> io::Result<()> {
    let mut out = io::stdout().lock();
    writeln!(
        out,
        "Graph: {} Artifacts, {} Evals",
        view.artifacts.len(),
        view.evals.len()
    )?;
    for (name, family) in &view.families {
        writeln!(out, "  Family {name}: {}", family.artifact_ids.join(", "))?;
    }
    for component in &view.components {
        writeln!(
            out,
            "  Component {}{}: {}",
            component.id,
            if component.cyclic { " [cycle]" } else { "" },
            component.artifacts.join(", ")
        )?;
        for id in &component.artifacts {
            let artifact = view.artifacts[id];
            writeln!(
                out,
                "    Artifact {id}{} ({})",
                if artifact.basis == Some(true) {
                    " [basis]"
                } else {
                    ""
                },
                if artifact.path.as_os_str().is_empty() {
                    ".".into()
                } else {
                    artifact.path.display().to_string()
                }
            )?;
            for eval in view.evals.iter().filter(|eval| eval.target == *id) {
                writeln!(
                    out,
                    "      {}: {} -> {}",
                    eval.id,
                    if eval.deps.is_empty() {
                        "(no deps)".into()
                    } else {
                        eval.deps.join(", ")
                    },
                    eval.target
                )?;
            }
        }
    }
    for edge in &view.relations {
        use crate::scope::RelationKind;
        let detail = match &edge.relation.kind {
            RelationKind::Child { path } => format!("child path={path}"),
            RelationKind::Mount { alias } => format!("mount alias={alias}"),
            RelationKind::Instruction { eval_id, name } => {
                format!("instruction eval={eval_id} name={name}")
            }
            RelationKind::Argument {
                eval_id,
                index,
                name,
                path,
            } => format!("argv eval={eval_id} index={index} name={name} path={path}"),
        };
        writeln!(
            out,
            "  {} -> {} [{detail}]{}",
            edge.relation.source,
            edge.relation.target,
            if edge.cyclic { " [cycle]" } else { "" }
        )?;
    }
    Ok(())
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
