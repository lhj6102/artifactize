//! Runtime verdicts come only from ordinary command exit codes.
//! Commands are trusted, not sandboxed; detached descendants can escape group cleanup.

mod environment;

use std::{
    ffi::OsString,
    future::Future,
    io,
    path::{Path, PathBuf},
    time::Duration,
};

use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::process::{self, ChildIdentity, Output};

/// Stop undeclared runtime commands after thirty seconds so a stalled check
/// cannot occupy a verification job indefinitely; owners can declare a longer timeout.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
/// Share configuration and CLI's signed 32-bit millisecond protocol bound.
pub const MAX_TIMEOUT: Duration = Duration::from_millis(crate::config::validation::MAX_TIMEOUT_MS);

/// A single runtime invocation with private writable directories and filtered env.
/// The caller owns the run directory and retains its contents for receipts/pruning.
#[derive(Debug)]
pub struct Command {
    /// Resolved artifact cwd; defaults to the canonical workspace.
    pub cwd: PathBuf,
    command: process::Command,
    directory: PathBuf,
}

impl Command {
    /// Prepare literal argv. Only PATH and LANG are inherited from the host.
    /// Each invocation gets independent 0700 directories below external `run_dir`.
    pub fn prepare(
        program: OsString,
        args: Vec<OsString>,
        workspace: &Path,
        run_dir: &Path,
        timeout: Option<Duration>,
    ) -> Result<Self, Error> {
        let timeout = timeout.unwrap_or(DEFAULT_TIMEOUT);
        if timeout.is_zero() || timeout > MAX_TIMEOUT {
            return Err(Error::InvalidTimeout);
        }
        let (cwd, directory, env) = environment::prepare(workspace, run_dir)?;
        Ok(Self {
            command: process::Command {
                program,
                args,
                cwd: cwd.clone(),
                env,
                timeout,
            },
            cwd,
            directory,
        })
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn timeout(&self) -> Duration {
        self.command.timeout
    }

    /// Protocol callers validate raw stdout rather than cleaned review audit text.
    pub(crate) async fn output(
        mut self,
        input: Vec<u8>,
        cancellation: CancellationToken,
    ) -> Result<Output, process::Error> {
        self.command.cwd = self.cwd;
        process::run_with_input(self.command, Some(input), cancellation, |_| async {
            Ok(())
        })
        .await
    }

    /// Tool protocols use a larger bounded capture than runtime audit output.
    pub(crate) async fn tool_output(
        mut self,
        input: Vec<u8>,
        output_limit: usize,
        cancellation: CancellationToken,
    ) -> Result<Output, process::Error> {
        self.command.cwd = self.cwd;
        process::run_with_input_limit(
            self.command,
            Some(input),
            output_limit,
            cancellation,
            |_| async { Ok(()) },
        )
        .await
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    clap::ValueEnum,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[value(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Verdict {
    Green,
    Red,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Green => "GREEN",
            Self::Red => "RED",
        }
    }
}

impl std::fmt::Display for Verdict {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug)]
pub struct ReviewResult {
    pub verdict: Verdict,
    pub exit_code: i32,
    pub output: Output,
}

/// Operational errors carry no review verdict.
#[derive(Debug)]
pub enum Outcome {
    Completed(ReviewResult),
    OperationalError(Error),
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("timeoutMs must be between 1 and 2147483647")]
    InvalidTimeout,
    #[error("runtime output directory must be outside the review workspace")]
    OutputInsideWorkspace,
    #[error("runtime environment preparation failed: {0}")]
    Environment(#[from] io::Error),
    #[error(transparent)]
    Process(#[from] process::Error),
    #[error("runtime process terminated without a verdict (signal {signal:?})")]
    AbnormalExit { signal: Option<i32>, output: Output },
}

/// Execute prepared argv with one deadline covering registration and execution.
/// Process capture is raw and bounded; only runtime audit text is cleaned.
pub async fn execute<F, R>(
    mut command: Command,
    cancellation: CancellationToken,
    register: F,
) -> Outcome
where
    F: FnOnce(ChildIdentity) -> R + Send + 'static,
    R: Future<Output = io::Result<()>> + Send + 'static,
{
    command.command.cwd = command.cwd;
    match process::run(command.command, cancellation, register).await {
        Ok(mut output) => {
            output.stdout = clean_output(&output.stdout);
            output.stderr = clean_output(&output.stderr);
            match crate::platform::process_end(&output.status) {
                crate::platform::ProcessEnd::Exited(exit_code) => {
                    Outcome::Completed(ReviewResult {
                        verdict: if exit_code == 0 {
                            Verdict::Green
                        } else {
                            Verdict::Red
                        },
                        exit_code,
                        output,
                    })
                }
                crate::platform::ProcessEnd::Signaled(signal) => {
                    Outcome::OperationalError(Error::AbnormalExit { signal, output })
                }
            }
        }
        Err(error) => Outcome::OperationalError(Error::Process(error)),
    }
}

pub(crate) use artifactize_tools::result::clean_output;
