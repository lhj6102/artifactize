//! Review-owned tool sessions. A detached supervisor holds each process group and
//! runs stop even when its review future is dropped or its token is cancelled.

use std::{collections::BTreeMap, ffi::OsString, io, path::Path, time::Duration};

use serde::{Deserialize, Serialize};
use tokio::{io::AsyncReadExt, sync::oneshot, task::JoinHandle, time::Instant};
use tokio_util::sync::{CancellationToken, DropGuard};

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
    ended: CancellationToken,
    stop: DropGuard,
    finished: JoinHandle<Result<(), String>>,
}

impl Sessions {
    pub(crate) fn environment(
        &self,
        name: &(ArtifactName, ToolOperationName),
    ) -> Option<&BTreeMap<OsString, OsString>> {
        let session = self.0.get(name)?;
        (!session.ended.is_cancelled()).then_some(&session.environment)
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
                let directory =
                    crate::task::joined(tokio::task::spawn_blocking(move || allocate(&root)).await)
                        .map_err(|error| error.to_string())?
                        .map_err(|error| error.to_string())?;
                let stop_token = CancellationToken::new();
                let guard = stop_token.clone().drop_guard();
                let ended = CancellationToken::new();
                let (send, receive) = oneshot::channel();
                let finished = tokio::spawn(supervise(
                    start,
                    stop,
                    workspace,
                    directory,
                    cancellation.clone(),
                    stop_token,
                    ended.clone(),
                    send,
                ));
                match receive.await.map_err(|error| error.to_string())? {
                    Ok(environment) => {
                        sessions.0.insert(
                            key,
                            Session {
                                environment,
                                ended,
                                stop: guard,
                                finished,
                            },
                        );
                    }
                    Err(error) => {
                        drop(guard);
                        let _ = crate::task::joined(finished.await);
                        return Err(format!("Tool session {name}: {error}"));
                    }
                }
            }
            Ok(())
        }
        .await;
        match result {
            Ok(()) => Ok(sessions),
            Err(error) => {
                let _ = sessions.close().await;
                Err(error)
            }
        }
    }
}

pub(super) fn session_key(
    name: &crate::config::ToolOperationName,
    owner: &crate::config::ArtifactName,
) -> (ArtifactName, ToolOperationName) {
    (owner.clone(), name.clone())
}

#[derive(Serialize, Deserialize)]
struct Owner {
    owner: process::ChildIdentity,
    leader: Option<process::ChildIdentity>,
}

fn allocate(root: &Path) -> io::Result<platform::PrivateTempDir> {
    platform::create_private_dir_all(root)?;
    let lock = platform::private_options()
        .read(true)
        .write(true)
        .create(true)
        .open(root.join("lock"))?;
    lock.lock()?;
    // The allocation lock makes an owner-less directory unambiguously abandoned.
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let path = entry.path();
        let record = match std::fs::read(path.join("owner.json")) {
            Ok(bytes) => Some(serde_json::from_slice::<Owner>(&bytes).map_err(io::Error::other)?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        if let Some(record) = record {
            if process::is_alive(Some(record.owner))? {
                continue;
            }
            if let Some(leader) = record.leader {
                platform::kill_saved_group(leader)?;
            }
        }
        match std::fs::remove_dir_all(path) {
            Ok(()) => {}
            // Another live session may finish between listing and reading its record.
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    let directory = platform::private_tempdir_in("session-", root)?;
    save_owner(directory.path(), None)?;
    Ok(directory)
}

fn save_owner(directory: &Path, leader: Option<process::ChildIdentity>) -> io::Result<()> {
    let owner = process::child_identity(std::process::id())?;
    let bytes = serde_json::to_vec(&Owner { owner, leader }).map_err(io::Error::other)?;
    // Registration happens before admission: after a crash every admitted process has
    // a durable identity, and an incomplete record never admits owner code.
    let path = directory.join("owner.next");
    let mut file = platform::private_options()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)?;
    std::io::Write::write_all(&mut file, &bytes)?;
    file.sync_all()?;
    std::fs::rename(path, directory.join("owner.json"))?;
    platform::sync_dir(directory)
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
    ended: CancellationToken,
    send: oneshot::Sender<Result<BTreeMap<OsString, OsString>, String>>,
) -> Result<(), String> {
    let result = supervise_inner(
        start,
        stop,
        &workspace,
        directory.path(),
        cancellation,
        stop_token,
        ended,
        send,
    )
    .await;
    let path = directory.keep();
    let cleaned = std::fs::remove_dir_all(path).map_err(|error| error.to_string());
    result.and(cleaned)
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
    ended: CancellationToken,
    send: oneshot::Sender<Result<BTreeMap<OsString, OsString>, String>>,
) -> Result<(), String> {
    let prepared = prepare(start, workspace, directory, None);
    let (command, input) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            let _ = send.send(Err(error.clone()));
            return Err(error);
        }
    };
    let environment = command.env.clone();
    let owner_path = directory.to_owned();
    let deadline = Instant::now() + LIFECYCLE_TIMEOUT;
    let child = process::spawn_owned(
        &command,
        true,
        cancellation.clone(),
        deadline,
        move |leader| async move { save_owner(&owner_path, Some(leader)) },
    )
    .await;
    let mut child = match child {
        Ok(child) => child,
        Err(error) => {
            let _ = send.send(Err(error.to_string()));
            return Err(error.to_string());
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
        status = child.wait_leader() => Err(format!("exited before readiness: {status:?}")),
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
    let started = ready.is_ok();
    let _ = send.send(ready.map(|()| environment.clone()));
    if started {
        tokio::select! {
            biased;
            status = child.wait_leader() => {
                ended.cancel();
                let _ = child.kill();
                let _ = status;
                stop_token.cancelled().await;
            },
            _ = stop_token.cancelled() => {},
        }
    }
    ended.cancel();
    // Stop ignores the review's cancelled token. Its own one-shot supervisor bounds
    // the command, then the session group is killed regardless of its outcome.
    if let Ok((command, input)) = prepare(stop, workspace, directory, Some(&environment)) {
        let _ = process::run_with_input_limit(
            command,
            Some(input),
            0,
            CancellationToken::new(),
            |_| async { Ok(()) },
        )
        .await;
    }
    let killed = child.kill().map_err(|error| error.to_string());
    let reaped = child.wait().await.map_err(|error| error.to_string());
    feed.abort();
    drain_stdout.abort();
    drain_stderr.abort();
    let _ = feed.await;
    let _ = drain_stdout.await;
    let _ = drain_stderr.await;
    killed.and(reaped.map(|_| ()))
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
