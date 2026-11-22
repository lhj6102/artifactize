//! ACP transport to the user's installed Antigravity CLI. No Google HTTP or credential I/O.
pub mod bridge;
mod wire;

use std::{io, path::Path, time::Duration};

use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{ChildStdin, ChildStdout},
    time::Instant,
};
use tokio_util::sync::CancellationToken;

pub(crate) use wire::{Content, Event, StopReason};
use wire::{Incoming, Initialized, Session};
use wire::{McpEnv, McpServer, PromptResult, Update};

use crate::{
    agent::error::{Code, Failure},
    config::{ModelId, Provider},
    llm::{claude_agent::channel::Running, models::ListedModel},
    platform::{self, PrivateTempDir},
    process,
};

/// 1.5.1 opened Google account models to every ACP client.
const MINIMUM: semver::Version = semver::Version::new(1, 5, 1);
/// Metadata requests never send a prompt; a stalled installation or sign-in check ends quickly.
pub(crate) const PROBE_TIMEOUT: Duration = super::models::LIST_TIMEOUT;
/// One ACP line holds a single request or a streamed event, not an entire conversation.
const LINE_BYTES: u64 = 32 * 1024 * 1024;
/// Cancel is best effort, even if the child is no longer reading its input.
const CANCEL_GRACE: Duration = Duration::from_millis(250);

#[derive(Debug)]
pub(crate) enum Problem {
    Missing,
    TooOld { version: semver::Version },
    SignedOut,
    Quota,
    RateLimit,
    Failed(String),
}

impl Problem {
    pub(crate) fn reported(text: &str) -> Option<Self> {
        let text = text.to_ascii_lowercase();
        let contains = |markers: &[&str]| markers.iter().any(|marker| text.contains(marker));
        if contains(&[
            "authentication required",
            "auth_required",
            "unauthenticated",
            "not authenticated",
            "not logged in",
            "not signed in",
            "login required",
            "select an authentication method",
            "no auth method",
            "credentials not found",
        ]) {
            Some(Self::SignedOut)
        } else if contains(&[
            "quota_exhausted",
            "quota exhausted",
            "quota exceeded",
            "usage limit reached",
        ]) {
            Some(Self::Quota)
        } else if contains(&[
            "resource_exhausted",
            "rate_limit_exceeded",
            "rate limit",
            "error 429",
            "code 429",
        ]) || text.starts_with("429 ")
        {
            Some(Self::RateLimit)
        } else {
            None
        }
    }
}

impl From<Problem> for Failure {
    fn from(problem: Problem) -> Self {
        match problem {
            Problem::Missing => Self::new(
                Code::Authentication,
                "Antigravity CLI (`agy`) is not on PATH. Install it, then run `artifactize login antigravity`.",
            ),
            Problem::TooOld { version } => Self::new(
                Code::AgentError,
                format!(
                    "Antigravity CLI {version} is too old; update `agy` to {MINIMUM} or newer."
                ),
            ),
            Problem::SignedOut => Self::new(
                Code::Authentication,
                "Antigravity needs your Google sign-in. Run `artifactize login antigravity` or use Settings → Providers.",
            ),
            Problem::Quota => Self::new(
                Code::Quota,
                "Your Antigravity account reached its usage quota. Try again when your quota resets.",
            ),
            Problem::RateLimit => Self::new(
                Code::RateLimit,
                "Antigravity is rate limiting this account. Try again later.",
            ),
            Problem::Failed(message) => Self::new(Code::ProviderError, message),
        }
    }
}

pub(crate) fn eval_refusal() -> Failure {
    Failure::from(Provider::Antigravity.validate_eval().unwrap_err())
}

pub(crate) fn command(args: &[&str], cwd: &Path, deadline: Instant) -> process::Command {
    process::Command {
        program: "agy".into(),
        args: args.iter().map(Into::into).collect(),
        cwd: cwd.to_owned(),
        env: platform::environment::for_command(),
        timeout: deadline.saturating_duration_since(Instant::now()),
    }
}

pub(crate) fn start_problem(error: process::Error) -> Problem {
    if matches!(&error, process::Error::Spawn(e) if e.kind() == io::ErrorKind::NotFound) {
        Problem::Missing
    } else {
        Problem::Failed(format!("Cannot start Antigravity: {error}."))
    }
}

pub(crate) fn workspace() -> Result<PrivateTempDir, Problem> {
    platform::private_tempdir_in("artifactize-acp-", &platform::temporary_directory()).map_err(
        |error| {
            Problem::Failed(format!(
                "Cannot create the Antigravity turn directory: {error}."
            ))
        },
    )
}

pub(crate) async fn check_version(
    cwd: &Path,
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<(), Problem> {
    let output = process::run(
        command(&["--version"], cwd, deadline),
        cancellation.clone(),
        |_| async { Ok(()) },
    )
    .await
    .map_err(start_problem)?;
    let version = output
        .status
        .success()
        .then(|| platform::output_text(&output.stdout))
        .and_then(|text| {
            text.split_whitespace()
                .find_map(|word| semver::Version::parse(word.trim_start_matches('v')).ok())
        })
        .ok_or_else(|| {
            Problem::Failed("Cannot read the Antigravity CLI version (`agy --version`).".into())
        })?;
    if version < MINIMUM {
        return Err(Problem::TooOld { version });
    }
    Ok(())
}

pub(crate) struct Acp {
    running: Running,
    wire: Channel,
    directory: PrivateTempDir,
}

struct Channel {
    read: BufReader<ChildStdout>,
    write: ChildStdin,
    line: Vec<u8>,
    request: u64,
}

impl Acp {
    pub(crate) async fn start(
        deadline: Instant,
        cancel: &CancellationToken,
    ) -> Result<Self, Problem> {
        let directory = workspace()?;
        check_version(directory.path(), deadline, cancel).await?;
        // Native login stays in its usual place. A fresh, empty cwd keeps repo settings and
        // hooks out of ACP; callback denial and terminal sandboxing are not eval confinement.
        let command = command(
            &["acp", "--sandbox", "--include-default-customizations=false"],
            directory.path(),
            deadline,
        );
        let (read, write, running) = Running::spawn(&command, deadline, cancel)
            .await
            .map_err(start_problem)?;
        Ok(Self {
            running,
            wire: Channel {
                read: BufReader::new(read),
                write,
                line: Vec::new(),
                request: 0,
            },
            directory,
        })
    }

    pub(crate) async fn inspect(&mut self) -> Result<Vec<ListedModel>, Problem> {
        self.wire.initialize().await?;
        let session = self.wire.session(self.directory.path(), &[], None).await?;
        session
            .model_option()
            .map(|(_, _, models)| models)
            .map_err(|error| Problem::Failed(error.message))
    }

    pub(crate) async fn logout(&mut self) -> Result<(), Problem> {
        let initialized = self.wire.initialize().await?;
        if initialized.agent_capabilities.auth.logout.is_none() {
            return Err(Problem::Failed("This `agy` does not offer ACP sign-out. Open the Antigravity CLI and run `/logout` there.".into()));
        }
        let _: Value = self.wire.request("logout", json!({})).await?;
        Ok(())
    }

    pub(crate) async fn open(
        &mut self,
        model: &ModelId,
        server: &McpServer<'_>,
        system: &str,
    ) -> Result<(String, bool), Failure> {
        let initialized = self.wire.initialize().await?;
        let append = initialized
            .agent_capabilities
            .meta
            .antigravity
            .system_prompt
            .append;
        let session = self
            .wire
            .session(
                self.directory.path(),
                std::slice::from_ref(server),
                append.then_some(system),
            )
            .await?;
        let (id, current, models) = session.model_option()?;
        if !models.iter().any(|listed| &listed.slug == model) {
            return Err(Problem::Failed(format!("Antigravity does not list model {model:?} for this account. Choose a model from `artifactize models antigravity`." )).into());
        }
        if current != model.as_str() {
            let updated: SessionOptions = self
                .wire
                .request(
                    "session/set_config_option",
                    json!({"sessionId":session.session_id,"configId":id,"value":model}),
                )
                .await?;
            if !updated.config_options.iter().any(|option| matches!(option, wire::ConfigOption::Select {id: selected, current_value, ..} if selected == id && current_value == model.as_str())) {
                return Err(Problem::Failed("Antigravity did not confirm the selected model.".into()).into());
            }
        }
        Ok((session.session_id, append))
    }

    pub(crate) async fn prompt(&mut self, session: &str, text: &str) -> Result<u64, Failure> {
        Ok(self
            .wire
            .send_request(
                "session/prompt",
                json!({"sessionId":session,"prompt":[{"type":"text","text":text}]}),
            )
            .await?)
    }

    /// Polls the same read buffer across `select!`, so an interrupted read never loses bytes.
    pub(crate) async fn next(&mut self, request: u64) -> Result<PromptEvent, Failure> {
        Ok(match self.wire.poll(request, "session/prompt").await? {
            Response::Update(update) => PromptEvent::Update(update),
            Response::Result(result) => PromptEvent::Finished(decode(result)?),
        })
    }

    pub(crate) async fn cancel(&mut self, session: &str) {
        let _ = tokio::time::timeout(
            CANCEL_GRACE,
            self.wire.send(
                &json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":session}}),
            ),
        )
        .await;
    }

    pub(crate) async fn stop(self) -> Option<Problem> {
        let Self {
            running,
            wire,
            directory,
        } = self;
        drop(wire);
        let stderr = running.reap().await;
        drop(directory);
        Problem::reported(&platform::output_text(&stderr))
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionOptions {
    config_options: Vec<wire::ConfigOption>,
}

pub(crate) enum PromptEvent {
    Update(Update),
    Finished(PromptResult),
}

enum Response {
    Update(Update),
    Result(Value),
}

fn decode<T: DeserializeOwned>(value: Value) -> Result<T, Problem> {
    serde_json::from_value(value)
        .map_err(|_| Problem::Failed("Antigravity gave an invalid ACP response.".into()))
}

impl Channel {
    async fn initialize(&mut self) -> Result<Initialized, Problem> {
        let initialized: Initialized = self.request("initialize", wire::initialization()).await?;
        if initialized.protocol_version != wire::PROTOCOL {
            return Err(Problem::Failed(format!(
                "Antigravity offered unsupported ACP version {}.",
                initialized.protocol_version
            )));
        }
        Ok(initialized)
    }

    async fn session(
        &mut self,
        cwd: &Path,
        servers: &[McpServer<'_>],
        system: Option<&str>,
    ) -> Result<Session, Problem> {
        let mut params = json!({"cwd":cwd,"mcpServers":servers});
        if let Some(system) = system {
            params["_meta"] = json!({"antigravity":{"systemPrompt":{"append":system}}});
        }
        self.request("session/new", params).await
    }

    async fn send_request(&mut self, method: &str, params: Value) -> Result<u64, Problem> {
        self.request += 1;
        self.send(&json!({"jsonrpc":"2.0","id":self.request,"method":method,"params":params}))
            .await?;
        Ok(self.request)
    }

    async fn request<T: DeserializeOwned>(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<T, Problem> {
        let request = self.send_request(method, params).await?;
        loop {
            if let Response::Result(result) = self.poll(request, method).await? {
                return decode(result);
            }
        }
    }

    async fn send(&mut self, value: &Value) -> Result<(), Problem> {
        let mut line = value.to_string().into_bytes();
        line.push(b'\n');
        self.write
            .write_all(&line)
            .await
            .map_err(|_| Problem::Failed("Antigravity stopped reading ACP input.".into()))
    }

    async fn poll(&mut self, request: u64, method: &str) -> Result<Response, Problem> {
        loop {
            let remaining = LINE_BYTES.saturating_sub(self.line.len() as u64);
            let read = (&mut self.read)
                .take(remaining + 1)
                .read_until(b'\n', &mut self.line)
                .await
                .map_err(|_| Problem::Failed("Cannot read Antigravity ACP output.".into()))?;
            if read == 0 {
                return Err(Problem::Failed(format!(
                    "Antigravity exited while handling ACP {method}."
                )));
            }
            if self.line.len() as u64 > LINE_BYTES {
                return Err(Problem::Failed(
                    "Antigravity wrote an oversized ACP message.".into(),
                ));
            }
            let line = serde_json::from_slice(&self.line)
                .map_err(|_| Problem::Failed("Antigravity wrote an invalid ACP message.".into()));
            self.line.clear();
            match line? {
                Incoming::Request { id, method } => self.send(&wire::refusal(id, &method)).await?,
                Incoming::Update {
                    method: wire::SessionUpdate::Update,
                    params,
                } => return Ok(Response::Update(params)),
                Incoming::Result { id, result } if id == request => {
                    return Ok(Response::Result(result));
                }
                Incoming::Error { id, error } if id == request => {
                    return Err(error.into_problem(method));
                }
                _ => {}
            }
        }
    }
}
