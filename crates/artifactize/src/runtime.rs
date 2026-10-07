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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Green,
    Red,
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
            match output.status.code() {
                Some(exit_code) => Outcome::Completed(ReviewResult {
                    verdict: if exit_code == 0 {
                        Verdict::Green
                    } else {
                        Verdict::Red
                    },
                    exit_code,
                    output,
                }),
                None => Outcome::OperationalError(Error::AbnormalExit {
                    signal: crate::platform::exit_signal(&output.status),
                    output,
                }),
            }
        }
        Err(error) => Outcome::OperationalError(Error::Process(error)),
    }
}

pub(crate) fn clean_output(bytes: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(bytes);
    let mut bytes = text.as_bytes();
    let mut clean = Vec::with_capacity(bytes.len());
    while let Some((&byte, tail)) = bytes.split_first() {
        if bytes.starts_with(b"\x1b[") {
            let mut end = 2;
            while bytes
                .get(end)
                .is_some_and(|byte| (0x30..=0x3f).contains(byte))
            {
                end += 1;
            }
            while bytes
                .get(end)
                .is_some_and(|byte| (0x20..=0x2f).contains(byte))
            {
                end += 1;
            }
            if bytes
                .get(end)
                .is_some_and(|byte| (0x40..=0x7e).contains(byte))
            {
                bytes = &bytes[end + 1..];
                continue;
            }
        }
        if !matches!(byte, 0..=8 | 11..=12 | 14..=31) {
            clean.push(byte);
        }
        bytes = tail;
    }
    clean
}
