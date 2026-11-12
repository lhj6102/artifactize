//! One Claude Code session's conversation: the lines Claude Code writes, turned into tool
//! runs, budget checks, recorded messages and attempts, live text, and the turn's answer.

use std::collections::{BTreeMap, HashMap};

use rig_core::message::{Message, UserContent};
use tokio::io::{AsyncRead, AsyncWrite};

use super::{Session, record, record::Counters};
use crate::{
    agent::{
        error::{Code, Failure},
        session::{self, DeliveryKind},
    },
    llm::{
        self, Attempt,
        claude_agent::{
            channel::Channel,
            command, mcp,
            wire::{self, Block, Delta, Finished, Line, StreamEvent, System},
        },
        delivery::Sink,
    },
    types::{ClaudeMessageId, ClaudeToolUseId, DeliveryBlock},
};

/// The text of one content block while it streams.
struct Streaming {
    kind: DeliveryKind,
    block: DeliveryBlock,
    text: String,
}

/// A response being received: its blocks, and the tool results Claude Code reported before
/// the response ended.
struct Response {
    id: ClaudeMessageId,
    turn: usize,
    blocks: Vec<Block>,
    results: Vec<(UserContent, bool)>,
}

/// One Claude Code session's conversation.
pub(super) struct Conversation<'a, 'r, R, W> {
    pub(super) session: Session<'a, 'r>,
    pub(super) channel: Channel<R, W>,
    /// Whether tool calls run; the format repair refuses them.
    pub(super) tools_open: bool,
    /// The responses so far: each is one provider request, a turn.
    pub(super) turn: usize,
    calls: u64,
    names: HashMap<ClaudeToolUseId, String>,
    usage: BTreeMap<ClaudeMessageId, Counters>,
    response: Option<Response>,
    streaming: HashMap<usize, Streaming>,
    sink: Sink,
    limit: Option<wire::RateLimit>,
}

impl<'a, 'r, R: AsyncRead + Unpin, W: AsyncWrite + Unpin> Conversation<'a, 'r, R, W> {
    pub(super) fn new(session: Session<'a, 'r>, channel: Channel<R, W>) -> Self {
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

    pub(super) async fn initialize(&mut self) -> Result<(), Failure> {
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
    pub(super) async fn ask(&mut self, text: &str) -> Result<String, Failure> {
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
            Line::System(System::Init { model }) => self.check_model(model.as_deref())?,
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
                let id = message.id.ok_or_else(|| {
                    Failure::new(
                        Code::ProviderError,
                        "Claude Code wrote a response without an id.",
                    )
                })?;
                self.begin(&id);
                self.count(&id, message.usage.as_ref())?;
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
                return match finished {
                    Finished::Structured(output) => Ok(Some(output.to_string())),
                    Finished::Text(text) => Ok(Some(text)),
                    Finished::Failed { ending, errors } => {
                        Err(record::finish_failure(&ending, &errors))
                    }
                };
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
        if server_name != command::SERVER {
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
    fn begin(&mut self, id: &ClaudeMessageId) {
        if self
            .response
            .as_ref()
            .is_some_and(|response| &response.id == id)
        {
            return;
        }
        self.end_response(None);
        self.turn += 1;
        self.streaming.clear();
        self.response = Some(Response {
            id: id.clone(),
            turn: self.turn,
            blocks: Vec::new(),
            results: Vec::new(),
        });
    }

    /// Add a usage report of response `id`, checked against `maxTokens`.
    fn count(&mut self, id: &ClaudeMessageId, usage: Option<&wire::Usage>) -> Result<(), Failure> {
        let Some(usage) = usage else {
            return Ok(());
        };
        self.usage.entry(id.clone()).or_default().merge(usage);
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
    pub(super) fn fail(&mut self, failure: &Failure) {
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
