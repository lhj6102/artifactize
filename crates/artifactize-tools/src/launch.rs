//! How the `help` built-in starts its program. A host with a process layer of its own passes
//! it as a [`Launcher`]; the standalone command uses [`Standalone`], which keeps the program
//! and its children in one process group or Job Object and gives it an explicit environment.

use std::{
    collections::BTreeMap, ffi::OsString, future::Future, path::PathBuf, pin::Pin, time::Duration,
};

use tokio_util::sync::CancellationToken;

use crate::platform;

/// One resolved program to run: its path, its arguments and its working directory.
#[derive(Debug, Clone)]
pub struct Launch {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: PathBuf,
    /// How long the program may run before it and its children are killed.
    pub timeout: Duration,
    /// How many bytes of each output stream are kept; more sets `truncated`.
    pub output_limit: usize,
}

/// What a finished program produced.
#[derive(Debug)]
pub struct Finished {
    /// The actual exit status, not independently editable success and diagnostic fields.
    pub status: std::process::ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// An output stream exceeded `output_limit`.
    pub truncated: bool,
}

/// Why a program did not finish.
#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
    #[error("timed out")]
    TimedOut,
    #[error("Tool call was cancelled.")]
    Cancelled,
    #[error("{0}")]
    Failed(String),
}

/// The boxed future a [`Launcher`] returns, so that hosts can pass one as a trait object.
pub type Running<'a> = Pin<Box<dyn Future<Output = Result<Finished, LaunchError>> + Send + 'a>>;

/// Starts a built-in's program and waits for it.
pub trait Launcher: Sync {
    fn run<'a>(&'a self, launch: Launch, cancellation: &'a CancellationToken) -> Running<'a>;
}

/// The standalone command's launcher: the program runs in its own process group or Job
/// Object, which is killed when it finishes, times out or is cancelled, with only the
/// variables in [`passed_environment`].
pub struct Standalone;

impl Launcher for Standalone {
    fn run<'a>(&'a self, launch: Launch, cancellation: &'a CancellationToken) -> Running<'a> {
        Box::pin(platform::process::run(launch, cancellation))
    }
}

/// The variables a standalone program receives: `PATH`, the locale, and the home, temporary
/// and system variables the platform's programs need to start; nothing else is inherited.
pub fn passed_environment() -> BTreeMap<OsString, OsString> {
    platform::process::passed_environment()
}
