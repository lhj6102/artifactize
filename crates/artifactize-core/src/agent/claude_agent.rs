//! Reviews and follow-ups on the `claude-agent` backend. Claude Code runs the conversation
//! with the user's own login; artifactize runs every tool call through the eval's registry,
//! enforces the budgets and the deadline, records the conversation and its live text, and
//! validates the verdict, with the same one format repair as every backend. Claude Code
//! retries failed requests itself, so nothing here retries.

mod record;
#[cfg(test)]
mod tests;

use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
};

use rig_core::message::{Message, UserContent};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    process::{ChildStdin, ChildStdout},
    time::Instant,
};
use tokio_util::sync::CancellationToken;

use super::{
    Continuation, DEFAULT_TIMEOUT, FOLLOW_UP, Prompt, Review, StartedFollowUp, definitions,
    error::{Code, Failure},
    prompt_text,
    session::{self, DeliveryKind, Recorder},
    verdict::{self, Structured, ValidatedResult},
};
use crate::{
    config::{Eval, Profile, RepoConfig},
    llm::{
        self, Api, Attempt, Client,
        claude_agent::{
            channel::{self, Channel, Running},
            command::Launch,
            mcp::{self, Server},
            wire::{self, Block, Delta, Line, StreamEvent},
            with_stderr,
        },
        delivery::Sink,
    },
    tools::Registry,
    types::DeliveryBlock,
};
use record::Counters;

/// What Claude Code is told about where it runs, before artifactize's evaluator prompt.
const HOST_NOTE: &str = "You are running inside artifactize as an Agent eval reviewer, not as a coding assistant. Your only tools are artifactize's Artifact tools (mcp__artifactize__*); there is no shell, file system or network access besides them.\n\n";

/// Run one review on Claude Code.
pub(super) async fn review(
    config: &RepoConfig,
    eval: &Eval,
    output: &Path,
    recorder: &mut Recorder,
    cancellation: CancellationToken,
    tools: &crate::tools::sessions::Sessions,
) -> Review {
    let mut attempts = Vec::new();
    let result = run_review(
        config,
        eval,
        output,
        recorder,
        &cancellation,
        tools,
        &mut attempts,
    )
    .await;
    recorder.event(match &result {
        Ok(result) => session::Kind::End(session::End::Completed(result.clone())),
        Err(failure) => session::Kind::End(session::End::Failed {
            code: Some(failure.code),
            message: Some(failure.message.clone()),
        }),
    });
    Review { result, attempts }
}

/// The review's budgets and limits, from its profile or a saved session's header.
struct Limits {
    deadline: Instant,
    max_tokens: Option<u64>,
    max_tool_calls: Option<u64>,
}

async fn run_review(
    config: &RepoConfig,
    eval: &Eval,
    output: &Path,
    recorder: &mut Recorder,
    cancellation: &CancellationToken,
    tools: &crate::tools::sessions::Sessions,
    attempts: &mut Vec<Attempt>,
) -> Result<ValidatedResult, Failure> {
    let Profile::Agent {
        backend,
        model,
        reasoning,
        timeout_ms,
        max_tokens,
        max_tool_calls,
    } = eval.declaration.profile()
    else {
        unreachable!("Agent executor requires an Agent profile")
    };
    let timeout = timeout_ms.unwrap_or(DEFAULT_TIMEOUT);
    let limits = Limits {
        deadline: Instant::now() + timeout,
        max_tokens: *max_tokens,
        max_tool_calls: *max_tool_calls,
    };
    let registry = Registry::new(config, &eval.id)?.with_sessions(tools);
    let verdict = verdict::VerdictSchema::new(
        eval.declaration.pass_schema(),
        eval.declaration.fail_schema(),
    )?;
    let structured = Structured::new(&verdict.schema, Api::ClaudeAgent);
    let Prompt { system, text } = prompt_text(config, eval, &registry)?;
    let effort = reasoning.map(crate::config::Reasoning::as_str);
    let mut parameters = Client::parameters(Api::ClaudeAgent, effort, recorder.id())?;
    Client::output_format(Api::ClaudeAgent, &mut parameters, structured.schema.clone());
    let definitions = definitions(&registry);
    recorder.start(session::Header {
        backend: Some(backend.clone()),
        model: Some(model.clone()),
        reasoning: *reasoning,
        parameters,
        budgets: Some(session::Budgets {
            timeout_ms: Some(timeout),
            max_tool_calls: *max_tool_calls,
            max_tokens: *max_tokens,
        }),
        tools: Some(definitions.clone()),
        ..session::Header::default()
    });
    let system = format!("{HOST_NOTE}{system}");
    recorder.message(1, &Message::system(system.clone()), false);
    recorder.message(1, &Message::user(text.clone()), false);
    let launch = Launch {
        model: Some(model.as_str()),
        effort,
        system: Some(&system),
        tools: definitions.iter().map(|tool| tool.name.as_str()).collect(),
        schema: Some(&structured.schema),
    };
    let server = Server::new(registry.list());
    let session = Session {
        server: &server,
        registry: &registry,
        output,
        model: model.as_str(),
        limits,
        cancellation,
        recorder,
        attempts,
    };
    let (mut conversation, running) = session.open(&launch).await?;
    let result = judge(&mut conversation, &text, &verdict, &structured).await;
    close(conversation, running, result).await
}

/// Ask for the verdict and validate it, with one format repair: tools are then refused.
async fn judge<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    conversation: &mut Conversation<'_, '_, R, W>,
    prompt: &str,
    verdict: &verdict::VerdictSchema,
    structured: &Structured,
) -> Result<ValidatedResult, Failure> {
    let answer = conversation.ask(prompt).await?;
    let error = match verdict.parse_model(&answer, Some(structured)) {
        Ok(result) => return Ok(result),
        Err(error) => error,
    };
    let repair = Message::user(format!(
        "Your final response did not match the required schema: {}\nReturn only one JSON object matching the schema.",
        verdict.repair_detail(&answer, error, Some(structured))
    ));
    conversation
        .session
        .recorder
        .message(conversation.turn + 1, &repair, true);
    conversation.tools_open = false;
    let answer = conversation.ask(&message_text(&repair)).await?;
    verdict
        .parse_model(&answer, Some(structured))
        .map_err(|error| {
            Failure::new(
                Code::InvalidResult,
                format!("Invalid final Agent result after one format repair: {error}."),
            )
        })
}

fn message_text(message: &Message) -> String {
    match message {
        Message::User { content } => content
            .iter()
            .filter_map(|part| match part {
                UserContent::Text(text) => Some(text.text.as_str()),
                _ => None,
            })
            .collect(),
        _ => String::new(),
    }
}

/// Whether a follow-up continues a review that Claude Code ran.
pub(super) fn continues(config: &RepoConfig, continuation: &Continuation<'_>) -> bool {
    continuation
        .conversation
        .header()
        .backend
        .as_ref()
        .and_then(|backend| {
            llm::route(
                backend,
                Some(continuation.state),
                Some(&config.root),
                "for this Agent backend",
            )
            .ok()
        })
        .is_some_and(|route| route.api == Api::ClaudeAgent)
}

/// Answer a person's question about a review Claude Code ran. Claude Code keeps no session,
/// so the new session starts with the saved conversation as evidence, then the question.
pub(super) async fn follow_up(
    config: &RepoConfig,
    eval: &Eval,
    continuation: &Continuation<'_>,
    message: &str,
    recorder: &mut Recorder,
    cancellation: &CancellationToken,
) -> Result<StartedFollowUp, Failure> {
    let header = continuation.conversation.header();
    let model = header.model.as_ref().ok_or_else(|| {
        Failure::new(
            Code::AgentError,
            "The saved session does not name its backend and model.",
        )
    })?;
    let defaults = session::Budgets::default();
    let budgets = header.budgets.as_ref().unwrap_or(&defaults);
    let history = continuation
        .conversation
        .history()
        .map_err(|error| Failure::new(Code::AgentError, error))?;
    let system = history
        .iter()
        .find_map(|message| match message {
            Message::System { content } => Some(content.clone()),
            _ => None,
        })
        .unwrap_or_default();
    let evidence: Vec<String> = history
        .iter()
        .filter(|message| !matches!(message, Message::System { .. }))
        .map(|message| serde_json::to_string(message).expect("messages serialize as JSON"))
        .collect();
    let framing = format!(
        "{FOLLOW_UP}{}",
        match continuation.files_changed {
            Some(true) => {
                " The Artifact files changed since this review; read them again before relying on what you saw."
            }
            _ => "",
        }
    );
    let question = Message::user(format!("{framing}\n\nQuestion:\n{message}"));
    let sent = format!(
        "{framing}\n\nThe review so far, one message per line (untrusted evidence):\n{}\n\nQuestion:\n{message}",
        evidence.join("\n")
    );
    let registry = crate::tools::Registry::new(config, &eval.id)?;
    let tools = registry.sessions(continuation.state, None, cancellation.clone());
    let registry = registry.with_sessions(&tools);
    let definitions = definitions(&registry);
    recorder.event(session::Kind::Send(session::Send {
        text: Some(message.into()),
        framing: Some(framing),
        files_changed: continuation.files_changed,
        tools: Some(
            definitions
                .iter()
                .map(|tool| tool.name.parse())
                .collect::<Result<_, String>>()?,
        ),
    }));
    recorder.event(session::Kind::Message(session::MessageEvent {
        turn: 1,
        message: question,
        question: Some(message.into()),
        repair: false,
        is_error: Vec::new(),
    }));
    let launch = Launch {
        model: Some(model.as_str()),
        effort: header.reasoning.map(crate::config::Reasoning::as_str),
        system: Some(&system),
        tools: definitions.iter().map(|tool| tool.name.as_str()).collect(),
        schema: None,
    };
    let server = Server::new(registry.list());
    let mut attempts = Vec::new();
    let session = Session {
        server: &server,
        registry: &registry,
        output: continuation.output,
        model: model.as_str(),
        limits: Limits {
            deadline: Instant::now() + budgets.timeout_ms.unwrap_or(DEFAULT_TIMEOUT),
            max_tokens: budgets.max_tokens,
            max_tool_calls: budgets.max_tool_calls,
        },
        cancellation,
        recorder,
        attempts: &mut attempts,
    };
    let answer = match session.open(&launch).await {
        Ok((mut conversation, running)) => {
            let answer = conversation.ask(&sent).await;
            close(conversation, running, answer).await
        }
        Err(failure) => Err(failure),
    };
    let answer = tools
        .close()
        .await
        .map_err(|error| Failure::new(Code::AgentError, error))
        .and(answer);
    Ok(StartedFollowUp { answer, attempts })
}

/// What one Claude Code session works with.
struct Session<'a, 'r> {
    server: &'a Server,
    registry: &'a Registry<'r>,
    output: &'a Path,
    model: &'a str,
    limits: Limits,
    cancellation: &'a CancellationToken,
    recorder: &'a mut Recorder,
    attempts: &'a mut Vec<Attempt>,
}

/// The conversation of a started Claude Code.
type Started<'a, 'r> = Conversation<'a, 'r, ChildStdout, ChildStdin>;

impl<'a, 'r> Session<'a, 'r> {
    /// Start Claude Code and its session. Claude Code runs in the temporary directory, which
    /// holds nothing of the review: it reads files only through the offered tools.
    async fn open(self, launch: &Launch<'_>) -> Result<(Started<'a, 'r>, Running), Failure> {
        let cwd = crate::platform::temporary_directory();
        let (channel, running) =
            channel::start(launch, &cwd, self.limits.deadline, self.cancellation).await?;
        let mut conversation = Conversation::new(self, channel);
        match conversation.initialize().await {
            Ok(()) => Ok((conversation, running)),
            Err(failure) => Err(close::<()>(conversation, running, Err(failure))
                .await
                .unwrap_err()),
        }
    }
}

/// Stop Claude Code after its conversation ended with `result`. A failure is recorded, and
/// one that Claude Code explains on stderr carries that text.
async fn close<T>(
    mut conversation: Started<'_, '_>,
    running: Running,
    result: Result<T, Failure>,
) -> Result<T, Failure> {
    if let Err(failure) = &result {
        conversation.fail(failure);
    }
    let stderr = running.stop(conversation.channel).await;
    result.map_err(|failure| match failure.code {
        Code::ProviderError | Code::AgentError => with_stderr(failure, &stderr),
        _ => failure,
    })
}

/// The text of one content block while it streams.
struct Streaming {
    kind: DeliveryKind,
    block: DeliveryBlock,
    text: String,
}

/// A response being received: its blocks, and the tool results Claude Code reported before
/// the response ended.
struct Response {
    id: String,
    turn: usize,
    blocks: Vec<Block>,
    results: Vec<(UserContent, bool)>,
}

/// One Claude Code session's conversation.
struct Conversation<'a, 'r, R, W> {
    session: Session<'a, 'r>,
    channel: Channel<R, W>,
    /// Whether tool calls run; the format repair refuses them.
    tools_open: bool,
    /// The responses so far: each is one provider request, a turn.
    turn: usize,
    calls: u64,
    names: HashMap<String, String>,
    usage: BTreeMap<String, Counters>,
    response: Option<Response>,
    streaming: HashMap<usize, Streaming>,
    sink: Sink,
    limit: Option<wire::RateLimit>,
}

impl<'a, 'r, R: AsyncRead + Unpin, W: AsyncWrite + Unpin> Conversation<'a, 'r, R, W> {
    fn new(session: Session<'a, 'r>, channel: Channel<R, W>) -> Self {
        Self {
            session,
            channel,
            tools_open: true,
            turn: 0,
            calls: 0,
            names: HashMap::new(),
            usage: BTreeMap::new(),
            response: None,
            streaming: HashMap::new(),
            sink: Sink::default(),
            limit: None,
        }
    }

    async fn initialize(&mut self) -> Result<(), Failure> {
        let (deadline, cancellation) = (self.session.limits.deadline, self.session.cancellation);
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(Failure::cancelled()),
            _ = tokio::time::sleep_until(deadline) => Err(Failure::timeout()),
            initialized = self.channel.initialize(Some(self.session.server)) => initialized.map(|_| ()),
        }
    }

    /// Send one user message and follow the conversation to its answer: the final answer's
    /// structured output, or its text.
    async fn ask(&mut self, text: &str) -> Result<String, Failure> {
        self.channel.send(&wire::user(text)).await?;
        let mut cadence = tokio::time::interval(llm::delivery::CADENCE);
        cadence.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            let (deadline, cancellation) =
                (self.session.limits.deadline, self.session.cancellation);
            let line = tokio::select! {
                biased;
                _ = cancellation.cancelled() => return Err(Failure::cancelled()),
                _ = tokio::time::sleep_until(deadline) => return Err(Failure::timeout()),
                _ = cadence.tick() => {
                    self.deliver();
                    continue;
                }
                line = self.channel.next() => line?,
            };
            let Some(line) = line else {
                return Err(Failure::new(
                    Code::ProviderError,
                    "Claude Code exited before it answered.",
                ));
            };
            if let Some(answer) = self.line(line).await? {
                return Ok(answer);
            }
        }
    }

    /// Handle one line; the answer once the turn ended.
    async fn line(&mut self, line: Line) -> Result<Option<String>, Failure> {
        match line {
            Line::ControlRequest {
                request_id,
                request,
            } => self.control(&request_id, request).await?,
            Line::System(system) if system.subtype == "init" => {
                self.check_model(system.model.as_deref())?;
            }
            Line::StreamEvent { event } => self.stream(event)?,
            Line::Assistant(assistant) if assistant.parent_tool_use_id.is_none() => {
                if let Some(error) = assistant.error {
                    let text: String = assistant
                        .message
                        .content
                        .iter()
                        .filter_map(|block| match block {
                            Block::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect();
                    return Err(record::assistant_failure(error, &text, self.limit.as_ref()));
                }
                self.check_model(Some(&assistant.message.model))?;
                let message = assistant.message;
                self.begin(&message.id);
                self.count(&message.id, message.usage.as_ref())?;
                let response = self.response.as_mut().expect("begun above");
                for block in message.content {
                    if let Block::ToolUse { id, name, .. } = &block {
                        self.names.insert(id.clone(), name.clone());
                    }
                    if !response.blocks.contains(&block) {
                        response.blocks.push(block);
                    }
                }
            }
            Line::User(user) if user.parent_tool_use_id.is_none() => {
                if let wire::UserContent::Blocks(blocks) = user.message.content {
                    for block in blocks {
                        let wire::UserBlock::ToolResult {
                            tool_use_id,
                            content,
                            is_error,
                        } = block
                        else {
                            continue;
                        };
                        let name = self.names.get(&tool_use_id).cloned().unwrap_or_default();
                        if let Some(result) = record::tool_result(&tool_use_id, &name, content) {
                            self.result(result, is_error);
                        }
                    }
                }
            }
            Line::RateLimitEvent { rate_limit_info } => {
                if record::reached(&rate_limit_info) {
                    return Err(record::limit_failure(&rate_limit_info));
                }
                self.limit = Some(rate_limit_info);
            }
            Line::Result(finished) => {
                self.end_response(None);
                self.deliver();
                if finished.is_error || finished.subtype != "success" {
                    return Err(record::finish_failure(&finished.subtype, &finished.errors));
                }
                return Ok(Some(match finished.structured_output {
                    Some(output) => output.to_string(),
                    None => finished.result.unwrap_or_default(),
                }));
            }
            _ => {}
        }
        Ok(None)
    }

    /// Serve a control request: a message for artifactize's MCP server.
    async fn control(
        &mut self,
        request_id: &str,
        request: wire::ControlRequest,
    ) -> Result<(), Failure> {
        let wire::ControlRequest::McpMessage {
            server_name,
            message,
        } = request
        else {
            return self
                .channel
                .send(&wire::refusal(request_id, "Not served by artifactize."))
                .await;
        };
        if server_name != crate::llm::claude_agent::command::SERVER {
            return self
                .channel
                .send(&wire::refusal(request_id, "Unknown MCP server."))
                .await;
        }
        let reply = match self.session.server.answer(message) {
            mcp::Answer::Reply(reply) => reply,
            mcp::Answer::Call {
                id,
                name,
                arguments,
            } => match self.call(&name, arguments).await {
                Ok(result) => mcp::call_reply(&id, result),
                Err(failure) => {
                    let reply = mcp::call_error(&id, &failure.message);
                    // Answer the call so Claude Code is not left waiting, then stop.
                    let _ = self.channel.send(&wire::mcp_reply(request_id, reply)).await;
                    return Err(failure);
                }
            },
        };
        self.channel.send(&wire::mcp_reply(request_id, reply)).await
    }

    /// Run one tool call, counted against `maxToolCalls`.
    async fn call(
        &mut self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<crate::tools::ToolResult, Failure> {
        if !self.tools_open {
            return Err(Failure::new(
                Code::InvalidResult,
                "Invalid final Agent result after one format repair: tools are disabled.",
            ));
        }
        self.calls = self.calls.saturating_add(1);
        if self
            .session
            .limits
            .max_tool_calls
            .is_some_and(|limit| self.calls > limit)
        {
            return Err(Failure::new(
                Code::ProviderBudgetExceeded,
                "PROVIDER_BUDGET_EXCEEDED: review exceeded its maxToolCalls budget.",
            ));
        }
        let (deadline, cancellation) = (self.session.limits.deadline, self.session.cancellation);
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(Failure::cancelled()),
            _ = tokio::time::sleep_until(deadline) => Err(Failure::timeout()),
            result = self.session.registry.call(name, arguments, self.session.output, cancellation.clone()) => Ok(result),
        }
    }

    fn stream(&mut self, event: StreamEvent) -> Result<(), Failure> {
        match event {
            StreamEvent::MessageStart { message } => {
                self.check_model(Some(&message.model))?;
                self.begin(&message.id);
                self.count(&message.id, message.usage.as_ref())?;
            }
            StreamEvent::ContentBlockStart {
                index,
                content_block,
            } => {
                let (kind, block) = match content_block {
                    Block::Text { .. } => (DeliveryKind::Text, DeliveryBlock::text(index)),
                    Block::Thinking { .. } => (
                        DeliveryKind::Summary,
                        DeliveryBlock::summary("thinking", index),
                    ),
                    _ => return Ok(()),
                };
                self.streaming.insert(
                    index,
                    Streaming {
                        kind,
                        block,
                        text: String::new(),
                    },
                );
            }
            StreamEvent::ContentBlockDelta { index, delta } => {
                let text = match delta {
                    Delta::TextDelta { text } => text,
                    Delta::ThinkingDelta { thinking } => thinking,
                    Delta::Other => return Ok(()),
                };
                if let Some(streaming) = self.streaming.get_mut(&index) {
                    streaming.text.push_str(&text);
                    self.sink
                        .delta(streaming.kind, streaming.block.clone(), &text);
                }
            }
            StreamEvent::ContentBlockStop { index } => {
                if let Some(streaming) = self.streaming.remove(&index)
                    && !streaming.text.is_empty()
                {
                    self.sink
                        .finished(streaming.kind, streaming.block, streaming.text);
                }
                self.deliver();
            }
            StreamEvent::MessageDelta { usage } => {
                if let Some(id) = self.response.as_ref().map(|response| response.id.clone()) {
                    self.count(&id, usage.as_ref())?;
                }
            }
            StreamEvent::MessageStop => self.end_response(None),
            StreamEvent::Other => {}
        }
        Ok(())
    }

    fn check_model(&self, actual: Option<&str>) -> Result<(), Failure> {
        match actual {
            Some(actual) if actual != self.session.model => Err(Failure::new(
                Code::ProviderError,
                format!(
                    "Provider model mismatch: requested {:?}, received {actual:?}.",
                    self.session.model
                ),
            )),
            _ => Ok(()),
        }
    }

    /// Start recording response `id`, a new turn, unless it is the one being received.
    fn begin(&mut self, id: &str) {
        if self
            .response
            .as_ref()
            .is_some_and(|response| response.id == id)
        {
            return;
        }
        self.end_response(None);
        self.turn += 1;
        self.streaming.clear();
        self.response = Some(Response {
            id: id.to_owned(),
            turn: self.turn,
            blocks: Vec::new(),
            results: Vec::new(),
        });
    }

    /// Add a usage report of response `id`, checked against `maxTokens`.
    fn count(&mut self, id: &str, usage: Option<&wire::Usage>) -> Result<(), Failure> {
        let Some(usage) = usage else {
            return Ok(());
        };
        self.usage.entry(id.to_owned()).or_default().merge(usage);
        let used = self
            .usage
            .values()
            .fold(0_u64, |sum, counters| sum.saturating_add(counters.total()));
        if self
            .session
            .limits
            .max_tokens
            .is_some_and(|limit| used > limit)
        {
            return Err(Failure::new(
                Code::ProviderBudgetExceeded,
                "PROVIDER_BUDGET_EXCEEDED: review exceeded its maxTokens budget.",
            ));
        }
        Ok(())
    }

    /// A tool result: kept until its response is recorded, or recorded now.
    fn result(&mut self, result: UserContent, failed: bool) {
        match &mut self.response {
            Some(response) => response.results.push((result, failed)),
            None => {
                let message = Message::User {
                    content: vec![result],
                };
                self.session
                    .recorder
                    .tool_results(self.turn + 1, &message, &[failed]);
            }
        }
    }

    /// Record the response being received, its tool results and its attempt.
    fn end_response(&mut self, failure: Option<&Failure>) {
        let Some(response) = self.response.take() else {
            return;
        };
        let recorder = &mut self.session.recorder;
        if let Some(message) = record::assistant(&response.id, &response.blocks) {
            recorder.message(response.turn, &message, false);
        }
        if !response.results.is_empty() {
            let (content, failed): (Vec<_>, Vec<_>) = response.results.into_iter().unzip();
            recorder.tool_results(response.turn + 1, &Message::User { content }, &failed);
        }
        let attempt = Attempt {
            turn: response.turn,
            attempt: 1,
            usage: self
                .usage
                .get(&response.id)
                .copied()
                .unwrap_or_default()
                .usage()
                .into(),
            error: failure.map(|failure| failure.message.clone()),
            error_code: failure.map(|failure| failure.code),
        };
        recorder.event(session::Kind::Attempt(attempt.clone()));
        self.session.attempts.push(attempt);
    }

    /// Record how the conversation failed: on the response being received, or on the turn
    /// that could not start.
    fn fail(&mut self, failure: &Failure) {
        self.deliver();
        if self.response.is_some() {
            self.end_response(Some(failure));
            return;
        }
        let attempt = Attempt {
            turn: self.turn + 1,
            attempt: 1,
            usage: BTreeMap::new().into(),
            error: Some(failure.message.clone()),
            error_code: Some(failure.code),
        };
        self.session
            .recorder
            .event(session::Kind::Attempt(attempt.clone()));
        self.session.attempts.push(attempt);
    }

    /// Pass the text received since the last delivery on to the live view.
    fn deliver(&mut self) {
        let turn = self.turn.max(1);
        for delivery in self.sink.take(turn, 1) {
            self.session
                .recorder
                .event(session::Kind::Delivery(delivery));
        }
    }
}
