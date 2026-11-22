//! One Antigravity Chat turn. ACP carries text, while tool calls and outcomes are recorded
//! only where the Chat MCP bridge actually dispatches them.
use rig_core::message::{AssistantContent, Message, ToolCall, ToolFunction, ToolName};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use super::{
    Toolset, check_deadline,
    error::{Code, Failure},
    recorded_call,
    session::{Delivery, DeliveryKind, DeliveryState, Kind, Recorder},
    tool_content,
};
use crate::{
    chat::{Model, tools::Tools},
    llm::{
        antigravity::{
            Acp, Content, Event, Problem, PromptEvent, StopReason,
            bridge::{Bridge, Call},
        },
        stdio_mcp,
    },
    runs,
    types::DeliveryBlock,
};

pub(super) async fn turn(
    selection: &Model,
    context: &runs::Context,
    tools: &Tools<'_>,
    history: Vec<Message>,
    text: &str,
    recorder: &mut Recorder,
    cancel: &CancellationToken,
) -> Result<String, Failure> {
    let deadline = Instant::now() + super::chat::TURN_TIMEOUT;
    let mut acp = Acp::start(deadline, cancel).await.map_err(|problem| {
        check_deadline(cancel.is_cancelled(), deadline, Instant::now())
            .err()
            .unwrap_or_else(|| problem.into())
    })?;
    let mut session = None;
    let bridge = Bridge::start(stdio_mcp::Server::new(tools.list()));
    let result = match bridge {
        Ok(mut bridge) => {
            let result = async {
                let server = bridge.server(context.executable());
                let (id, system_appended) = tokio::select! {
                    biased;
                    () = cancel.cancelled() => return Err(Failure::cancelled()),
                    () = tokio::time::sleep_until(deadline) => return Err(Failure::timeout()),
                    result = acp.open(&selection.model, selection.effort.as_deref(), &server, super::chat::SYSTEM) => result?,
                };
                let id = session.insert(id);
                let prompt = super::chat::replay(&history, text);
                let prompt = if system_appended {
                    prompt
                } else {
                    format!("{}\n\n{prompt}", super::chat::SYSTEM)
                };
                let mut conversation = Conversation {
                    tools,
                    recorder,
                    cancel,
                    deadline,
                    stream: Text::default(),
                };
                let result = conversation.ask(&mut acp, &mut bridge, id, &prompt).await;
                conversation.stream.flush(conversation.recorder);
                result
            }
            .await;
            bridge.stop().await;
            result
        }
        Err(error) => Err(error),
    };
    if result.is_err()
        && let Some(session) = &session
    {
        acp.cancel(session).await;
    }
    let diagnostic = acp.stop().await;
    result.map_err(|failure| {
        if failure.code == Code::ProviderError
            && let Some(problem) = diagnostic
        {
            problem.into()
        } else {
            failure
        }
    })
}

struct Conversation<'a, 'r> {
    tools: &'a Tools<'r>,
    recorder: &'a mut Recorder,
    cancel: &'a CancellationToken,
    deadline: Instant,
    stream: Text,
}

impl Conversation<'_, '_> {
    async fn ask(
        &mut self,
        acp: &mut Acp,
        bridge: &mut Bridge,
        session: &str,
        prompt: &str,
    ) -> Result<String, Failure> {
        let request = tokio::select! {
            biased;
            () = self.cancel.cancelled() => return Err(Failure::cancelled()),
            () = tokio::time::sleep_until(self.deadline) => return Err(Failure::timeout()),
            result = acp.prompt(session, prompt) => result?,
        };
        loop {
            tokio::select! {
                biased;
                () = self.cancel.cancelled() => return Err(Failure::cancelled()),
                () = tokio::time::sleep_until(self.deadline) => return Err(Failure::timeout()),
                call = bridge.next() => match call {
                    Some(call) => self.call(call).await?,
                    None => return Err(Failure::from("The Antigravity Chat tool bridge stopped.".to_owned())),
                },
                event = acp.next(request) => match event? {
                    PromptEvent::Update(update) if update.session_id == session => {
                        if let Event::AgentMessageChunk { content: Content::Text { text } } = update.update {
                            self.stream.delta(self.recorder, &text);
                        }
                    }
                    PromptEvent::Update(_) => {}
                    PromptEvent::Finished(answer) => return match answer.stop_reason {
                        StopReason::EndTurn if !self.stream.answer.is_empty() => Ok(std::mem::take(&mut self.stream.answer)),
                        StopReason::Cancelled => Err(Failure::cancelled()),
                        StopReason::Refusal => Err(Problem::Quota.into()),
                        StopReason::MaxTokens | StopReason::MaxTurnRequests => Err(Failure::new(Code::ProviderBudgetExceeded, "Antigravity ended the turn at its output or request limit.")),
                        _ => Err(Failure::new(Code::ProviderError, "Antigravity ended without a Chat answer.")),
                    },
                }
            }
        }
    }

    async fn call(&mut self, requested: Call) -> Result<(), Failure> {
        check_deadline(self.cancel.is_cancelled(), self.deadline, Instant::now())?;
        if !self
            .tools
            .list()
            .any(|tool| tool.name.as_str() == requested.name)
        {
            let _ = requested
                .reply
                .send(stdio_mcp::call_error(&requested.id, "Unknown Chat tool."));
            return Ok(());
        }
        self.stream.flush(self.recorder);
        let name = ToolName::new(requested.name).expect("advertised Chat tool has a valid name");
        let identity = super::uuid()?;
        let call = ToolCall::from_wire(&identity, ToolFunction::new(name, requested.arguments));
        self.recorder.message(
            self.stream.turn(),
            &Message::Assistant {
                id: None,
                content: vec![AssistantContent::ToolCall(call.clone())],
            },
            false,
        );
        let registry = Toolset::Chat(self.tools);
        let name = call.function.name.as_str();
        let invoke = registry.invoke(
            name,
            call.function.arguments.clone(),
            self.tools.output(),
            self.cancel.clone(),
        );
        let called = if registry.durable_start(name) {
            // Once Verify admits a Run, Stop must not suppress its durable reference.
            invoke.await
        } else {
            tokio::select! {
                biased;
                () = self.cancel.cancelled() => return Err(Failure::cancelled()),
                () = tokio::time::sleep_until(self.deadline) => return Err(Failure::timeout()),
                result = invoke => result,
            }
        };
        let result = recorded_call(self.recorder, called);
        let reply = stdio_mcp::call_reply(&requested.id, result.clone());
        let failed = result.is_error();
        self.recorder.tool_results(
            self.stream.turn() + 1,
            &Message::tool_results(vec![call.result(tool_content(result))]),
            &[failed],
        );
        let _ = requested.reply.send(reply);
        self.stream.segment += 1;
        Ok(())
    }
}

/// Delivery events show incremental text, while one normal message per segment makes the
/// same text available to the next model after restart. Chat folds the two into one block.
#[derive(Default)]
struct Text {
    segment: usize,
    pending: String,
    answer: String,
}

impl Text {
    fn turn(&self) -> usize {
        self.segment + 1
    }

    fn delta(&mut self, recorder: &mut Recorder, text: &str) {
        if text.is_empty() {
            return;
        }
        self.pending.push_str(text);
        self.answer.push_str(text);
        recorder.event(Kind::Delivery(Delivery {
            turn: self.turn(),
            attempt: 1,
            block: DeliveryBlock::text(0),
            kind: DeliveryKind::Text,
            text: text.into(),
            state: DeliveryState::Delta,
        }));
    }

    fn flush(&mut self, recorder: &mut Recorder) {
        if !self.pending.is_empty() {
            recorder.message(
                self.turn(),
                &Message::assistant(std::mem::take(&mut self.pending)),
                false,
            );
        }
    }
}
