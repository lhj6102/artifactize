//! Runtime verdicts come only from ordinary command exit codes.

use std::{future::Future, io, os::unix::process::ExitStatusExt};

use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::process::{self, ChildIdentity, Command, Output};

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
    #[error(transparent)]
    Process(#[from] process::Error),
    #[error("runtime process terminated without a verdict (signal {signal:?})")]
    AbnormalExit { signal: Option<i32>, output: Output },
}

/// Execute resolved argv in the caller's cwd and explicit environment.
pub async fn execute<F, R>(
    command: Command,
    cancellation: CancellationToken,
    register: F,
) -> Outcome
where
    F: FnOnce(ChildIdentity) -> R + Send + 'static,
    R: Future<Output = io::Result<()>> + Send + 'static,
{
    match process::run(command, cancellation, register).await {
        Ok(output) => match output.status.code() {
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
                signal: output.status.signal(),
                output,
            }),
        },
        Err(error) => Outcome::OperationalError(Error::Process(error)),
    }
}
