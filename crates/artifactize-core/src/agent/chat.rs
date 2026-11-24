//! Conversational turns reuse the Agent provider loop without the verdict schema or repair.
use std::{borrow::Cow, collections::HashSet, path::Path, time::Duration};

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

/// Transport input, separate from the user message and recorded transcript.
pub(crate) struct Prompt {
    history: Vec<Message>,
    text: String,
    pub(super) system: Cow<'static, str>,
}

impl Prompt {
    pub(crate) fn new(
        history: Vec<Message>,
        text: String,
        moved_from: Option<&Path>,
        worktree: &Path,
    ) -> Self {
        let system = match moved_from {
            Some(from) => {
                let from = crate::platform::path_text(from);
                let to = crate::platform::path_text(worktree);
                Cow::Owned(format!(
                    "{SYSTEM}\n\nThis Chat session moved from {from:?} to {to:?}. Tools now use the new worktree."
                ))
            }
            None => Cow::Borrowed(SYSTEM),
        };
        Self {
            history,
            text,
            system,
        }
    }

    /// Replay Artifactize's own history into a fresh subprocess session, not as tool executions.
    pub(super) fn replay(&self) -> String {
        if self.history.is_empty() {
            return self.text.clone();
        }
        let records = self
            .history
            .iter()
            .map(|msg| serde_json::to_string(msg).expect("message is JSON"))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "Conversation history (one JSON message per line):\n{records}\n\nNew user message:\n{}",
            self.text
        )
    }
}

pub(crate) async fn turn(
    selection: &Model,
    context: &runs::Context,
    tools: &Tools<'_>,
    prompt: Prompt,
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
            return claude_agent::chat(selection, tools, &prompt, recorder, cancellation).await;
        }
        Route::Antigravity => {
            return super::antigravity::turn(
                selection,
                context,
                tools,
                &prompt,
                recorder,
                cancellation,
            )
            .await;
        }
    };
    let client = Client::at(route, &selection.model, context.state(), context.repo())?;
    let mut request = CompletionRequest::new(prompt.text);
    request.chat_history.splice(..0, prompt.history);
    request = request.preamble(prompt.system.into_owned());
    request.tools = definitions(tools.list());
    request.additional_params = Some(Client::chat_parameters(
        client.api(),
        selection.effort.as_deref(),
        recorder.id(),
    ));
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
        // The relocation note is startup context, not a repeat instruction on tool replies.
        request.chat_history[0] = Message::system(SYSTEM);
        request.chat_history.push(Message::Assistant {
            id: response.message_id,
            content: response.choice,
        });
        request.chat_history.push(turns.call_tools(calls).await?);
    }
}
