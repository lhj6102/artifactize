//! Command-line parsing, projections, and exit codes.

mod remote;
mod request;
mod server;
use request::RequestCommand;
use server::ServerCommand;

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Args, CommandFactory, Parser, Subcommand, error::ErrorKind};
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
    /// Check declared tools, optionally invoking exactly one without a review.
    Tools {
        #[command(subcommand)]
        command: ToolsCommand,
    },
    /// Sign in with Codex (ChatGPT) in the system browser and save artifactize's tokens.
    Login {
        #[command(subcommand)]
        provider: AuthProvider,
    },
    /// Revoke and remove artifactize's own Codex tokens.
    Logout {
        #[command(subcommand)]
        provider: AuthProvider,
    },
    /// Sign in to, sign out of, or check a shared remote review store.
    Remote {
        #[command(subcommand)]
        command: remote::RemoteCommand,
    },
    /// Check local readiness without provider calls or creating a Run.
    Doctor,
    /// Remove finished, unowned Run output; preserve database audit and repositories.
    Prune {
        /// Minimum age since Run completion (for example 7d, 24h, 30m).
        #[arg(long, value_name = "DURATION", value_parser = crate::store::prune::parse_duration)]
        older_than: Option<std::time::Duration>,
        /// Report eligible paths without deleting anything.
        #[arg(long)]
        dry_run: bool,
    },
    /// List the models a backend offers to your key or account.
    Models {
        #[command(subcommand)]
        provider: ModelProvider,
    },
    /// Execute selected evals in the foreground.
    #[command(group(clap::ArgGroup::new("required_selection")
        .args(["artifact", "eval", "evals", "artifacts", "evals_file", "artifacts_file", "all"])
        .required(true)))]
    Verify {
        #[command(flatten)]
        selection: SelectionArgs,
        #[command(flatten)]
        policy: PolicyArgs,
        /// Maximum concurrent evals, including fingerprint waiters.
        #[arg(long, value_name = "N", default_value = "4", value_parser = clap::value_parser!(u32).range(1..))]
        jobs: u32,
        /// Limit executor starts in this Run; cache hits and waiters are free.
        #[arg(long, value_name = "N")]
        max_executions: Option<u64>,
        /// Keep this Run alive while Human results are pending.
        #[arg(long)]
        wait: bool,
        /// Human wait timeout; does not cancel pending requests (default 600000).
        #[arg(long, value_name = "MS", requires = "wait", value_parser = clap::value_parser!(u32).range(1..=2_147_483_647))]
        timeout_ms: Option<u32>,
    },
    /// Inspect current validation, saved Run audit, and what verify would do.
    Status {
        #[command(flatten)]
        selection: SelectionArgs,
        #[command(flatten)]
        policy: PolicyArgs,
    },
    /// Moved to "artifactize config graph".
    #[command(hide = true, disable_help_flag = true)]
    Graph {
        // Any former graph arguments, including --help, get the same hint.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
        args: Vec<String>,
    },
    /// Read recorded Runs without discovering or executing project code.
    Run {
        #[command(subcommand)]
        command: RunCommand,
    },
    /// Read saved requests or claim, inspect, and submit Human reviews.
    Request {
        #[command(subcommand)]
        command: RequestCommand,
    },
    /// Inspect or maintain reusable results by reuse key without a repository.
    Cache {
        #[command(subcommand)]
        command: CacheCommand,
    },
    /// Inspect static folder declarations without executing hooks or reviews.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Watch saved and running Runs in a read-only terminal UI; defaults to the current repository.
    Monitor {
        /// Show Runs from every repository in the shared state.
        #[arg(long)]
        all: bool,
    },
    /// Claim, run tools for and submit waiting Human reviews in a terminal UI; defaults to the current repository.
    Review {
        /// Open this request instead of the waiting list.
        #[arg(value_name = "REQUEST_ID")]
        request: Option<String>,
        /// List waiting requests from every repository in the shared state.
        #[arg(long)]
        all: bool,
        /// Reviewer name (defaults to USER).
        #[arg(long, value_name = "NAME")]
        reviewer: Option<String>,
    },
    /// Serve or administer a shared remote review store (review-store.sqlite).
    Server {
        #[command(subcommand)]
        command: ServerCommand,
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

#[derive(Debug, Args)]
pub struct PolicyArgs {
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
    /// Fingerprints to compute at once (default: the available CPUs).
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..))]
    fingerprint_jobs: Option<u32>,
}

impl PolicyArgs {
    fn options(self) -> crate::project::VerifyOptions {
        crate::project::VerifyOptions {
            profile: self.profile.map(ProfileSelection::Named),
            recursive: self.recursive,
            force: self.force,
            ignore_gates: self.ignore_gates.then_some(true),
            fingerprint_jobs: self.fingerprint_jobs.map(|jobs| jobs as usize),
            ..Default::default()
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum AuthProvider {
    /// The ChatGPT/Codex account that the codex backend uses.
    Codex,
}

#[derive(Debug, Subcommand)]
pub enum ModelProvider {
    /// OpenAI API models (OPENAI_API_KEY).
    Openai,
    /// Anthropic API models (ANTHROPIC_API_KEY).
    Anthropic,
    /// Codex models your ChatGPT account can use (artifactize login codex).
    Codex,
}

impl ModelProvider {
    fn backend(self) -> crate::config::Backend {
        match self {
            Self::Openai => crate::config::Backend::Openai,
            Self::Anthropic => crate::config::Backend::Anthropic,
            Self::Codex => crate::config::Backend::Codex,
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Validate declarations and unique Artifact/Eval ids.
    Check,
    /// Inspect full static definitions, relations, cycles, and families.
    Graph {
        /// Select one Artifact or family and its required closure; defaults to all.
        artifact: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum RunCommand {
    /// List saved Runs, newest first; defaults to the current repository.
    List {
        /// Explicitly restrict results to --repo (the default).
        #[arg(long, conflicts_with = "all")]
        repo_only: bool,
        /// List Runs from every repository in the shared state.
        #[arg(long)]
        all: bool,
        /// Maximum number of Runs to return.
        #[arg(long, value_name = "N", default_value = "50")]
        limit: u32,
        /// Skip this many Runs before returning results.
        #[arg(long, value_name = "N", default_value = "0")]
        offset: u32,
    },
    /// Read the full saved audit as JSON, even without --json.
    Show {
        run_id: String,
        /// Follow a RUNNING Run until it finishes and exit with its outcome code.
        #[arg(long)]
        wait: bool,
        /// Wait timeout; never cancels the Run (default 600000).
        #[arg(long, value_name = "MS", requires = "wait", value_parser = clap::value_parser!(u32).range(1..=2_147_483_647))]
        timeout_ms: Option<u32>,
    },
}

#[derive(Debug, Subcommand)]
pub enum CacheCommand {
    /// List the latest record of each reuse key, the one verify reuses.
    List {
        /// List every record of every key, latest first per key.
        #[arg(long)]
        history: bool,
    },
    /// Read a key's latest record (result, profile, options, provenance) as JSON.
    Show {
        key: String,
        /// Print every record of the key as a JSON array, latest first.
        #[arg(long)]
        history: bool,
    },
    /// Remove every record of an unused key, preserving saved Runs and executions.
    Rm { key: String },
}

#[derive(Debug, Subcommand)]
pub enum ToolsCommand {
    /// Static preflight by default; --execute opts in to one tool call.
    Check(crate::diagnostics::ToolCheckOptions),
}

async fn execute(cli: Cli) -> Result<u8, String> {
    match cli.command {
        Some(Command::Tools {
            command: ToolsCommand::Check(options),
        }) => {
            let (cancellation, listener) = cancellation_listener()?;
            let result = crate::diagnostics::check_tools(
                &cli.repo.unwrap_or_else(|| PathBuf::from(".")),
                cli.state_dir.as_deref(),
                &options,
                cancellation,
            )
            .await;
            listener.abort();
            let report = result?;
            print_json(&report)?;
            Ok(u8::from(!report.ok))
        }
        Some(Command::Login {
            provider: AuthProvider::Codex,
        }) => {
            crate::auth::codex::login(cli.state_dir.as_deref(), cli.repo.as_deref()).await?;
            if cli.json {
                print_json(&json!({ "provider": "codex", "signed_in": true }))?;
            } else {
                writeln!(io::stdout().lock(), "Signed in with Codex.")
                    .map_err(|e| e.to_string())?;
            }
            Ok(0)
        }
        Some(Command::Logout {
            provider: AuthProvider::Codex,
        }) => {
            let revoked =
                crate::auth::codex::logout(cli.state_dir.as_deref(), cli.repo.as_deref()).await?;
            if !revoked {
                writeln!(
                    io::stderr().lock(),
                    "Local tokens removed; their revocation was not confirmed."
                )
                .map_err(|e| e.to_string())?;
            }
            if cli.json {
                print_json(
                    &json!({ "provider": "codex", "signed_in": false, "revoked": revoked }),
                )?;
            } else {
                writeln!(io::stdout().lock(), "Signed out of Codex.").map_err(|e| e.to_string())?;
            }
            Ok(0)
        }
        Some(Command::Remote { command }) => {
            remote::execute(
                cli.state_dir.as_deref(),
                cli.repo.as_deref(),
                command,
                cli.json,
            )
            .await
        }
        Some(Command::Doctor) => {
            let report =
                crate::diagnostics::doctor(cli.state_dir.as_deref(), cli.repo.as_deref()).await?;
            if cli.json {
                print_json(&report)?;
            } else {
                let mut out = io::stdout().lock();
                writeln!(out, "State directory: {}", report.state_dir.display())
                    .map_err(|e| e.to_string())?;
                for check in &report.checks {
                    writeln!(out, "{} {}: {}", check.status, check.name, check.message)
                        .map_err(|e| e.to_string())?;
                }
            }
            Ok(u8::from(!report.ok))
        }
        Some(Command::Prune {
            older_than,
            dry_run,
        }) => {
            let state = match cli.state_dir {
                Some(state) => state,
                None => crate::store::state_home().map_err(|e| e.to_string())?,
            };
            let report =
                crate::store::prune::prune(&state, cli.repo.as_deref(), older_than, dry_run)?;
            if cli.json {
                print_json(&report)?;
            } else {
                let mut out = io::stdout().lock();
                for path in &report.removed {
                    writeln!(out, "Removed {}", path.display()).map_err(|e| e.to_string())?;
                }
                for path in &report.would_remove {
                    writeln!(out, "Would remove {}", path.display()).map_err(|e| e.to_string())?;
                }
                for id in &report.skipped_runs {
                    writeln!(out, "Skipped Run {id}").map_err(|e| e.to_string())?;
                }
                writeln!(out, "Database audit and repository files were preserved.")
                    .map_err(|e| e.to_string())?;
            }
            Ok(0)
        }
        Some(Command::Models { provider }) => {
            let listing = crate::llm::models::list(
                provider.backend(),
                cli.state_dir.as_deref(),
                cli.repo.as_deref(),
            )
            .await?;
            if cli.json {
                print_json(&listing)?;
            } else {
                let mut out = io::stdout().lock();
                for model in listing.models {
                    writeln!(out, "{}\t{}", model.slug, model.display_name)
                        .map_err(|e| e.to_string())?;
                }
            }
            Ok(0)
        }
        None => {
            Cli::command()
                .write_help(&mut io::stdout().lock())
                .map_err(|error| error.to_string())?;
            Ok(0)
        }
        Some(Command::Verify {
            selection,
            policy,
            jobs,
            max_executions,
            wait,
            timeout_ms,
        }) => {
            let selection = selection.resolve()?;
            let options = crate::project::VerifyOptions {
                jobs: jobs as usize,
                max_executions,
                wait_timeout_ms: wait.then_some(timeout_ms.unwrap_or(600_000)),
                announce_run: true,
                ..policy.options()
            };
            let (cancellation, listener) = cancellation_listener()?;
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
                print_json(&crate::query::run_output(&view))?;
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
                if let Some(error) = &view.run.error {
                    writeln!(stdout, "Reason: {error}").map_err(|e| e.to_string())?;
                }
                for request in &view.requests {
                    writeln!(
                        stdout,
                        "  {} [{}]: {}{}{}",
                        request.eval_id,
                        request.id,
                        request.status,
                        reuse_marker(request),
                        request
                            .error
                            .as_ref()
                            .or(request.blocked_reason.as_ref())
                            .map_or(String::new(), |reason| format!(" — {reason}"))
                    )
                    .map_err(|e| e.to_string())?;
                }
                let output = crate::query::run_output(&view);
                let summary = &output["summary"];
                writeln!(
                    stdout,
                    "Summary: executed {}, reused {}{}",
                    kinds(&summary["executed"]),
                    kinds(&summary["reused"]),
                    match summary["reused"]["otherProfile"].as_u64() {
                        Some(0) | None => String::new(),
                        Some(count) => format!("; {count} produced by another profile"),
                    }
                )
                .map_err(|e| e.to_string())?;
                let usage = &output["usage"];
                if *usage != json!({"spent":{},"saved":{}}) {
                    writeln!(
                        stdout,
                        "Usage: spent {}; saved {}",
                        counters(&usage["spent"]),
                        counters(&usage["saved"])
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
            Ok(outcome_code(&view.run))
        }
        Some(Command::Status { selection, policy }) => {
            let selection = selection.resolve()?;
            let (cancellation, listener) = cancellation_listener()?;
            let result = crate::project::status(
                &cli.repo.unwrap_or_else(|| PathBuf::from(".")),
                cli.state_dir.as_deref(),
                &selection,
                &policy.options(),
                cancellation,
            )
            .await;
            listener.abort();
            let view = result?;
            if cli.json {
                print_json(&view)?;
            } else {
                print_status(&view).map_err(|error| error.to_string())?;
            }
            Ok(u8::from(!view.satisfied))
        }
        Some(Command::Graph { .. }) => Err(r#"graph moved to "artifactize config graph""#.into()),
        Some(Command::Config {
            command: ConfigCommand::Graph { artifact },
        }) => {
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
            command: RunCommand::List {
                all, limit, offset, ..
            },
        }) => {
            let state = crate::store::state_dir(cli.state_dir.as_deref())?;
            let repo = (!all).then(|| cli.repo.unwrap_or_else(|| PathBuf::from(".")));
            let runs = crate::store::read_runs(&state, repo.as_deref(), limit, offset).await?;
            if cli.json {
                print_json(&runs)?;
            } else {
                let mut out = io::stdout().lock();
                writeln!(out, "ID\tREPO\tCREATED\tCOMPLETED\tSTATUS\tREQUEST COUNTS")
                    .map_err(|e| e.to_string())?;
                for run in runs {
                    let counts = run
                        .counts
                        .iter()
                        .map(|(state, count)| format!("{state}={count}"))
                        .collect::<Vec<_>>()
                        .join(" ");
                    writeln!(
                        out,
                        "{}\t{}\t{}\t{}\t{}\t{}",
                        run.id,
                        run.repo_path.display(),
                        run.created_at,
                        run.completed_at.as_deref().unwrap_or("-"),
                        run.status,
                        counts
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
            Ok(0)
        }
        Some(Command::Run {
            command:
                RunCommand::Show {
                    run_id,
                    wait,
                    timeout_ms,
                },
        }) => {
            let state = crate::store::state_dir(cli.state_dir.as_deref())?;
            let deadline = tokio::time::Instant::now()
                + Duration::from_millis(timeout_ms.unwrap_or(600_000).into());
            let mut view = crate::store::read_run(&state, &run_id).await?;
            while wait && view.run.status == "RUNNING" {
                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                if remaining.is_zero() {
                    break;
                }
                tokio::time::sleep(remaining.min(Duration::from_millis(200))).await;
                view = crate::store::read_run(&state, &run_id).await?;
            }
            print_json(&crate::query::run_output(&view))?;
            if !wait {
                return Ok(0);
            }
            if view.run.status == "RUNNING" {
                writeln!(
                    io::stderr().lock(),
                    "Waiting timed out; Run {run_id} is still RUNNING."
                )
                .map_err(|e| e.to_string())?;
                return Ok(3);
            }
            Ok(outcome_code(&view.run))
        }
        Some(Command::Monitor { all }) => {
            if cli.json {
                return Err(
                    "monitor is an interactive terminal UI; --json is not supported.".into(),
                );
            }
            if all && cli.repo.is_some() {
                return Err("monitor accepts --repo or --all, not both.".into());
            }
            let state = crate::store::state_dir(cli.state_dir.as_deref())?;
            let repo = (!all).then(|| cli.repo.unwrap_or_else(|| PathBuf::from(".")));
            let (cancellation, listener) = cancellation_listener()?;
            let result = crate::monitor::run(state, repo, cancellation).await;
            listener.abort();
            result?;
            Ok(0)
        }
        Some(Command::Review {
            request,
            all,
            reviewer,
        }) => {
            if cli.json {
                return Err(
                    "review is an interactive terminal UI; --json is not supported.".into(),
                );
            }
            if all && cli.repo.is_some() {
                return Err("review accepts --repo or --all, not both.".into());
            }
            let reviewer = match reviewer {
                Some(reviewer) => crate::human::validate_reviewer(&reviewer).map(|()| reviewer),
                None => crate::human::default_reviewer(),
            }?;
            let state = crate::store::state_dir(cli.state_dir.as_deref())?;
            if let Some(id) = &request {
                crate::store::read_request(&state, id).await?;
            }
            let repo = (!all).then(|| cli.repo.unwrap_or_else(|| PathBuf::from(".")));
            let (cancellation, listener) = cancellation_listener()?;
            let result = crate::review::run(state, repo, reviewer, request, cancellation).await;
            listener.abort();
            result?;
            Ok(0)
        }
        Some(Command::Server { command }) => {
            let state = crate::store::state_dir(cli.state_dir.as_deref())?;
            server::execute(&state, command, cli.json).await
        }
        Some(Command::Request { command }) => {
            let state = crate::store::state_dir(cli.state_dir.as_deref())?;
            request::execute(&state, command, cli.json).await
        }
        Some(Command::Cache { command }) => {
            let state = crate::store::state_dir(cli.state_dir.as_deref())?;
            match command {
                CacheCommand::List { history } => {
                    let entries = crate::cache::list(&state, history).await?;
                    if cli.json {
                        print_json(&entries)?;
                    } else {
                        let mut out = io::stdout().lock();
                        writeln!(
                            out,
                            "KEY\tEVAL\tVERDICT\tCOMPLETED\tPRODUCER\tSOURCE\tRECORDS\tBYTES\tLAST USED"
                        )
                        .map_err(|e| e.to_string())?;
                        for entry in entries {
                            writeln!(
                                out,
                                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                                entry.key,
                                entry.eval_id,
                                entry.verdict,
                                entry.completed_at.as_deref().unwrap_or("-"),
                                entry.producer.as_deref().unwrap_or("-"),
                                entry.origin.as_ref().unwrap_or(&entry.repo_path),
                                entry.records,
                                entry.bytes,
                                entry.last_used
                            )
                            .map_err(|e| e.to_string())?;
                        }
                    }
                }
                CacheCommand::Show { key, history } => {
                    let mut records = crate::cache::show(&state, &key, history).await?;
                    let found = !records.is_empty();
                    if history {
                        print_json(&records)?;
                    } else {
                        print_json(&records.pop())?;
                    }
                    return Ok(if found { 0 } else { 4 });
                }
                CacheCommand::Rm { key } => {
                    print_json(&json!({"removed": crate::cache::remove(&state, &key).await?}))?;
                }
            }
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

fn cancellation_listener() -> Result<
    (
        tokio_util::sync::CancellationToken,
        tokio::task::JoinHandle<()>,
    ),
    String,
> {
    let cancellation = tokio_util::sync::CancellationToken::new();
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        .map_err(|e| e.to_string())?;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|e| e.to_string())?;
    let token = cancellation.clone();
    let listener = tokio::spawn(async move {
        tokio::select! { _ = interrupt.recv() => {}, _ = terminate.recv() => {} }
        token.cancel();
    });
    Ok((cancellation, listener))
}

/// Where a reused result came from: its source Run, plus the producer for a remote result,
/// or the reviewer and the authenticated publisher for a remote Human sign-off.
fn reuse_marker(request: &crate::store::Request) -> String {
    let Some(source) = request
        .provenance
        .as_ref()
        .filter(|_| crate::query::reused(request))
    else {
        return String::new();
    };
    let profile = if request.profile == request.requested_profile {
        String::new()
    } else {
        format!(
            ", profile {}",
            crate::query::profile_name(&request.profile, &request.options)
        )
    };
    match &request.origin {
        None => format!(" (reused from {}{profile})", source.run_id),
        Some(origin) if request.profile["kind"] == "human" => format!(
            " (reused from remote: Human sign-off by {}, published by {}, {})",
            request.reviewer.as_deref().unwrap_or("unknown"),
            origin.publisher,
            source.run_id
        ),
        Some(_) => format!(
            " (reused from remote: {}, {}{profile})",
            request
                .producer
                .as_ref()
                .map_or("unknown producer", |producer| producer.name.as_str()),
            source.run_id
        ),
    }
}

/// `N (runtime R, agent A, human H)` from a summary tally.
fn kinds(tally: &serde_json::Value) -> String {
    format!(
        "{} (runtime {}, agent {}, human {})",
        tally["total"], tally["runtime"], tally["agent"], tally["human"]
    )
}

fn counters(totals: &serde_json::Value) -> String {
    let pairs: Vec<_> = totals
        .as_object()
        .into_iter()
        .flatten()
        .map(|(key, value)| format!("{key} {value}"))
        .collect();
    if pairs.is_empty() {
        "none".into()
    } else {
        pairs.join(", ")
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
        if let Some(changes) = &eval.changes {
            writeln!(
                out,
                "    Fingerprint changed since Run {}: {}",
                changes.since_run_id, changes.summary
            )?;
        }
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
        "Verify actions: will execute {}, will reuse {}, wait {}, blocked {}",
        view.counts.execute, view.counts.reuse, view.counts.wait, view.counts.blocked
    )?;
    if view.counts.wait > 0 {
        writeln!(
            out,
            "  wait: needs a result verify has not produced yet (a dependency it executes, or a live execution); status cannot predict it."
        )?;
    }
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

/// Run outcome exit codes shared by `verify` and `run show --wait`.
fn outcome_code(run: &crate::store::Run) -> u8 {
    if run.wait_timed_out {
        return 3;
    }
    match run.status.as_str() {
        "GREEN" => 0,
        "RED" => 1,
        "INCOMPLETE" => 4,
        _ => 2,
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

/// Global options may precede or follow the subcommand, but only once in total.
fn repeated_global(options: &[&str]) -> Option<&'static str> {
    ["--repo", "--state-dir", "--json"]
        .into_iter()
        .find(|name| {
            options
                .iter()
                .filter(|arg| {
                    arg.strip_prefix(name)
                        .is_some_and(|rest| rest.is_empty() || rest.starts_with('='))
                })
                .count()
                > 1
        })
}

pub fn run() -> ExitCode {
    let args: Vec<_> = std::env::args_os().collect();
    let options: Vec<_> = args
        .iter()
        .skip(1)
        .take_while(|arg| *arg != "--")
        .filter_map(|arg| arg.to_str())
        .collect();
    let json = options.contains(&"--json");
    if let Some(name) = repeated_global(&options) {
        return failure(&format!("{name} cannot be used multiple times."), json);
    }
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) if json && error.use_stderr() => {
            let message = error.to_string();
            let message = if error.kind() == ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand {
                "A subcommand is required; see --help."
            } else {
                let usage = message.split("\n\n").next().unwrap_or_default();
                usage.trim_start_matches("error: ")
            };
            return failure(message, true);
        }
        Err(error) => {
            return if error.print().is_ok() && !error.use_stderr() {
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
