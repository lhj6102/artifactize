//! Command-line parsing, projections, and exit codes.

mod cache;
#[cfg(test)]
mod defaults_tests;
mod dispatch;
#[cfg(test)]
mod identity_tests;
mod interactive;
mod maintenance;
mod remote;
mod render;
mod run;
mod workflow;
use dispatch::Context;
use render::{outcome_code, print_json};
mod request;
mod server;
mod session;
use request::RequestCommand;
use server::ServerCommand;
use session::SessionCommand;

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, error::ErrorKind};
use serde_json::json;

use crate::{
    config::ProfileKind,
    project::selection::{ProfileSelection, Selection, read_selection_file},
    types::{RequestId, ReuseKey, RunId},
};

/// Keep default Run history output readable; --limit/--offset expose older records.
const DEFAULT_RUN_LIST_LIMIT: u32 = 50;

#[derive(Debug, Parser)]
#[command(
    name = "artifactize",
    // Help names the command, not the file run; on Windows that is artifactize.exe.
    bin_name = "artifactize",
    version,
    about = "The AI-native collaboration layer for one-of-a-kind teammates and their agents."
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
        #[arg(
            long,
            value_name = "N",
            default_value_t = crate::project::DEFAULT_JOBS as u32,
            value_parser = clap::value_parser!(u32).range(1..)
        )]
        jobs: u32,
        /// Limit executor starts in this Run; cache hits and waiters are free.
        #[arg(long, value_name = "N")]
        max_executions: Option<u64>,
        /// How long to wait for Human results; does not cancel pending requests (default 600000).
        #[arg(
            long,
            value_name = "MS",
            value_parser = timeout_duration
        )]
        timeout_ms: Option<std::time::Duration>,
        /// Only reuse results for these kinds (comma-separated); an eval with nothing to reuse is
        /// not executed.
        #[arg(long, value_name = "KINDS", value_enum, value_delimiter = ',')]
        reuse_only: Vec<ProfileKind>,
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
    /// Read a saved Agent review conversation or continue it with a follow-up.
    Session {
        #[command(subcommand)]
        command: SessionCommand,
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
    /// Browse repositories/worktrees, Runs and eval details; initial selection is the current
    /// worktree.
    Monitor {
        /// Initially select ALL repositories; every scope remains accessible in the UI.
        #[arg(long)]
        all: bool,
    },
    /// Claim, run tools for and submit waiting Human reviews in a terminal UI; defaults to the
    /// current repository.
    Review {
        /// Open this request instead of the waiting list.
        #[arg(value_name = "REQUEST_ID")]
        request: Option<RequestId>,
        /// List waiting requests from every repository in the shared state.
        #[arg(long)]
        all: bool,
        /// Reviewer name (defaults to USER).
        #[arg(long, value_name = "NAME")]
        reviewer: Option<crate::types::ReviewerId>,
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
    /// Select one Artifact.
    artifact: Option<Selector>,
    /// Select one qualified Eval ID.
    #[arg(long, value_name = "ID")]
    eval: Option<crate::types::EvalId>,
    /// Select comma-separated qualified Eval IDs.
    #[arg(long, value_name = "CSV", value_delimiter = ',', num_args = 1)]
    evals: Option<Vec<crate::types::EvalId>>,
    /// Select comma-separated Artifact names.
    #[arg(long, value_name = "CSV", value_delimiter = ',', num_args = 1)]
    artifacts: Option<Vec<crate::types::ArtifactName>>,
    /// Read Eval IDs from a JSON array or one ID per line.
    #[arg(long, value_name = "PATH")]
    evals_file: Option<PathBuf>,
    /// Read Artifact names from a JSON array or one ID per line.
    #[arg(long, value_name = "PATH")]
    artifacts_file: Option<PathBuf>,
    /// Select every Artifact.
    #[arg(long)]
    all: bool,
}

impl SelectionArgs {
    fn resolve(self) -> Result<Selection, String> {
        if let Some(selector) = self.artifact {
            Ok(match selector {
                Selector::Artifact(id) => Selection::Artifact { artifact_id: id },
                Selector::Eval(id) => Selection::Eval { eval_id: id },
            })
        } else if let Some(eval_id) = self.eval {
            Ok(Selection::Eval { eval_id })
        } else if let Some(ids) = self.evals {
            Ok(Selection::Evals { eval_ids: ids })
        } else if let Some(ids) = self.artifacts {
            Ok(Selection::Artifacts { artifact_ids: ids })
        } else if let Some(path) = self.evals_file {
            Ok(Selection::Evals {
                eval_ids: selection_file::<crate::types::EvalId>(&path)?,
            })
        } else if let Some(path) = self.artifacts_file {
            Ok(Selection::Artifacts {
                artifact_ids: selection_file::<crate::types::ArtifactName>(&path)?,
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
    profile: Option<crate::config::ProfileVariantName>,
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
    /// Inspect full static definitions, relations, and cycles.
    Graph {
        /// Select one Artifact and its required closure; defaults to all.
        artifact: Option<crate::types::ArtifactName>,
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
        #[arg(long, value_name = "N", default_value_t = DEFAULT_RUN_LIST_LIMIT)]
        limit: u32,
        /// Skip this many Runs before returning results.
        #[arg(long, value_name = "N", default_value = "0")]
        offset: u32,
    },
    /// Read the full saved audit as JSON, even without --json.
    Show {
        run_id: RunId,
        /// Follow a RUNNING Run until it finishes and exit with its outcome code.
        #[arg(long)]
        wait: bool,
        /// Wait timeout; never cancels the Run (default 600000).
        #[arg(
            long,
            value_name = "MS",
            requires = "wait",
            value_parser = timeout_duration
        )]
        timeout_ms: Option<std::time::Duration>,
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
        key: ReuseKey,
        /// Print every record of the key as a JSON array, latest first.
        #[arg(long)]
        history: bool,
    },
    /// Remove every record of an unused key, preserving saved Runs and executions.
    Rm { key: ReuseKey },
}

#[derive(Debug, Subcommand)]
pub enum ToolsCommand {
    /// Static preflight by default; --execute opts in to one tool call.
    Check(crate::diagnostics::ToolCheckOptions),
}

fn cancellation_listener() -> Result<
    (
        tokio_util::sync::CancellationToken,
        tokio::task::JoinHandle<()>,
    ),
    String,
> {
    let cancellation = tokio_util::sync::CancellationToken::new();
    let stop = crate::platform::stop_requested().map_err(|e| e.to_string())?;
    let token = cancellation.clone();
    let listener = tokio::spawn(async move {
        stop.await;
        token.cancel();
    });
    Ok((cancellation, listener))
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
    match runtime.block_on(async {
        let result = dispatch::execute(cli).await;
        crate::changes::drain().await;
        result
    }) {
        Ok(code) => ExitCode::from(code),
        Err(error) => failure(&error, json),
    }
}

/// Parse protocol milliseconds exactly once, preserving the public signed 32-bit bound.
fn timeout_duration(text: &str) -> Result<std::time::Duration, String> {
    let millis: u64 = text
        .parse()
        .map_err(|_| "timeout must be milliseconds".to_owned())?;
    if !(1..=crate::config::validation::MAX_TIMEOUT_MS).contains(&millis) {
        return Err("timeoutMs must be between 1 and 2147483647".into());
    }
    Ok(std::time::Duration::from_millis(millis))
}

/// The positional selection intentionally accepts either an Artifact or qualified Eval.
#[derive(Debug, Clone)]
enum Selector {
    Artifact(crate::types::ArtifactName),
    Eval(crate::types::EvalId),
}
impl std::str::FromStr for Selector {
    type Err = String;
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.contains('/') {
            text.parse().map(Self::Eval)
        } else {
            text.parse().map(Self::Artifact)
        }
    }
}
fn selection_file<T>(path: &std::path::Path) -> Result<Vec<T>, String>
where
    T: std::str::FromStr<Err = String>,
{
    read_selection_file(path)?
        .into_iter()
        .map(|text| text.parse::<T>())
        .collect()
}
