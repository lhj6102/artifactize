//! Review-owned tool sessions. A detached supervisor holds each process group and
//! runs stop even when its review future is dropped or its token is cancelled.

use std::{collections::BTreeMap, ffi::OsString, io, path::Path, time::Duration};

use tokio::{
    io::AsyncReadExt,
    sync::{Mutex, oneshot},
    task::JoinHandle,
    time::Instant,
};
use tokio_util::sync::{CancellationToken, DropGuard};

mod journal;
pub(super) mod ownership;
use journal::{Record, allocate};
pub(crate) use ownership::executor;
use ownership::{Cleanup, Ownership};

use super::{Invocation, Registry};
use crate::{
    config::{AgentTool, ArtifactName, ToolOperationName},
    platform, process, runtime, workspace,
};

/// Spawning a session's start and running its stop each get thirty seconds, so a
/// broken harness cannot delay cleanup indefinitely. Readiness has no fixed bound:
/// preparing a harness may take as long as it needs while its start lives, and the
/// review's cancellation or the start's exit ends the wait.
const LIFECYCLE_TIMEOUT: Duration = Duration::from_secs(30);
/// Drain without retaining untrusted harness output or allocating for a whole line.
const DRAIN_BYTES: usize = 8192;

#[derive(Default)]
pub struct Sessions(BTreeMap<(ArtifactName, ToolOperationName), Mutex<Session>>);

enum Session {
    Pending(Box<Start>),
    Starting {
        running: Running,
        ready: oneshot::Receiver<Result<BTreeMap<OsString, OsString>, String>>,
    },
    Ready {
        running: Running,
        environment: BTreeMap<OsString, OsString>,
    },
    Failed {
        running: Option<Running>,
        error: String,
    },
}

struct Start {
    commands: Result<(Invocation, Invocation), String>,
    workspace: std::path::PathBuf,
    state: std::path::PathBuf,
    cancellation: CancellationToken,
}

struct Running {
    owner: std::sync::Arc<Ownership>,
    stop: DropGuard,
    finished: JoinHandle<Result<(), String>>,
}

impl Start {
    async fn spawn(self) -> Result<Session, String> {
        let (start, stop) = self.commands?;
        let root = workspace::prepare_directory(&self.state.join("tool-sessions"), &self.workspace)
            .map_err(|error| error.to_string())?;
        let (directory, lease) =
            crate::task::joined(tokio::task::spawn_blocking(move || allocate(&root)).await)
                .map_err(|error| error.to_string())?
                .map_err(|error| error.to_string())?;
        let stop_token = CancellationToken::new();
        let guard = stop_token.clone().drop_guard();
        let owner = Ownership::new(directory.path(), lease);
        let (send, ready) = oneshot::channel();
        let finished = executor()
            .map_err(|error| error.to_string())?
            .spawn(supervise(
                start,
                stop,
                self.workspace,
                directory,
                self.cancellation,
                stop_token,
                owner.clone(),
                send,
            ));
        Ok(Session::Starting {
            running: Running {
                owner,
                stop: guard,
                finished,
            },
            ready,
        })
    }
}

impl Session {
    fn failed(error: String) -> Self {
        Self::Failed {
            running: None,
            error,
        }
    }

    fn running(self) -> Option<Running> {
        match self {
            Self::Pending(_) => None,
            Self::Starting { running, .. } | Self::Ready { running, .. } => Some(running),
            Self::Failed { running, .. } => running,
        }
    }
}

impl Sessions {
    pub(crate) async fn call(
        &self,
        name: &(ArtifactName, ToolOperationName),
    ) -> Result<ownership::Call, String> {
        let ended = || format!("The tool session `{}` ended.", name.1);
        let session = self.0.get(name).ok_or_else(ended)?;
        // Keep startup and readiness serialized, including if tools later allow parallel calls.
        let mut session = session.lock().await;
        if matches!(*session, Session::Pending(_)) {
            let Session::Pending(start) =
                std::mem::replace(&mut *session, Session::failed("start was dropped".into()))
            else {
                unreachable!()
            };
            *session = start.spawn().await.unwrap_or_else(Session::failed);
        }
        if let Session::Starting { ready, .. } = &mut *session {
            // Store the supervisor before waiting: a dropped call still leaves close
            // owning its stop guard and join handle, even before readiness.
            let ready = ready.await.unwrap_or_else(|error| Err(error.to_string()));
            let running =
                std::mem::replace(&mut *session, Session::failed("start was dropped".into()))
                    .running()
                    .expect("starting session owns its supervisor");
            *session = match ready {
                Ok(environment) => Session::Ready {
                    running,
                    environment,
                },
                Err(error) => Session::Failed {
                    running: Some(running),
                    error,
                },
            };
        }
        match &*session {
            Session::Ready {
                running,
                environment,
            } => running.owner.call(environment).ok_or_else(ended),
            Session::Failed { error, .. } => Err(format!("Tool session {}: {error}", name.1)),
            _ => unreachable!("session startup resolved"),
        }
    }

    pub async fn close(self) -> Result<(), String> {
        let finished: Vec<_> = self
            .0
            .into_values()
            .filter_map(|session| session.into_inner().running())
            .map(|running| {
                drop(running.stop);
                running.finished
            })
            .collect();
        let mut result = Ok(());
        for task in finished {
            let cleaned = crate::task::joined(task.await)
                .map_err(|error| error.to_string())
                .and_then(|result| result);
            if result.is_ok() {
                result = cleaned;
            }
        }
        result
    }
}

impl Registry<'_> {
    /// Register lazy sessions for admitted tools, or the selected diagnostic tool.
    /// No process or session directory is created until a bound call.
    pub fn sessions(
        &self,
        state: &Path,
        selected: Option<&str>,
        cancellation: CancellationToken,
    ) -> Sessions {
        let mut sessions = Sessions::default();
        for (tool_name, tool) in &self.tools {
            if selected.is_some_and(|selected| tool_name != selected) {
                continue;
            }
            let AgentTool::Command(tool_declaration) = tool.declaration else {
                continue;
            };
            let Some(name) = &tool_declaration.session else {
                continue;
            };
            let owner = &tool.definition.artifact_id;
            let key = session_key(name, owner);
            sessions.0.entry(key).or_insert_with(|| {
                let declaration = &self.scope.artifacts[owner].views.agent_sessions[name];
                let commands = self
                    .prepare_session(owner, &declaration.start)
                    .and_then(|start| {
                        self.prepare_session(owner, &declaration.stop)
                            .map(|stop| (start, stop))
                    });
                Mutex::new(Session::Pending(Box::new(Start {
                    commands,
                    workspace: self.config.root.clone(),
                    state: state.to_owned(),
                    cancellation: cancellation.clone(),
                })))
            });
        }
        sessions
    }
}

pub(super) fn session_key(
    name: &crate::config::ToolOperationName,
    owner: &crate::config::ArtifactName,
) -> (ArtifactName, ToolOperationName) {
    (owner.clone(), name.clone())
}

#[expect(
    clippy::too_many_arguments,
    reason = "a supervisor owns its two commands, directory, independent stop and readiness channels"
)]
async fn supervise(
    start: Invocation,
    stop: Invocation,
    workspace: std::path::PathBuf,
    directory: platform::PrivateTempDir,
    cancellation: CancellationToken,
    stop_token: CancellationToken,
    owner: std::sync::Arc<Ownership>,
    send: oneshot::Sender<Result<BTreeMap<OsString, OsString>, String>>,
) -> Result<(), String> {
    // Once a supervisor may admit a process, only confirmed cleanup may delete
    // its journal. Panic or an unexpected return must not invoke TempDir::drop.
    let path = directory.keep();
    let mut cleanup = supervise_inner(
        start,
        stop,
        &workspace,
        &path,
        cancellation,
        stop_token,
        owner,
        send,
    )
    .await;
    if cleanup.confirmed()
        && let Err(error) = std::fs::remove_dir_all(&path)
        && error.kind() != io::ErrorKind::NotFound
    {
        cleanup.merge(Cleanup::error(error));
    }
    cleanup.result()
}

#[expect(
    clippy::too_many_arguments,
    reason = "the supervisor's command, lifecycle and readiness state"
)]
async fn supervise_inner(
    start: Invocation,
    stop: Invocation,
    workspace: &Path,
    directory: &Path,
    cancellation: CancellationToken,
    stop_token: CancellationToken,
    owner: std::sync::Arc<Ownership>,
    send: oneshot::Sender<Result<BTreeMap<OsString, OsString>, String>>,
) -> Cleanup {
    let prepared = prepare(start, workspace, directory, None);
    let (command, input) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            let _ = send.send(Err(error));
            return Cleanup::default();
        }
    };
    let environment = command.env.clone();
    let record = Record::start(directory);
    let admitted = record.clone();
    let deadline = Instant::now() + LIFECYCLE_TIMEOUT;
    let child = process::spawn_owned(
        &command,
        true,
        cancellation.clone(),
        deadline,
        Some(move |leader| async move { admitted.register(leader).await }),
    )
    .await;
    let mut child = match child {
        Ok(child) => child,
        Err(error) => {
            let _ = send.send(Err(error.to_string()));
            record.after_run::<()>(&Err(error));
            return record.finish();
        }
    };
    let mut stdout = child.stdout().take().expect("stdout is piped");
    let mut stderr = child.stderr().take().expect("stderr is piped");
    let mut stdin = child.stdin().take().expect("stdin is piped");
    let (ready_send, ready_receive) = oneshot::channel();
    let drain_stdout = tokio::spawn(async move {
        let ready = async {
            let mut buffer = [0; DRAIN_BYTES];
            loop {
                let count = stdout.read(&mut buffer).await?;
                if count == 0 {
                    return Err(io::Error::other("exited before readiness"));
                }
                if buffer[..count].contains(&b'\n') {
                    return Ok(());
                }
            }
        }
        .await;
        let _ = ready_send.send(ready);
        let _ = tokio::io::copy(&mut stdout, &mut tokio::io::sink()).await;
    });
    let drain_stderr = tokio::spawn(async move {
        let _ = tokio::io::copy(&mut stderr, &mut tokio::io::sink()).await;
    });
    let feed = tokio::spawn(async move {
        let result = tokio::io::AsyncWriteExt::write_all(&mut stdin, &input).await;
        drop(stdin);
        result
    });
    let ready = tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err("start was cancelled".to_owned()),
        _ = stop_token.cancelled() => Err("start was dropped".to_owned()),
        status = child.wait_leader() => Err(match status {
            Ok(status) => format!("exited before readiness: {}", platform::exit_description(&status)),
            Err(error) => format!("could not wait for the start command: {error}"),
        }),
        result = ready_receive => match result {
            Ok(Ok(())) => match child.try_wait_leader() {
                Ok(None) => Ok(()),
                _ => Err("exited before readiness".into()),
            },
            Ok(Err(error)) => Err(error.to_string()),
            Err(error) => Err(error.to_string()),
        }
    };
    let mut child = Some(child);
    let mut cleanup = Cleanup::default();
    let started = ready.is_ok();
    let _ = send.send(ready.map(|()| environment.clone()));
    if started {
        tokio::select! {
            biased;
            status = child.as_mut().expect("owned start").wait_leader() => {
                owner.ended.cancel();
                match child.as_mut().expect("owned start").cleanup().await {
                    Ok(_) => {
                        record.confirm();
                        drop(child.take());
                        cleanup.merge(record.finish());
                    },
                    Err(error) => cleanup.merge(Cleanup::error(error)),
                }
                let _ = status;
                stop_token.cancelled().await;
            },
            _ = stop_token.cancelled() => {},
        }
    }
    cleanup.merge(owner.finish_calls().await);
    // Stop ignores the review's cancelled token. Its own one-shot supervisor bounds
    // the command, then the session group is killed regardless of its outcome.
    if let Ok((command, input)) = prepare(stop, workspace, directory, Some(&environment)) {
        let stopped = Record::new(directory);
        let admitted = stopped.clone();
        let result = process::run_with_input_limit(
            command,
            Some(input),
            0,
            CancellationToken::new(),
            move |child| async move { admitted.register(child).await },
        )
        .await;
        stopped.after_run(&result);
        cleanup.merge(stopped.finish());
    }
    if let Some(mut child) = child {
        match child.cleanup().await {
            Ok(_) => record.confirm(),
            Err(error) => cleanup.merge(Cleanup::error(error)),
        }
    }
    // A failed signal/wait leaves Record owned; finish marks cleanup incomplete
    // and keeps every journal even after this supervisor/lease is released.
    cleanup.merge(record.finish());
    feed.abort();
    drain_stdout.abort();
    drain_stderr.abort();
    // Aborted tasks end cancelled, which is expected; a panic in one continues here.
    let _ = crate::task::joined(feed.await);
    let _ = crate::task::joined(drain_stdout.await);
    let _ = crate::task::joined(drain_stderr.await);
    cleanup
}

fn prepare(
    mut invocation: Invocation,
    workspace: &Path,
    directory: &Path,
    environment: Option<&BTreeMap<OsString, OsString>>,
) -> Result<(process::Command, Vec<u8>), String> {
    let mut command = runtime::Command::prepare(
        invocation.program,
        invocation.argv,
        workspace,
        directory,
        Some(LIFECYCLE_TIMEOUT),
    )
    .map_err(|error| error.to_string())?;
    command.cwd = invocation.cwd;
    command.session_environment(directory);
    if let Some(environment) = environment {
        command.inherit_session(environment);
    }
    invocation.context.output_dir = Some(command.directory().join("output"));
    invocation.context.tmp_dir = Some(command.directory().join("tmp"));
    let input = serde_json::json!({"version": super::JSON_TOOL_INPUT_VERSION, "context": invocation.context, "args": {}}).to_string().into_bytes();
    Ok((command.into_process(), input))
}

/// Recover abandoned session journals through their existing identity-checked allocation sweep.
pub(crate) fn recover(state: &Path) -> io::Result<()> {
    let root = state.join("tool-sessions");
    if root.is_dir() {
        let _ = allocate(&root)?;
    }
    Ok(())
}
