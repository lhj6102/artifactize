//! Command-line parsing, projections, and exit codes.

mod cache;
#[cfg(test)]
mod defaults_tests;
mod dispatch;
#[cfg(test)]
mod identity_tests;
mod interactive;
mod maintenance;
#[cfg(test)]
mod monitor_tests;
mod remote;
mod render;
mod run;
mod tools;
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

use clap::{
    Args, Parser, Subcommand,
    error::{ContextKind, ErrorKind},
};
use serde_json::json;

use artifactize_core::{
    config::ProfileKind,
    project::selection::{ProfileSelection, Selection, read_selection_file},
    types::{RequestId, ReuseKey, RunId},
};

/// Keep default Run history output readable; --limit/--offset expose older records.
const DEFAULT_RUN_LIST_LIMIT: u32 = 50;

/// Shells conventionally report an interrupt as 130; use the same status on every platform.
const INTERRUPTED_EXIT: u8 = 130;

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
        #[arg(long, value_name = "DURATION", value_parser = artifactize_core::store::prune::parse_duration)]
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
        #[arg(
            long,
            value_name = "N",
            value_parser = clap::value_parser!(u32).range(1..)
        )]
        jobs: Option<u32>,
        /// Limit executor starts in this Run; cache hits and waiters are free.
        #[arg(long, value_name = "N")]
        max_executions: Option<u64>,
        /// Observe until the Run ends; interrupting stops watching, not execution.
        #[arg(long)]
        wait: bool,
        /// Submit and stream JSON-line updates, eval results and final validation.
        #[arg(long, conflicts_with = "wait")]
        watch: bool,
        /// Retry-safe caller attempt key (1–128 ASCII letters/digits/._:-).
        #[arg(long, value_name = "KEY")]
        request_key: Option<artifactize_core::types::RequestKey>,
        /// Only reuse results for these kinds (comma-separated); an eval with nothing to reuse is
        /// not executed.
        #[arg(long, value_name = "KINDS", value_enum, value_delimiter = ',')]
        reuse_only: Vec<ProfileKind>,
        /// Start reviews before unresolved dependency gates; cancel blocked work.
        #[arg(long)]
        speculate: bool,
        /// Run past pending sign-offs and finish with SIGNOFFS_PENDING (exit 5).
        #[arg(long)]
        defer_human: bool,
    },
    /// Inspect current validation, saved Run audit, and what verify would do.
    Status {
        #[command(flatten)]
        selection: SelectionArgs,
        #[command(flatten)]
        policy: PolicyArgs,
    },
    /// Print the current fingerprint of each named Artifact, one `NAME FINGERPRINT` line each.
    Fingerprint {
        /// Artifact names, printed in this order.
        #[arg(value_name = "ARTIFACT", required = true)]
        artifacts: Vec<artifactize_core::types::ArtifactName>,
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
        /// Reviewer name (defaults to USER).
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
                eval_ids: selection_file::<artifactize_core::types::EvalId>(&path)?,
            })
        } else if let Some(path) = self.artifacts_file {
            Ok(Selection::Artifacts {
                artifact_ids: selection_file::<artifactize_core::types::ArtifactName>(&path)?,
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
    profile: Option<artifactize_core::config::ProfileVariantName>,
    /// Include all Evals in the required dependency scope, including cycle peers.
    #[arg(long)]
    recursive: bool,
    /// Force explicitly selected Evals only; dependency gates still apply.
    #[arg(long)]
    force: bool,
    /// Fingerprints to compute at once (default: the available CPUs).
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..))]
    fingerprint_jobs: Option<u32>,
}

impl PolicyArgs {
    fn options(self) -> artifactize_core::project::VerifyOptions {
        artifactize_core::project::VerifyOptions {
            profile: self.profile.map(ProfileSelection::Named),
            recursive: self.recursive,
            force: self.force,
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
    fn backend(self) -> artifactize_core::config::Backend {
        match self {
            Self::Openai => artifactize_core::config::Backend::Openai,
            Self::Anthropic => artifactize_core::config::Backend::Anthropic,
            Self::Codex => artifactize_core::config::Backend::Codex,
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
        artifact: Option<artifactize_core::types::ArtifactName>,
        /// Include downstream consumers and their required inputs (conservative impact).
        #[arg(long)]
        impact: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum RunCommand {
    /// Request cancellation; committed results are kept and cleanup settles asynchronously.
    Cancel { run_id: RunId },
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
    /// Stream live updates, saved eval results and final validation as JSON lines.
    Watch { run_id: RunId },
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
    Check(tools::ToolCheckArgs),
    #[command(flatten)]
    Builtin(artifactize_tools::cli::Tool),
}

fn cancellation_listener() -> Result<
    (
        tokio_util::sync::CancellationToken,
        tokio::task::JoinHandle<()>,
    ),
    String,
> {
    let cancellation = tokio_util::sync::CancellationToken::new();
    let stop = artifactize_core::platform::stop_requested().map_err(|e| e.to_string())?;
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

/// What a screen runs until the reviewer quits; screens need no `Send`, the command runs on a
/// single thread.
pub type Screen = std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>>>>;

/// The terminal screens `artifactize monitor` and `artifactize review` open. They live in
/// `artifactize-tui`; the binary wires these entry points without a dependency from core to CLI.
#[derive(Clone, Copy)]
pub struct Screens {
    /// The Monitor over the state directory, on one repository or on every one.
    pub monitor: fn(PathBuf, Option<PathBuf>, tokio_util::sync::CancellationToken) -> Screen,
    /// The Human review list, or one request when given.
    pub review: fn(
        PathBuf,
        Option<PathBuf>,
        artifactize_core::types::ReviewerId,
        Option<artifactize_core::types::RequestId>,
        tokio_util::sync::CancellationToken,
    ) -> Screen,
}

pub fn run(screens: Screens) -> ExitCode {
    let args: Vec<_> = std::env::args_os().collect();
    let has_args = args.len() > 1;
    let parsed = Cli::try_parse_from(&args);
    // The help tool's trailing arguments belong to the target program, even if they
    // happen to share names with artifactize's global flags.
    let tool_args = match &parsed {
        Ok(Cli {
            command:
                Some(Command::Tools {
                    command: ToolsCommand::Builtin(artifactize_tools::cli::Tool::Help { args }),
                }),
            ..
        }) => args.len(),
        _ => 0,
    };
    let options: Vec<_> = args
        .iter()
        .take(args.len() - tool_args)
        .skip(1)
        .take_while(|arg| *arg != "--")
        .filter_map(|arg| arg.to_str())
        .collect();
    let mcp = matches!(
        &parsed,
        Ok(Cli {
            command: Some(Command::Mcp),
            ..
        })
    ) || parsed
        .as_ref()
        .err()
        .and_then(|error| error.get(ContextKind::Usage))
        .is_some_and(|usage| usage.to_string().starts_with("Usage: artifactize mcp"));
    let json = options.contains(&"--json") && !mcp;
    if let Some(name) = repeated_global(&options) {
        return failure(&format!("{name} cannot be used multiple times."), json);
    }
    let cli = match parsed {
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
    // Run on a thread whose stack is the same size everywhere: Windows gives the main thread
    // only 1 MiB, which deep review futures outgrow in debug builds.
    let worker = std::thread::Builder::new()
        .name("artifactize".into())
        .stack_size(STACK_BYTES)
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .thread_stack_size(STACK_BYTES)
                .build()
                .map_err(|error| error.to_string())?;
            runtime.block_on(Box::pin(async {
                let result = dispatch::execute(cli, screens).await;
                artifactize_core::changes::drain().await;
                result
            }))
        });
    let result = match worker {
        Ok(worker) => worker
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic)),
        Err(error) => Err(error.to_string()),
    };
    match result {
        Ok(code) => match artifactize_core::platform::pause_after_help(has_args) {
            Ok(()) => ExitCode::from(code),
            Err(error) => failure(&error.to_string(), json),
        },
        Err(error) => failure(&error, json),
    }
}

/// The stack of the thread commands run on, and of blocking helper threads: 8 MiB, the
/// usual Linux and macOS main-thread size, so every system runs the same futures.
const STACK_BYTES: usize = 8 * 1024 * 1024;

/// Parse protocol milliseconds exactly once, preserving the public signed 32-bit bound.
fn timeout_duration(text: &str) -> Result<std::time::Duration, String> {
    let millis: u64 = text
        .parse()
        .map_err(|_| "timeout must be milliseconds".to_owned())?;
    if !(1..=artifactize_core::config::MAX_TIMEOUT_MS).contains(&millis) {
        return Err("timeoutMs must be between 1 and 2147483647".into());
    }
    Ok(std::time::Duration::from_millis(millis))
}

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
