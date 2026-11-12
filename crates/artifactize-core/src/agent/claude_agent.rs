//! Reviews and follow-ups on the `claude-agent` backend. Claude Code runs the conversation
//! with the user's own login; artifactize runs every tool call through the eval's registry,
//! enforces the budgets and the deadline, records the conversation and its live text, and
//! validates the verdict, with the same one format repair as every backend. Claude Code
//! retries failed requests itself, so nothing here retries.

mod conversation;
mod record;
#[cfg(test)]
mod tests;

use std::path::Path;

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
    session::{self, Recorder},
    verdict::{self, Structured, ValidatedResult},
};
use crate::{
    config::{Eval, Profile, RepoConfig},
    llm::{
        self, Api, Attempt, Client,
        claude_agent::{
            channel::{self, Running},
            command::Launch,
            mcp::Server,
            with_stderr,
        },
    },
    tools::Registry,
};
use conversation::Conversation;

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
