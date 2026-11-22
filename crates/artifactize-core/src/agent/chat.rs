//! Conversational turns reuse the Agent provider loop without the verdict schema or repair.
use std::{collections::HashSet, time::Duration};

use rig_core::{
    completion::{CompletionRequest, FinishReason},
    message::Message,
};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use super::{
    Toolset, Turns, calls_of, check_deadline, claude_agent, definitions,
    error::{Code, Failure},
    session::Recorder,
    text_of,
};
use crate::{
    chat::{Model, tools::Tools},
    llm::{self, Client, Route},
    runs,
};

/// One turn may do several calls, but a stuck model or tool cannot run forever.
pub(super) const TURN_TIMEOUT: Duration = Duration::from_secs(20 * 60);

pub(super) const SYSTEM: &str = "You are Artifactize Chat, working in the user's selected worktree. Use only the supplied tools to read or change files and run commands. Use the Verify tools to verify changes and observe the Run, and explain failures or waiting Human reviews. Run acceptance is not approval. Never claim a Human review or submit a verdict for a person.";

pub(crate) async fn turn(
    selection: &Model,
    context: &runs::Context,
    tools: &Tools<'_>,
    history: Vec<Message>,
    text: String,
    recorder: &mut Recorder,
    cancellation: &CancellationToken,
) -> Result<String, Failure> {
    let deadline = Instant::now() + TURN_TIMEOUT;
    check_deadline(cancellation.is_cancelled(), deadline, Instant::now())?;
    let route = llm::route(
        &selection.backend,
        Some(context.state()),
        Some(context.repo()),
        "for this Chat model",
    )?;
    let route = match route {
        Route::Http(route) => route,
        Route::ClaudeAgent => {
            return claude_agent::chat(
                &selection.model,
                tools,
                history,
                &text,
                recorder,
                cancellation,
            )
            .await;
        }
        Route::Antigravity => {
            return super::antigravity::turn(
                &selection.model,
                context,
                tools,
                history,
                &text,
                recorder,
                cancellation,
            )
            .await;
        }
    };
    let client = Client::at(route, &selection.model, context.state(), context.repo())?;
    let mut request = CompletionRequest::new(text).preamble(SYSTEM);
    request.chat_history.splice(..0, history);
    request.tools = definitions(tools.list());
    request.additional_params = Some(Client::parameters(client.api(), None, recorder.id())?);
    let mut turns = Turns {
        client: &client,
        registry: Toolset::Chat(tools),
        model: &selection.model,
        output: tools.output(),
        deadline,
        cancellation,
        max_tokens: None,
        max_tool_calls: None,
        turn: 0,
        tokens_used: 0,
        calls_issued: 0,
        call_ids: HashSet::new(),
        recorder,
    };
    let mut attempts = Vec::new();
    loop {
        let response = turns.next(&request, &mut attempts).await?;
        let calls = calls_of(&response);
        if calls.is_empty() {
            if response.finish_reason() != Some(FinishReason::Stop) {
                return Err(Failure::new(
                    Code::ProviderError,
                    "Provider ended without an answer or a callable tool.",
                ));
            }
            return Ok(text_of(&response));
        }
        request.chat_history.push(Message::Assistant {
            id: response.message_id,
            content: response.choice,
        });
        request.chat_history.push(turns.call_tools(calls).await?);
    }
}

/// Replay Artifactize's own history into a fresh subprocess session. Do not rely on a
/// vendor session's storage or replay any calls as executions.
pub(super) fn replay(history: &[Message], text: &str) -> String {
    if history.is_empty() {
        return text.to_owned();
    }
    let records = history
        .iter()
        .map(|msg| serde_json::to_string(msg).expect("message is JSON"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Conversation history (one JSON message per line):\n{records}\n\nNew user message:\n{text}"
    )
}
