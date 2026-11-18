//! Command-line parsing, projections, and exit codes.

mod account;
mod cache;
use cache::CacheCommand;
mod config;
use config::ConfigCommand;
#[cfg(test)]
mod defaults_tests;
mod dispatch;
mod follow;
use follow::Follow;
mod graph;
#[cfg(test)]
mod identity_tests;
mod interactive;
mod maintenance;
#[cfg(test)]
mod monitor_tests;
mod provider;
mod remote;
mod render;
mod run;
use run::RunCommand;
mod runtime;
use runtime::cancellation_listener;
pub use runtime::{Screens, run};
mod secret;
mod status;
mod tools;
use tools::ToolsCommand;
mod workflow;
use dispatch::Context;
use render::{outcome_code, print_json};
mod request;
mod server;
mod session;
use request::RequestCommand;
use server::ServerCommand;
use session::SessionCommand;

use std::{num::NonZeroU32, path::PathBuf};

use clap::{Args, Parser, Subcommand};

use artifactize_core::{
    config::ProfileKind,
    project::selection::{ProfileSelection, Selection, read_selection_file},
    types::{RequestId, RunId},
};

/// Shells conventionally report an interrupt as 130; use the same status on every platform.
const INTERRUPTED_EXIT: u8 = 130;

/// Public exit 2 distinguishes CLI usage and operational failures from a RED verdict.
const FAILURE_EXIT: u8 = 2;

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
    /// Serve bounded verification and inspection tools over MCP stdio.
    Mcp,
    #[command(name = "__verify", hide = true)]
    VerifyWorker { run_id: RunId },
    #[command(name = "__execute", hide = true)]
    VerifyExecution { run_id: RunId },
    /// Run scoped tools or check declared review tools.
    #[command(disable_help_subcommand = true, version)]
    Tools {
        #[command(subcommand)]
        command: ToolsCommand,
    },
    /// Sign in with Codex (ChatGPT) in the system browser, or save an API key read from stdin.
    /// Claude Agent runs Claude Code's own `claude auth login`, which keeps its own login.
    Login {
        #[arg(value_enum)]
        provider: artifactize_core::config::Provider,
    },
    /// Revoke and remove artifactize's own Codex tokens, or forget a saved API key.
    /// Claude Agent runs Claude Code's own `claude auth logout`.
    Logout {
        #[arg(value_enum)]
        provider: artifactize_core::config::Provider,
    },
    /// List, add or remove the custom providers an Agent profile's backend may name.
    Provider {
        #[command(subcommand)]
        command: provider::ProviderCommand,
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
        #[arg(long, value_name = "DURATION", value_parser = artifactize_core::store::prune::parse_duration)]
        older_than: Option<std::time::Duration>,
        /// Report eligible paths without deleting anything.
        #[arg(long)]
        dry_run: bool,
    },
    /// Show the machine-wide limits; with --signoff-idle-grace, change it first.
    Limits {
        #[arg(long, value_name = "SECONDS", value_parser = seconds, help = format!(
            "Seconds a Run with nothing but unclaimed sign-offs left stays open (0–{}), for the Runs started afterwards.",
            artifactize_core::limits::MAX_SIGNOFF_IDLE_GRACE.as_secs()
        ))]
        signoff_idle_grace: Option<std::time::Duration>,
    },
    /// List the models a built-in or custom provider offers to your key or account.
    Models {
        /// The provider, as an Agent profile's backend names it.
        provider: artifactize_core::config::Backend,
    },
    /// Submit selected evals; use --wait to observe the validation outcome.
    #[command(group(clap::ArgGroup::new("required_selection")
        .args(["artifact", "eval", "evals", "artifacts", "evals_file", "artifacts_file", "all"])
        .required(true)))]
    Verify {
        #[command(flatten)]
        selection: SelectionArgs,
        #[command(flatten)]
        policy: PolicyArgs,
        /// Opt-in maximum concurrent evals, including fingerprint waiters; otherwise unlimited.
        #[arg(long, value_name = "N")]
        jobs: Option<NonZeroU32>,
        /// Limit executor starts in this Run; cache hits and waiters are free.
        #[arg(long, value_name = "N")]
        max_executions: Option<u64>,
        #[command(flatten)]
        follow: Follow,
        #[arg(long, value_name = "KEY", help = format!(
            "Retry-safe caller attempt key (1–{} ASCII letters/digits/._:-).",
            artifactize_core::types::MAX_REQUEST_KEY_BYTES
        ))]
        request_key: Option<artifactize_core::types::RequestKey>,
        #[arg(long, value_name = "TEXT", help = format!(
            "A short note on what you are working on, shown with the Run (1–{} characters).",
            artifactize_core::types::MAX_RUN_COMMENT_CHARS
        ))]
        comment: Option<artifactize_core::types::RunComment>,
        /// Only reuse results for these kinds (comma-separated); an eval with nothing to reuse is
        /// not executed.
        #[arg(long, value_name = "KINDS", value_enum, value_delimiter = ',')]
        reuse_only: Vec<ProfileKind>,
        /// Start reviews before unresolved dependency gates; cancel blocked work.
        #[arg(long)]
        speculate: bool,
        /// Removed in 0.16.0: a sign-off never holds up other evals.
        #[arg(long, hide = true)]
        defer_human: bool,
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
        args: Vec<std::ffi::OsString>,
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
    /// Browse Runs; starts on ALL regardless of cwd. Paths select only the initial repository.
    Monitor {
        /// Initial repository/worktree; all scopes remain accessible in the UI.
        #[arg(value_name = "PATH", conflicts_with = "repo")]
        path: Option<PathBuf>,
        /// Initially select ALL repositories (the default).
        #[arg(long, conflicts_with_all = ["path", "repo"])]
        all: bool,
    },
    /// Search saved requests and claim, run tools for and submit Human reviews in a terminal
    /// UI; the search starts on the waiting Human reviews of every repository.
    Review {
        /// Open this request instead of the list.
        #[arg(value_name = "REQUEST_ID")]
        request: Option<RequestId>,
        /// Search every repository (the default); with --repo the search starts on that
        /// repository's term instead.
        #[arg(long)]
        all: bool,
        /// Reviewer name (defaults to the signed-in user).
        #[arg(long, value_name = "NAME")]
        reviewer: Option<artifactize_core::types::ReviewerId>,
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
    eval: Option<artifactize_core::types::EvalId>,
    /// Select comma-separated qualified Eval IDs.
    #[arg(long, value_name = "CSV", value_delimiter = ',', num_args = 1)]
    evals: Option<Vec<artifactize_core::types::EvalId>>,
    /// Select comma-separated Artifact names.
    #[arg(long, value_name = "CSV", value_delimiter = ',', num_args = 1)]
    artifacts: Option<Vec<artifactize_core::types::ArtifactName>>,
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
        match (
            self.artifact,
            self.eval,
            self.evals,
            self.artifacts,
            self.evals_file,
            self.artifacts_file,
            self.all,
        ) {
            (None, None, None, None, None, None, _) => Ok(Selection::All),
            (Some(Selector::Artifact(artifact_id)), None, None, None, None, None, false) => {
                Ok(Selection::Artifact { artifact_id })
            }
            (Some(Selector::Eval(eval_id)), None, None, None, None, None, false)
            | (None, Some(eval_id), None, None, None, None, false) => {
                Ok(Selection::Eval { eval_id })
            }
            (None, None, Some(eval_ids), None, None, None, false) => {
                Ok(Selection::Evals { eval_ids })
            }
            (None, None, None, Some(artifact_ids), None, None, false) => {
                Ok(Selection::Artifacts { artifact_ids })
            }
            (None, None, None, None, Some(path), None, false) => Ok(Selection::Evals {
                eval_ids: selection_file(&path)?,
            }),
            (None, None, None, None, None, Some(path), false) => Ok(Selection::Artifacts {
                artifact_ids: selection_file(&path)?,
            }),
            _ => unreachable!("clap makes Artifact and Eval selectors exclusive"),
        }
    }
}

#[derive(Debug, Args)]
pub struct PolicyArgs {
    /// Use a declared profile variant for every included Eval.
    #[arg(long, value_name = "NAME")]
    profile: Option<artifactize_core::config::ProfileVariantName>,
    /// Include all Evals in the required dependency scope, including cycle peers.
    #[arg(long)]
    recursive: bool,
    /// Force explicitly selected Evals only; dependency gates still apply.
    #[arg(long)]
    force: bool,
    /// Fingerprints to compute at once (default: the available CPUs).
    #[arg(long, value_name = "N")]
    fingerprint_jobs: Option<NonZeroU32>,
}

impl PolicyArgs {
    fn options(self) -> artifactize_core::project::VerifyOptions {
        artifactize_core::project::VerifyOptions {
            profile: self.profile.map(ProfileSelection::Named),
            recursive: self.recursive,
            force: self.force,
            fingerprint_jobs: self.fingerprint_jobs.map(|jobs| jobs.get() as usize),
            ..Default::default()
        }
    }
}

mod argv;
use argv::{ParseError, parse_args};

/// The positional selection intentionally accepts either an Artifact or qualified Eval.
#[derive(Debug, Clone)]
enum Selector {
    Artifact(artifactize_core::types::ArtifactName),
    Eval(artifactize_core::types::EvalId),
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

/// Whole seconds as a duration, read at the command line.
fn seconds(text: &str) -> Result<std::time::Duration, String> {
    text.parse::<u64>()
        .map(std::time::Duration::from_secs)
        .map_err(|error| error.to_string())
}
