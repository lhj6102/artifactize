//! Command-line parsing, projections, and exit codes.

mod account;
mod cache;
use cache::CacheCommand;
mod chat_agent;
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
mod plugin;
mod provider;
mod remote;
mod render;
mod run;
use run::RunCommand;
mod runtime;
use runtime::cancellation_listener;
pub use runtime::run;
mod secret;
mod selection;
use selection::SelectionArgs;
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
    project::selection::ProfileSelection,
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
    command: Option<CommandArgs>,
}

/// Raw interactive arguments are lowered to an exclusive scope at the parse boundary.
#[derive(Debug, Subcommand)]
enum CommandArgs {
    /// Open the Desktop app to browse Runs; paths select only the initial repository.
    Monitor {
        /// Initial repository/worktree; all scopes remain accessible in the UI.
        #[arg(value_name = "PATH", conflicts_with = "repo")]
        path: Option<PathBuf>,
        /// Initially select ALL repositories (the default).
        #[arg(long, conflicts_with_all = ["path", "repo"])]
        all: bool,
    },
    /// Open a Human review in the Desktop app (defaults to the newest waiting review).
    Review {
        /// Open this request instead of the newest waiting review.
        #[arg(value_name = "REQUEST_ID")]
        request: Option<RequestId>,
        /// Select no initial repository (the default).
        #[arg(long)]
        all: bool,
        /// Reviewer name (defaults to the signed-in user).
        #[arg(long, value_name = "NAME")]
        reviewer: Option<artifactize_core::types::ReviewerId>,
    },
    /// Run scoped tools or check declared review tools.
    #[command(disable_help_subcommand = true, version)]
    Tools {
        #[command(subcommand)]
        command: ToolsCommand,
    },
    #[command(flatten)]
    General(Command),
}

/// A removed flag's value: none exists, since parsing refuses the flag.
#[derive(Clone, Debug)]
pub enum Removed {}

fn removed_defer_human(_: &str) -> Result<Removed, String> {
    Err("--defer-human was removed in 0.16.0: every verify lets machine work go on while sign-offs wait; remove the flag.".into())
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Serve bounded verification and inspection tools over MCP stdio.
    Mcp,
    #[command(name = "__chat-mcp", hide = true)]
    ChatMcp,
    #[command(name = "__verify", hide = true)]
    VerifyWorker { run_id: RunId },
    #[command(name = "__execute", hide = true)]
    VerifyExecution { run_id: RunId },
    /// Sign in with Codex (ChatGPT) in the system browser, or save an API key read from stdin.
    /// Claude Agent and Antigravity run their own CLI sign-in and keep their own login.
    Login {
        #[arg(value_enum)]
        provider: account::LoginTarget,
    },
    /// Revoke and remove artifactize's own Codex tokens, or forget a saved API key.
    /// Claude Agent and Antigravity run their own CLI logout.
    Logout {
        #[arg(value_enum)]
        provider: account::LoginTarget,
    },
    /// List, add or remove the custom providers an Agent profile's backend may name.
    Provider {
        #[command(subcommand)]
        command: provider::ProviderCommand,
    },
    /// Manage ACP Chat agents, official ACP Registry installs, and Workspace Trust.
    #[command(name = "chat-agent")]
    ChatAgent {
        #[command(subcommand)]
        command: chat_agent::ChatAgentCommand,
    },
    /// Add, remove, or list Artifact type plugins for this project.
    Plugin {
        #[command(subcommand)]
        command: plugin::PluginCommand,
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
        /// Removed in 0.16.0: a sign-off never holds up other evals. Refused while parsing, so a
        /// parsed command never carries it.
        #[arg(long, hide = true, num_args = 0, default_missing_value = "", value_parser = removed_defer_human)]
        defer_human: Option<Removed>,
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
    /// Serve or administer a shared remote review store (review-store.sqlite).
    Server {
        #[command(subcommand)]
        command: ServerCommand,
    },
}

#[derive(Debug, Args)]
pub struct PolicyArgs {
    /// Run every included Agent eval with this declared profile instead of its own.
    #[arg(long, value_name = "NAME")]
    profile: Option<artifactize_core::config::ProfileName>,
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
use argv::{Invocation, ParseError, Parsed, parse_args};

/// Whole seconds as a duration, read at the command line.
fn seconds(text: &str) -> Result<std::time::Duration, String> {
    text.parse::<u64>()
        .map(std::time::Duration::from_secs)
        .map_err(|error| error.to_string())
}
