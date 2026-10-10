//! How the `help` built-in starts its program. A host with a process layer of its own passes
//! it as a [`Launcher`]; the standalone command uses [`Standalone`], which keeps the program
//! and its children in one process group or Job Object and gives it an explicit environment.

use std::{
    collections::BTreeMap, ffi::OsString, future::Future, path::PathBuf, pin::Pin, process::Stdio,
    time::Duration,
};

use tokio::io::{AsyncRead, AsyncReadExt};
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
    pub success: bool,
    /// The exit status as the operating system describes it.
    pub status: String,
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
        Box::pin(async move {
            let mut command = tokio::process::Command::new(&launch.program);
            command
                .args(&launch.args)
                .current_dir(&launch.cwd)
                .env_clear()
                .envs(passed_environment())
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);
            let mut tree = platform::spawn_tree(&mut command)
                .map_err(|error| LaunchError::Failed(error.to_string()))?;
            let stdout = tree.child.stdout.take().expect("piped stdout");
            let stderr = tree.child.stderr.take().expect("piped stderr");
            let limit = launch.output_limit;
            let execution = async {
                let (stdout, stderr, status) = tokio::try_join!(
                    capture(stdout, limit),
                    capture(stderr, limit),
                    tree.child.wait()
                )
                .map_err(|error| LaunchError::Failed(error.to_string()))?;
                Ok(Finished {
                    success: status.success(),
                    status: status.to_string(),
                    truncated: stdout.len() > limit || stderr.len() > limit,
                    stdout,
                    stderr,
                })
            };
            let result = tokio::select! {
                _ = cancellation.cancelled() => Err(LaunchError::Cancelled),
                result = tokio::time::timeout(launch.timeout, execution) => {
                    result.unwrap_or(Err(LaunchError::TimedOut))
                }
            };
            tree.kill();
            result
        })
    }
}

/// Read at most one byte past `limit`, enough to tell that a stream was too long.
async fn capture(reader: impl AsyncRead + Unpin, limit: usize) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .await?;
    Ok(bytes)
}

/// The variables a standalone program receives: `PATH`, the locale, the home directory and
/// the system variables the platform's programs need to start; nothing else is inherited.
pub fn passed_environment() -> BTreeMap<OsString, OsString> {
    ["PATH", "LANG", "LC_ALL"]
        .into_iter()
        .chain(platform::SYSTEM_VARIABLES.iter().copied())
        .filter_map(|name| std::env::var_os(name).map(|value| (name.into(), value)))
        .collect()
}
