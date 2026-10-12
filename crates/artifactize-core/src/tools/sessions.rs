//! Review-owned tool sessions. A detached supervisor holds each process group and
//! runs stop even when its review future is dropped or its token is cancelled.

use std::{collections::BTreeMap, ffi::OsString, io, path::Path, time::Duration};

use tokio::{io::AsyncReadExt, sync::oneshot, task::JoinHandle, time::Instant};
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

/// Readiness and stop each get thirty seconds, independently of the Agent budget;
/// a broken harness must not retain a backend slot or delay cleanup indefinitely.
const LIFECYCLE_TIMEOUT: Duration = Duration::from_secs(30);
/// Drain without retaining untrusted harness output or allocating for a whole line.
const DRAIN_BYTES: usize = 8192;

#[derive(Default)]
pub struct Sessions(BTreeMap<(ArtifactName, ToolOperationName), Session>);

struct Session {
    environment: BTreeMap<OsString, OsString>,
    owner: std::sync::Arc<Ownership>,
    stop: DropGuard,
    finished: JoinHandle<Result<(), String>>,
}

impl Sessions {
    pub(crate) fn call(&self, name: &(ArtifactName, ToolOperationName)) -> Option<ownership::Call> {
        let session = self.0.get(name)?;
        session.owner.call(&session.environment)
    }

    pub async fn close(self) -> Result<(), String> {
        let sessions: Vec<_> = self.0.into_values().collect();
        let finished: Vec<_> = sessions
            .into_iter()
            .map(|session| {
                drop(session.stop);
                session.finished
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
    /// Start only referenced sessions, or the selected diagnostic tool's session.
    /// Listing and static checks never call this function.
    pub async fn start_sessions(
        &self,
        state: &Path,
        selected: Option<&str>,
        cancellation: CancellationToken,
    ) -> Result<Sessions, String> {
        let required = self.tools.iter().any(|(name, tool)| {
            selected.is_none_or(|selected| name == selected)
                && matches!(tool.declaration, AgentTool::Command(command) if command.session.is_some())
        });
        if !required {
            return Ok(Sessions::default());
        }
        let root = workspace::prepare_directory(&state.join("tool-sessions"), &self.config.root)
            .map_err(|error| error.to_string())?;
        let mut sessions = Sessions::default();
        let result = async {
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
                if sessions.0.contains_key(&key) {
                    continue;
                }
                let declaration = &self.scope.artifacts[owner].views.agent_sessions[name];
                let start = self.prepare_session(owner, &declaration.start)?;
                let stop = self.prepare_session(owner, &declaration.stop)?;
                let workspace = self.config.root.clone();
                let root = root.clone();
                let (directory, lease) =
                    crate::task::joined(tokio::task::spawn_blocking(move || allocate(&root)).await)
                        .map_err(|error| error.to_string())?
                        .map_err(|error| error.to_string())?;
                let stop_token = CancellationToken::new();
                let guard = stop_token.clone().drop_guard();
                let owner = Ownership::new(directory.path(), lease);
                let (send, receive) = oneshot::channel();
                let finished = executor()
                    .map_err(|error| error.to_string())?
                    .spawn(supervise(
                        start,
                        stop,
                        workspace,
                        directory,
                        cancellation.clone(),
                        stop_token,
                        owner.clone(),
                        send,
                    ));
                match receive.await.map_err(|error| error.to_string())? {
                    Ok(environment) => {
                        sessions.0.insert(
                            key,
                            Session {
                                environment,
                                owner,
                                stop: guard,
                                finished,
                            },
                        );
                    }
                    Err(error) => {
                        drop(guard);
                        let cleaned = crate::task::joined(finished.await)
                            .map_err(|error| error.to_string())
                            .and_then(|result| result);
                        return Err(format!(
                            "Tool session {name}: {}",
                            cleanup_message(error, cleaned)
                        ));
                    }
                }
            }
            Ok(())
        }
        .await;
        match result {
            Ok(()) => Ok(sessions),
            Err(error) => Err(cleanup_message(error, sessions.close().await)),
        }
    }
}

fn cleanup_message(error: String, cleanup: Result<(), String>) -> String {
    match cleanup {
        Err(cleanup) if cleanup != error => format!("{error}; {cleanup}"),
        _ => error,
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
            let _ = send.send(Err(error.clone()));
            return Cleanup::error(error);
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
        move |leader| async move { admitted.register(leader).await },
    )
    .await;
    let mut child = match child {
        Ok(child) => child,
        Err(error) => {
            let message = error.to_string();
            let _ = send.send(Err(message.clone()));
            record.after_run::<()>(&Err(error));
            let mut cleanup = record.finish();
            cleanup.merge(Cleanup::error(message));
            return cleanup;
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
        result = tokio::time::timeout_at(deadline, ready_receive) => match result {
            Ok(Ok(Ok(()))) => match child.try_wait_leader() {
                Ok(None) => Ok(()),
                _ => Err("exited before readiness".into()),
            },
            Ok(Ok(Err(error))) => Err(error.to_string()),
            Ok(Err(error)) => Err(error.to_string()),
            Err(_) => Err("start timed out".into()),
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
    let _ = feed.await;
    let _ = drain_stdout.await;
    let _ = drain_stderr.await;
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
