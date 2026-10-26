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
    io::AsyncWriteExt,
    time::{Instant, sleep, sleep_until},
};
use tokio_util::sync::CancellationToken;

use crate::{
    platform::{self, Child},
    tools::human::log::{End, Lines, Run, Stream},
};

/// Retain bounded stdout/stderr audit per stream while continuing to drain the pipes.
pub(crate) const OUTPUT_LIMIT: usize = 128 * 1024;
/// Give interrupted children one second to exit cooperatively before killing/reaping them.
const CLEANUP_GRACE: Duration = Duration::from_secs(1);

mod capture;
pub use capture::{CaptureError, StreamFailure};
#[cfg(test)]
pub(crate) use capture::{OUTPUT_CLOSE_GRACE, slow_disk};

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

/// A process id, guaranteed nonzero: every OS reserves 0 for the kernel or an idle
/// process, never one artifactize spawns or supervises. `#[serde(transparent)]` keeps
/// the saved and wire representation the same plain integer.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct ProcessId(std::num::NonZeroU32);

impl ProcessId {
    /// `pid` is a real, just-observed OS process id, so it is never 0.
    pub fn new(pid: u32) -> Self {
        Self(std::num::NonZeroU32::new(pid).expect("a real process id is never 0"))
    }

    pub fn get(self) -> u32 {
        self.0.get()
    }
}

impl From<std::num::NonZeroU32> for ProcessId {
    fn from(pid: std::num::NonZeroU32) -> Self {
        Self(pid)
    }
}

impl std::fmt::Display for ProcessId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl rusqlite::types::ToSql for ProcessId {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::from(i64::from(self.0.get())))
    }
}

impl rusqlite::types::FromSql for ProcessId {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let pid = u32::try_from(value.as_i64()?)
            .map_err(|error| rusqlite::types::FromSqlError::Other(error.into()))?;
        std::num::NonZeroU32::new(pid).map(Self).ok_or_else(|| {
            rusqlite::types::FromSqlError::Other(
                "a saved owner_pid of 0 is not a process id".into(),
            )
        })
    }
}

/// Process id and start time, captured while the group leader is still inert.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ChildIdentity {
    pub pid: ProcessId,
    /// Linux: clock ticks since boot (procfs field 22). macOS: microseconds since the
    /// epoch (kernel birth timestamp). Windows: 100 ns intervals since 1601.
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

/// Run a program artifactize itself uses, such as git, found through the shared program
/// lookup, and return its standard output when it succeeds. `None` when it is missing or
/// fails.
pub(crate) fn tool_output<'a>(
    program: &str,
    args: impl IntoIterator<Item = &'a std::ffi::OsStr>,
) -> Option<Vec<u8>> {
    platform::program::tool_output(program, args)
}

/// Starts a built-in's program, such as `help`'s, through this process layer: in one process
/// group or Job Object from before it runs, cleaned up with its children, with exactly the
/// environment given here.
pub(crate) struct Launcher {
    pub env: BTreeMap<OsString, OsString>,
}

impl artifactize_tools::launch::Launcher for Launcher {
    fn run<'a>(
        &'a self,
        launch: artifactize_tools::launch::Launch,
        cancellation: &'a CancellationToken,
    ) -> artifactize_tools::launch::Running<'a> {
        use artifactize_tools::launch::{Finished, LaunchError};
        Box::pin(async move {
            let command = Command {
                program: launch.program.into(),
                args: launch.args,
                cwd: launch.cwd,
                env: self.env.clone(),
                timeout: launch.timeout,
            };
            match run_unregistered(command, None, launch.output_limit, cancellation.clone()).await {
                Ok(output) => Ok(Finished {
                    status: output.status,
                    stdout: output.stdout,
                    stderr: output.stderr,
                    truncated: output.truncated,
                }),
                Err(Error::Timeout) => Err(LaunchError::TimedOut),
                Err(Error::Cancelled) => Err(LaunchError::Cancelled),
                Err(error) => Err(LaunchError::Failed(
                    error
                        .argument_refusal()
                        .unwrap_or_else(|| error.to_string()),
                )),
            }
        })
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("process could not be spawned: {0}")]
    Spawn(#[source] io::Error),
    #[error("child registration failed: {0}")]
    Registration(#[source] io::Error),
    #[error("process supervision failed: {0}")]
    Io(#[from] io::Error),
    #[error("process cleanup could not be confirmed: {0}")]
    Cleanup(#[source] io::Error),
    #[error("process supervision task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
    #[error("process timed out")]
    Timeout,
    #[error("process was cancelled")]
    Cancelled,
    /// The process ended, but its output could not be captured to the end; the error keeps
    /// how it ended and the output read.
    #[error(transparent)]
    Capture(Box<CaptureError>),
}

impl Error {
    /// Every ordinary run error is returned only after cleanup or before admission.
    /// Cleanup/task failures are the exceptions: retain any admission record.
    pub(crate) fn cleanup_confirmed(&self) -> bool {
        match self {
            Self::Cleanup(_) | Self::Task(_) => false,
            Self::Spawn(_)
            | Self::Registration(_)
            | Self::Io(_)
            | Self::Timeout
            | Self::Cancelled
            | Self::Capture(_) => true,
        }
    }

    pub(crate) fn argument_refusal(&self) -> Option<String> {
        match self {
            Self::Spawn(error) if error.kind() == io::ErrorKind::InvalidInput => Some(format!(
                "Process arguments could not be passed safely: {error}"
            )),
            _ => None,
        }
    }
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
    crate::task::joined(
        tokio::spawn(run_inner(
            command,
            input,
            output_limit,
            Capture::default(),
            cancellation,
            Some(register),
        ))
        .await,
    )?
}

/// A registration nobody makes: the type of [`run_inner`]'s and [`spawn_owned`]'s absent
/// `register`.
type Unregistered = fn(ChildIdentity) -> std::future::Ready<io::Result<()>>;

/// Run a child nobody registers, such as a fingerprint script or a tool's protocol call: it
/// runs at once, in its own process group or Job Object, and is cleaned up with its children.
pub(crate) async fn run_unregistered(
    command: Command,
    input: Option<Vec<u8>>,
    output_limit: usize,
    cancellation: CancellationToken,
) -> Result<Output, Error> {
    let cancellation = cancellation.child_token();
    let _cancel_on_drop = cancellation.clone().drop_guard();
    crate::task::joined(
        tokio::spawn(run_inner(
            command,
            input,
            output_limit,
            Capture::default(),
            cancellation,
            None::<Unregistered>,
        ))
        .await,
    )?
}

/// Where a run's output goes besides its bounded capture: its log files, and the lines a
/// Tool log shows while it runs.
#[derive(Default)]
struct Capture {
    logs: Option<PathBuf>,
    lines: Option<(Lines, Lines)>,
}

/// Run a Human output tool, its output reaching `run`'s Tool log line by line as it arrives
/// as well as its bounded capture.
pub(crate) async fn run_watched(
    command: Command,
    cancellation: CancellationToken,
    run: &Run,
) -> Result<Output, Error> {
    let cancellation = cancellation.child_token();
    let _cancel_on_drop = cancellation.clone().drop_guard();
    let capture = Capture {
        logs: None,
        lines: Some((run.lines(Stream::Stdout), run.lines(Stream::Stderr))),
    };
    crate::task::joined(
        tokio::spawn(run_inner(
            command,
            None,
            OUTPUT_LIMIT,
            capture,
            cancellation,
            None::<Unregistered>,
        ))
        .await,
    )?
}

async fn run_inner<F, R>(
    command: Command,
    input: Option<Vec<u8>>,
    output_limit: usize,
    Capture { logs, lines }: Capture,
    cancellation: CancellationToken,
    register: Option<F>,
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

    let mut child = spawn_owned(
        &command,
        input.is_some(),
        cancellation.clone(),
        deadline,
        register,
    )
    .await?;

    let stdout = child.stdout().take().expect("stdout is piped");
    let stderr = child.stderr().take().expect("stderr is piped");
    let stdin = child.stdin().take();
    // Cancelled once the process group is gone: from then on only a process outside it can
    // hold the pipes.
    let gone = CancellationToken::new();
    let feed = async {
        if let (Some(mut stdin), Some(input)) = (stdin, input) {
            tokio::select! {
                _ = gone.cancelled() => {},
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
            child.cleanup().await.map(|_| ())
        };
        gone.cancel();
        cleanup.map_err(Error::Cleanup)?;
        result
    };
    let (stdout_lines, stderr_lines) = lines.unzip();
    let evidence = |name| logs.as_ref().map(|directory| directory.join(name));
    let streams = async {
        tokio::join!(
            capture::capture(
                stdout,
                output_limit,
                evidence("runtime.stdout"),
                stdout_lines,
                &gone
            ),
            capture::capture(
                stderr,
                output_limit,
                evidence("runtime.stderr"),
                stderr_lines,
                &gone
            )
        )
    };
    let (status, (stdout, stderr), input) = tokio::join!(wait, streams, feed);
    let status = status?;
    input?;
    capture::finish(status, stdout, stderr, started.elapsed())
}

/// Spawn shared by one-shot calls and long-lived tool sessions. With `register` the child is
/// held before exec until registered; the caller must own this future in a supervisor so
/// dropping a request cannot abandon admission. Without it the child runs at once: the hold
/// would protect nothing, and on Unix it costs a `fork`, which slows with this process's
/// memory.
pub(crate) async fn spawn_owned<F, R>(
    command: &Command,
    input: bool,
    cancellation: CancellationToken,
    deadline: Instant,
    register: Option<F>,
) -> Result<Child, Error>
where
    F: FnOnce(ChildIdentity) -> R,
    R: Future<Output = io::Result<()>>,
{
    let program = platform::program::resolve_with(&command.program, &command.cwd, |name| {
        environment_value(&command.env, name).map(ToOwned::to_owned)
    })
    .map_err(Error::Spawn)?;
    let mut child = tokio::process::Command::new(&program);
    child
        .args(&command.args)
        .current_dir(&command.cwd)
        .env_clear()
        .envs(&command.env)
        .stdin(if input { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let Some(register) = register else {
        return platform::spawn_now(child).await.map_err(Error::Spawn);
    };
    let (gate, mut spawning) = platform::spawn_gated(child).map_err(Error::Spawn)?;
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
    let spawned = crate::task::joined((&mut spawning).await)?;
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
        terminate(&mut child).await.map_err(Error::Cleanup)?;
        return Err(error);
    }

    Ok(child)
}

fn environment_value<'a>(
    environment: &'a BTreeMap<OsString, OsString>,
    name: &str,
) -> Option<&'a std::ffi::OsStr> {
    crate::platform::environment::lookup(environment, name)
}

/// Intentional desktop handoff: no owned process group, pipes, or kill-on-drop policy.
pub(crate) fn launch_detached(
    program: &std::ffi::OsStr,
    args: &[String],
    cwd: &std::path::Path,
) -> Result<(), Error> {
    let program = platform::program::resolve(program, cwd).map_err(Error::Spawn)?;
    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .env_clear()
        .envs(platform::environment::snapshot())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(false);
    let mut child = platform::spawn_detached(command).map_err(Error::Spawn)?;
    tokio::spawn(async move {
        let _ = child.wait().await;
    });
    Ok(())
}

/// A desktop handoff whose output a Tool log shows: like [`launch_detached`] it is never
/// owned, killed or waited on by the review, but its stdout and stderr are pipes, drained
/// line by line into `run` until the program closes them and exits, whoever still listens.
/// An unwatched run gets the plain handoff. A failed spawn hands the run back to be ended.
pub(crate) fn launch_watched(
    program: &std::ffi::OsStr,
    args: &[String],
    cwd: &std::path::Path,
    run: Run,
) -> Result<(), (Error, Run)> {
    if !run.watched() {
        return match launch_detached(program, args, cwd) {
            Ok(()) => {
                run.end(End::Launched);
                Ok(())
            }
            Err(error) => Err((error, run)),
        };
    }
    let program = match platform::program::resolve(program, cwd) {
        Ok(program) => program,
        Err(error) => return Err((Error::Spawn(error), run)),
    };
    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .env_clear()
        .envs(platform::environment::snapshot())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(false);
    let mut child = match platform::spawn_watched(command) {
        Ok(child) => child,
        Err(error) => return Err((Error::Spawn(error), run)),
    };
    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");
    tokio::spawn(async move {
        // Nothing is retained: the lines go to the log, the rest is discarded. Nobody owns
        // this program's group, so its pipes are read for as long as anything holds them.
        let held = CancellationToken::new();
        let (status, _, _) = tokio::join!(
            child.wait(),
            capture::capture(stdout, 0, None, Some(run.lines(Stream::Stdout)), &held),
            capture::capture(stderr, 0, None, Some(run.lines(Stream::Stderr)), &held),
        );
        run.end(match status {
            Ok(status) => End::exited(&status),
            Err(error) => End::Failed(error.to_string()),
        });
    });
    Ok(())
}

/// Resolve the installed CLI, with sibling build outputs for Rust library consumers/tests.
pub(crate) fn artifactize_executable() -> Result<std::path::PathBuf, String> {
    let current = std::env::current_exe().map_err(|e| e.to_string())?;
    for directory in current
        .parent()
        .into_iter()
        .flat_map(|parent| [Some(parent), parent.parent()])
        .flatten()
    {
        if let Some(candidate) = platform::program::candidates(&directory.join("artifactize"))
            .into_iter()
            .find(|path| path.is_file())
        {
            return Ok(candidate);
        }
    }
    platform::program::resolve(
        std::ffi::OsStr::new("artifactize"),
        &std::env::current_dir().map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

/// The internal supervisor and executor have no inherited requester stdio/lifetime.
#[derive(Clone, Copy)]
pub(crate) enum RunEntry {
    Supervisor,
    Executor,
}
pub(crate) fn spawn_run(
    program: &std::path::Path,
    id: &crate::types::RunId,
    repo: &std::path::Path,
    state: &std::path::Path,
    entry: RunEntry,
) -> Result<(ChildIdentity, tokio::process::Child), Error> {
    let mut options = platform::private_options();
    options.write(true);
    match entry {
        RunEntry::Supervisor => {
            options.create_new(true);
        }
        RunEntry::Executor => {
            options.append(true);
        }
    }
    let directory = state.join("runs").join(id.as_str());
    let stderr = options
        .open(directory.join("worker.stderr"))
        .map_err(Error::Spawn)?;
    let program = platform::program::resolve(program.as_os_str(), repo).map_err(Error::Spawn)?;
    let mut command = tokio::process::Command::new(program);
    command
        .arg("--repo")
        .arg(repo)
        .arg("--state-dir")
        .arg(state)
        .arg(match entry {
            RunEntry::Supervisor => "__verify",
            RunEntry::Executor => "__execute",
        })
        .arg(id.as_str())
        // Internal workers must not pin the repository after publishing a terminal Run.
        // Authored commands still use their declared repository cwd in `spawn_owned`.
        .current_dir(directory)
        .env_clear()
        .envs(platform::environment::snapshot())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr))
        .kill_on_drop(false);
    let child = platform::spawn_run(command).map_err(Error::Spawn)?;
    let identity = child_identity(child.id().expect("spawned worker has a PID"))
        .map_err(Error::Registration)?;
    Ok((identity, child))
}
pub(crate) fn launch_run(
    program: &std::path::Path,
    id: &crate::types::RunId,
    repo: &std::path::Path,
    state: &std::path::Path,
) -> Result<ChildIdentity, Error> {
    let (identity, mut child) = spawn_run(program, id, repo, state, RunEntry::Supervisor)?;
    tokio::spawn(async move {
        let _ = child.wait().await;
    });
    Ok(identity)
}

pub(crate) fn child_identity(pid: u32) -> io::Result<ChildIdentity> {
    let start_time = platform::process_start_time(pid)?;
    Ok(ChildIdentity {
        pid: ProcessId::new(pid),
        start_time,
    })
}

/// Whether `owner` is both present and still the process it was: absent (no local
/// owner, such as a result mirrored from a remote review store) is never alive.
pub(crate) fn is_alive(owner: Option<ChildIdentity>) -> io::Result<bool> {
    let Some(owner) = owner else {
        return Ok(false);
    };
    match platform::running_process_start_time(owner.pid.get()) {
        Ok(start_time) => Ok(start_time == owner.start_time),
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
    child.cleanup().await?;
    Ok(())
}

/// Runtime logs live with their Run and are pruned with it. Protocol callers retain
/// prefix-only capture: changing protocol bytes to head/tail would corrupt replies.
pub(crate) async fn run_logged<F, R>(
    command: Command,
    directory: PathBuf,
    cancellation: CancellationToken,
    register: F,
) -> Result<Output, Error>
where
    F: FnOnce(ChildIdentity) -> R + Send + 'static,
    R: Future<Output = io::Result<()>> + Send + 'static,
{
    let cancellation = cancellation.child_token();
    let _cancel_on_drop = cancellation.clone().drop_guard();
    crate::task::joined(
        tokio::spawn(run_inner(
            command,
            None,
            OUTPUT_LIMIT,
            Capture {
                logs: Some(directory),
                lines: None,
            },
            cancellation,
            Some(register),
        ))
        .await,
    )?
}
