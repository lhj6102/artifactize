//! Literal process launches, bounded capture, and process-group cleanup.

use std::{
    collections::BTreeMap,
    ffi::OsString,
    future::Future,
    io,
    path::PathBuf,
    process::{ExitStatus, Stdio},
    time::Duration,
};

use thiserror::Error;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    time::{Instant, sleep, sleep_until, timeout},
};
use tokio_util::sync::CancellationToken;

use crate::platform::{self, Child};

const OUTPUT_LIMIT: usize = 128 * 1024;
const CLEANUP_GRACE: Duration = Duration::from_secs(1);

/// Arguments have already been resolved by the caller. No shell is added.
#[derive(Debug, Clone)]
pub struct Command {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub cwd: PathBuf,
    /// The complete child environment, not additions to the parent's environment.
    pub env: BTreeMap<OsString, OsString>,
    pub timeout: Duration,
}

/// Process id and start time, captured while the group leader is still inert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChildIdentity {
    pub pid: u32,
    /// Linux: field 22 of /proc/PID/stat, in clock ticks since boot. Windows: the creation
    /// time, in 100 ns intervals since 1601.
    pub start_time: u64,
}

#[derive(Debug)]
pub struct Output {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub truncated: bool,
    pub duration: Duration,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("process could not be spawned: {0}")]
    Spawn(#[source] io::Error),
    #[error("child registration failed: {0}")]
    Registration(#[source] io::Error),
    #[error("process supervision failed: {0}")]
    Io(#[from] io::Error),
    #[error("process supervision task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
    #[error("process timed out")]
    Timeout,
    #[error("process was cancelled")]
    Cancelled,
    #[error("process output pipes did not close after cleanup")]
    OutputDidNotClose,
}

/// Register the inert child before allowing exec. A rejected, cancelled, or timed-out
/// registration cannot start user code. The callback must not block its executor.
/// Dropping this future cancels execution; its supervisor still kills and reaps it.
pub async fn run<F, R>(
    command: Command,
    cancellation: CancellationToken,
    register: F,
) -> Result<Output, Error>
where
    F: FnOnce(ChildIdentity) -> R + Send + 'static,
    R: Future<Output = io::Result<()>> + Send + 'static,
{
    run_with_input(command, None, cancellation, register).await
}

pub(crate) async fn run_with_input<F, R>(
    command: Command,
    input: Option<Vec<u8>>,
    cancellation: CancellationToken,
    register: F,
) -> Result<Output, Error>
where
    F: FnOnce(ChildIdentity) -> R + Send + 'static,
    R: Future<Output = io::Result<()>> + Send + 'static,
{
    run_with_input_limit(command, input, OUTPUT_LIMIT, cancellation, register).await
}

pub(crate) async fn run_with_input_limit<F, R>(
    command: Command,
    input: Option<Vec<u8>>,
    output_limit: usize,
    cancellation: CancellationToken,
    register: F,
) -> Result<Output, Error>
where
    F: FnOnce(ChildIdentity) -> R + Send + 'static,
    R: Future<Output = io::Result<()>> + Send + 'static,
{
    let cancellation = cancellation.child_token();
    let _cancel_on_drop = cancellation.clone().drop_guard();
    tokio::spawn(run_inner(
        command,
        input,
        output_limit,
        cancellation,
        register,
    ))
    .await?
}

async fn run_inner<F, R>(
    command: Command,
    input: Option<Vec<u8>>,
    output_limit: usize,
    cancellation: CancellationToken,
    register: F,
) -> Result<Output, Error>
where
    F: FnOnce(ChildIdentity) -> R,
    R: Future<Output = io::Result<()>>,
{
    let started = Instant::now();
    let deadline = started + command.timeout;
    if cancellation.is_cancelled() {
        return Err(Error::Cancelled);
    }
    if command.timeout.is_zero() {
        return Err(Error::Timeout);
    }

    let mut child = tokio::process::Command::new(&command.program);
    child
        .args(&command.args)
        .current_dir(&command.cwd)
        .env_clear()
        .envs(&command.env)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let (gate, mut spawning) = platform::spawn_gated(child)?;
    let admission = tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(Error::Cancelled),
        _ = sleep_until(deadline) => Err(Error::Timeout),
        result = async {
            let pid = gate.pid().await?;
            let child = child_identity(pid).map_err(Error::Registration)?;
            register(child).await.map_err(Error::Registration)?;
            Ok::<_, Error>(())
        } => result,
    };
    // Cancellation may have arrived during the callback's final poll.
    let admission = admission.and_then(|()| {
        if cancellation.is_cancelled() {
            Err(Error::Cancelled)
        } else if Instant::now() >= deadline {
            Err(Error::Timeout)
        } else {
            gate.admit().map_err(Error::Io)
        }
    });
    drop(gate);
    let spawned = (&mut spawning).await?;
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => {
            return Err(match admission {
                Err(error @ (Error::Registration(_) | Error::Cancelled | Error::Timeout)) => error,
                _ => Error::Spawn(error),
            });
        }
    };
    if let Err(error) = admission {
        terminate(&mut child).await?;
        return Err(error);
    }

    let stdout = child.stdout().take().expect("stdout is piped");
    let stderr = child.stderr().take().expect("stderr is piped");
    let stdin = child.stdin().take();
    let reaped = CancellationToken::new();
    let feed = async {
        if let (Some(mut stdin), Some(input)) = (stdin, input) {
            tokio::select! {
                _ = reaped.cancelled() => {},
                _ = cancellation.cancelled() => {},
                _ = sleep_until(deadline) => {},
                result = stdin.write_all(&input) => {
                    // Fingerprint scripts may intentionally ignore their context.
                    if let Err(error) = result && error.kind() != io::ErrorKind::BrokenPipe {
                        return Err(Error::Io(error));
                    }
                }
            }
        }
        Ok(())
    };
    let wait = async {
        let result = tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(Error::Cancelled),
            _ = sleep_until(deadline) => Err(Error::Timeout),
            status = child.wait() => status.map_err(Error::Io),
        };
        let cleanup = if result.is_err() {
            terminate(&mut child).await
        } else {
            // An ordinary main-process exit must not leave its descendants running.
            child.kill()
        };
        reaped.cancel();
        cleanup?;
        result
    };
    let capture = async {
        let read = async {
            tokio::try_join!(capture(stdout, output_limit), capture(stderr, output_limit))
        };
        tokio::pin!(read);
        tokio::select! {
            result = &mut read => result.map_err(Error::Io),
            _ = reaped.cancelled() => {
                timeout(CLEANUP_GRACE, read).await
                    .map_err(|_| Error::OutputDidNotClose)?
                    .map_err(Error::Io)
            }
        }
    };
    let (status, output, input) = tokio::join!(wait, capture, feed);
    let status = status?;
    input?;
    let ((stdout, stdout_truncated), (stderr, stderr_truncated)) = output?;
    Ok(Output {
        status,
        stdout,
        stderr,
        truncated: stdout_truncated || stderr_truncated,
        duration: started.elapsed(),
    })
}

/// Intentional desktop handoff: no owned process group, pipes, or kill-on-drop policy.
pub(crate) fn launch_detached(
    program: &std::ffi::OsStr,
    args: &[String],
    cwd: &std::path::Path,
) -> Result<(), Error> {
    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(false);
    platform::detach(&mut command);
    let mut child = command.spawn().map_err(Error::Spawn)?;
    tokio::spawn(async move {
        let _ = child.wait().await;
    });
    Ok(())
}

pub(crate) fn child_identity(pid: u32) -> io::Result<ChildIdentity> {
    let start_time = platform::process_start_time(pid)?;
    Ok(ChildIdentity { pid, start_time })
}

pub(crate) fn is_alive(owner: ChildIdentity) -> io::Result<bool> {
    match child_identity(owner.pid) {
        Ok(current) => Ok(current == owner),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

async fn terminate(child: &mut Child) -> io::Result<()> {
    let _ = child.interrupt();
    tokio::select! {
        _ = sleep(CLEANUP_GRACE) => {},
        _ = child.wait() => {},
    }
    child.kill()?;
    child.wait().await?;
    Ok(())
}

async fn capture(mut pipe: impl AsyncRead + Unpin, limit: usize) -> io::Result<(Vec<u8>, bool)> {
    let mut output = Vec::new();
    let mut buffer = [0; 8192];
    let mut truncated = false;
    loop {
        let count = pipe.read(&mut buffer).await?;
        if count == 0 {
            return Ok((output, truncated));
        }
        let retained = count.min(limit - output.len());
        output.extend_from_slice(&buffer[..retained]);
        truncated |= retained < count;
    }
}
