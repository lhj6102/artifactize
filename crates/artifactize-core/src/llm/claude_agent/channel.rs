//! A running Claude Code and the JSON lines between it and artifactize.

use std::{io, path::Path, time::Duration};

use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    task::JoinHandle,
    time::Instant,
};
use tokio_util::sync::CancellationToken;

use super::{
    command::{self, Launch},
    mcp::{self, Server},
    wire::{self, ControlResponse, Initialized, Line},
};
use crate::{
    agent::error::{Code, Failure},
    platform::{self, environment},
    process,
};

/// One line holds at most one message, tool result or verdict: artifactize's own tool
/// results stay below 16 MiB, so a longer line is not Claude Code's.
const LINE_LIMIT: u64 = 32 * 1024 * 1024;
/// Keep the end of Claude Code's stderr for an error; it rarely writes more.
const STDERR_LIMIT: usize = 16 * 1024;
/// Claude Code exits on its own once its input closes; past this it is stopped.
const STOP_GRACE: Duration = Duration::from_secs(2);

/// The lines between artifactize and Claude Code: `reader` is Claude Code's output, `writer`
/// its input.
pub struct Channel<R, W> {
    reader: BufReader<R>,
    writer: Option<W>,
}

impl<R: AsyncRead + Unpin, W: AsyncWrite + Unpin> Channel<R, W> {
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            reader: BufReader::new(reader),
            writer: Some(writer),
        }
    }

    pub async fn send(&mut self, line: &Value) -> Result<(), Failure> {
        let writer = self
            .writer
            .as_mut()
            .expect("input is closed only at the end");
        let mut bytes = line.to_string().into_bytes();
        bytes.push(b'\n');
        let written = async {
            writer.write_all(&bytes).await?;
            writer.flush().await
        };
        written
            .await
            .map_err(|error| failure(format!("Claude Code stopped reading its input: {error}.")))
    }

    /// The next line, or `None` once Claude Code's output ended.
    pub async fn next(&mut self) -> Result<Option<Line>, Failure> {
        loop {
            let mut bytes = Vec::new();
            let read = (&mut self.reader)
                .take(LINE_LIMIT + 1)
                .read_until(b'\n', &mut bytes)
                .await
                .map_err(|error| failure(format!("Cannot read Claude Code's output: {error}.")))?;
            if read == 0 {
                return Ok(None);
            }
            if bytes.len() as u64 > LINE_LIMIT {
                return Err(failure("Claude Code wrote a line over 32 MiB.".into()));
            }
            if bytes.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            return serde_json::from_slice(&bytes).map(Some).map_err(|error| {
                failure(format!(
                    "Claude Code wrote a line artifactize cannot read: {error}."
                ))
            });
        }
    }

    /// Close Claude Code's input, after which it finishes and exits.
    pub fn close(&mut self) {
        self.writer = None;
    }

    /// Start the session: answer the MCP server's setup, then return what `initialize`
    /// reported. `server` is the MCP server of a review, `None` for a probe.
    pub async fn initialize(&mut self, server: Option<&Server>) -> Result<Initialized, Failure> {
        self.send(&wire::initialize(server.map(|_| command::SERVER)))
            .await?;
        loop {
            match self.next().await? {
                None => return Err(failure("Claude Code exited while starting.".into())),
                Some(Line::ControlRequest {
                    request_id,
                    request,
                }) => {
                    let reply = match (server, request) {
                        (Some(server), wire::ControlRequest::McpMessage { message, .. }) => {
                            match server.answer(message) {
                                mcp::Answer::Reply(reply) => wire::mcp_reply(&request_id, reply),
                                mcp::Answer::Call { id, .. } => wire::mcp_reply(
                                    &request_id,
                                    mcp::call_error(&id, "The review has not started."),
                                ),
                            }
                        }
                        _ => wire::refusal(&request_id, "Not served by artifactize."),
                    };
                    self.send(&reply).await?;
                }
                Some(Line::ControlResponse {
                    response: ControlResponse::Success { response, .. },
                }) => {
                    let initialized = response.unwrap_or_default();
                    check_version(initialized.claude_code_version.as_deref())?;
                    return Ok(initialized);
                }
                Some(Line::ControlResponse {
                    response: ControlResponse::Error { error, .. },
                }) => {
                    return Err(failure(format!(
                        "Claude Code refused to start the session: {}",
                        super::shown(&error)
                    )));
                }
                Some(_) => {}
            }
        }
    }
}

/// Older Claude Code ignores verbatim prompts and would add its own context to the review's.
pub const MINIMUM_VERSION: semver::Version = semver::Version::new(2, 1, 248);

fn check_version(version: Option<&str>) -> Result<(), Failure> {
    let parsed = version.and_then(|text| semver::Version::parse(text).ok());
    match parsed {
        Some(version) if version >= MINIMUM_VERSION => Ok(()),
        _ => Err(Failure::new(
            Code::AgentError,
            format!(
                "Claude Code {} is older than {MINIMUM_VERSION}, which the claude-agent backend needs; run `claude update`.",
                version.unwrap_or("of an unknown version")
            ),
        )),
    }
}

fn failure(message: String) -> Failure {
    Failure::new(Code::ProviderError, message)
}

/// A started Claude Code: its process, owned with its children, and its stderr.
pub struct Running {
    child: platform::Child,
    stderr: JoinHandle<Vec<u8>>,
}

pub type ProcessChannel = Channel<tokio::process::ChildStdout, tokio::process::ChildStdin>;

/// Start Claude Code in `cwd`, which holds nothing of the review: Claude Code reads files
/// only through the offered tools.
pub async fn start(
    launch: &Launch<'_>,
    cwd: &Path,
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<(ProcessChannel, Running), Failure> {
    let command = process::Command {
        program: command::PROGRAM.into(),
        args: launch.args(),
        cwd: cwd.to_owned(),
        env: command::environment(environment::for_command()),
        timeout: deadline.saturating_duration_since(Instant::now()),
    };
    let mut child = process::spawn_owned(
        &command,
        true,
        cancellation.clone(),
        deadline,
        None::<process::Unregistered>,
    )
    .await
    .map_err(|error| match error {
        process::Error::Spawn(error) if error.kind() == io::ErrorKind::NotFound => Failure::new(
            Code::Authentication,
            "Claude Code (`claude`) is not on PATH; install it and sign in with `claude auth login`.",
        ),
        error => Failure::new(
            Code::AgentError,
            format!("Cannot start Claude Code: {error}."),
        ),
    })?;
    let stdout = child.stdout().take().expect("stdout is piped");
    let stdin = child.stdin().take().expect("stdin is piped");
    let mut stderr = child.stderr().take().expect("stderr is piped");
    let stderr = tokio::spawn(async move {
        let mut kept = Vec::new();
        let mut buffer = [0; 4096];
        while let Ok(count) = stderr.read(&mut buffer).await {
            if count == 0 {
                break;
            }
            kept.extend_from_slice(&buffer[..count]);
            if kept.len() > STDERR_LIMIT {
                kept.drain(..kept.len() - STDERR_LIMIT);
            }
        }
        kept
    });
    Ok((Channel::new(stdout, stdin), Running { child, stderr }))
}

impl Running {
    /// Let Claude Code exit after its input closed, stop it past a grace, and return the end
    /// of what it wrote on stderr.
    pub async fn stop(mut self, mut channel: ProcessChannel) -> String {
        channel.close();
        drop(channel);
        let exited = tokio::time::timeout(STOP_GRACE, self.child.wait()).await;
        if !matches!(exited, Ok(Ok(_))) {
            let _ = process::terminate(&mut self.child).await;
        }
        let _ = self.child.cleanup().await;
        let stderr = crate::task::joined(self.stderr.await).unwrap_or_default();
        super::shown(&String::from_utf8_lossy(&stderr))
    }
}
