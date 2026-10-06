//! `session show` and `session send`: read and continue a saved Agent conversation.

use std::{
    io::{self, Write},
    path::{Path, PathBuf},
};

use clap::Subcommand;
use rig_core::message::{AssistantContent, Message, ToolResultContent, UserContent};
use serde_json::json;

use super::{cancellation_listener, print_json};
use crate::{
    agent::{
        self,
        session::{self, Conversation, Recorder, SessionRef},
    },
    cache,
    config::read_workspace_config,
    store::{self, Producer, RequestView},
    workspace,
};

#[derive(Debug, Subcommand)]
pub enum SessionCommand {
    /// Print an Agent review's saved conversation; --json prints its stored events.
    Show {
        /// A session reference, request id or session id.
        #[arg(value_name = "REF")]
        reference: String,
    },
    /// Ask the reviewer a follow-up in the saved conversation; the recorded verdict stays.
    Send {
        /// A session reference, request id or session id.
        #[arg(value_name = "REF")]
        reference: String,
        /// The message to send.
        message: String,
    },
}

pub(super) async fn execute(
    state: &Path,
    repo: Option<&Path>,
    command: SessionCommand,
    json: bool,
) -> Result<u8, String> {
    match command {
        SessionCommand::Show { reference } => {
            let located = locate(state, &reference).await?;
            let conversation = located.load()?;
            if json {
                print_json(&json!({
                    "reference": located.reference.as_ref().map(ToString::to_string),
                    "file": conversation.path,
                    "events": conversation.events,
                }))?;
            } else {
                show(&located, &conversation).map_err(|e| e.to_string())?;
            }
        }
        SessionCommand::Send { reference, message } => {
            let located = locate(state, &reference).await?;
            if matches!(
                located.request.request.status.as_str(),
                "QUEUED" | "RUNNING"
            ) {
                return Err(format!(
                    "Request {} is still running; send a follow-up once its review has finished.",
                    located.request.request.id
                ));
            }
            let (cancellation, listener) = cancellation_listener()?;
            let result = send(state, repo, &located, &message, cancellation).await;
            listener.abort();
            let (send, answer) = result?;
            let answer_text = match &answer.answer {
                Ok(text) => text.clone(),
                Err(failure) => {
                    return Err(format!(
                        "The follow-up failed ({}): {}",
                        failure.code.as_str(),
                        failure.message
                    ));
                }
            };
            if json {
                print_json(&json!({
                    "reference": located.reference.as_ref().map(ToString::to_string),
                    "sessionId": located.id,
                    "requestId": located.request.request.id,
                    "send": send.number,
                    "filesChanged": send.files_changed,
                    "answer": answer_text,
                    "toolCalls": answer.tool_calls,
                    "usage": answer.attempts,
                }))?;
            } else {
                let mut out = io::stdout().lock();
                let mut header = format!(
                    "Session {} · follow-up {} · {}",
                    located.id, send.number, send.model
                );
                if send.files_changed == Some(true) {
                    header.push_str(" · files changed since this review");
                }
                writeln!(out, "{header}\n\n{}", answer_text.trim_end())
                    .map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(0)
}

/// The review whose conversation a reference names, in this state.
struct Located {
    /// The request whose Agent review ran as the session.
    request: RequestView,
    /// The request the reference named, when it reused that review's result.
    reused_by: Option<String>,
    id: String,
    /// Set once the conversation was saved; absent when saving was off.
    reference: Option<SessionRef>,
    path: PathBuf,
}

impl Located {
    /// The saved conversation, or why there is none.
    fn load(&self) -> Result<Conversation, String> {
        Conversation::load(&self.path)?.ok_or_else(|| {
            let request = &self.request.request.id;
            if self.reference.is_some() {
                format!(
                    "Session {} of request {request} was removed by the session GC (limits.json agentSessions).",
                    self.id
                )
            } else {
                format!(
                    "Session {} of request {request} was not saved: saving was off (limits.json agentSessions.enabled), or the review never started.",
                    self.id
                )
            }
        })
    }
}

/// Fail unless `reference` names this machine's producer and this state.
fn here(state: &Path, reference: &SessionRef) -> Result<(), String> {
    let producer = Producer::current().name;
    let state_id = store::read_state_id(state)?;
    if reference.producer == producer && Some(&reference.state) == state_id.as_ref() {
        return Ok(());
    }
    Err(format!(
        "Session {} lives in {}'s state {}; this is {producer}'s state {}. Read or continue it there.",
        reference.session_id,
        reference.producer,
        reference.state,
        state_id.as_deref().unwrap_or("(not created yet)")
    ))
}

/// Resolve a full reference, a request id or a session id to its review's request.
async fn locate(state: &Path, text: &str) -> Result<Located, String> {
    let mut view = if let Some(reference) = SessionRef::parse(text) {
        here(state, &reference)?;
        let view = store::read_request(state, &reference.request_id)
            .await
            .map_err(|_| format!("Request {} is not in this state.", reference.request_id))?;
        if view.request.session_id.as_ref() != Some(&reference.session_id) {
            return Err(format!(
                "Request {} did not run session {}.",
                reference.request_id, reference.session_id
            ));
        }
        view
    } else {
        match store::read_request(state, text).await {
            Ok(view) => view,
            Err(_) => store::read_session_request(state, text)
                .await?
                .ok_or_else(|| {
                    format!(
                        "No Agent request or session {text} in this state ({}).",
                        state.display()
                    )
                })?,
        }
    };
    let mut reused_by = None;
    if view.request.session_id.is_none() {
        // A reused result names the review that produced it, which may live elsewhere.
        let Some(reference) = view.request.session.clone() else {
            return Err(format!(
                "Request {} ran no Agent review and names no saved conversation.",
                view.request.id
            ));
        };
        here(state, &reference)?;
        reused_by = Some(view.request.id.clone());
        view = store::read_request(state, &reference.request_id)
            .await
            .map_err(|_| format!("Request {} is not in this state.", reference.request_id))?;
    }
    let id = view
        .request
        .session_id
        .clone()
        .ok_or_else(|| format!("Request {} ran no Agent review.", view.request.id))?;
    Ok(Located {
        path: session::path(state, &id)?,
        reference: view.request.session.clone(),
        reused_by,
        id,
        request: view,
    })
}

struct Sent {
    number: usize,
    model: String,
    files_changed: Option<bool>,
}

/// Continue the conversation under its lock, so concurrent sends make one thread.
async fn send(
    state: &Path,
    repo: Option<&Path>,
    located: &Located,
    message: &str,
    cancellation: tokio_util::sync::CancellationToken,
) -> Result<(Sent, agent::FollowUp), String> {
    // A session that was never saved, or is gone, gets no lock file.
    located.load()?;
    let _lock = tokio::select! {
        lock = session::lock(state, &located.id) => lock?,
        () = cancellation.cancelled() => return Err("Cancelled while waiting for another send to this session.".into()),
    };
    let conversation = located.load()?;
    let request = &located.request.request;
    let repo = match repo {
        Some(repo) => repo.to_path_buf(),
        None => store::read_run(state, &request.run_id).await?.run.repo_path,
    };
    let config = read_workspace_config(&repo).map_err(|error| {
        format!(
            "Cannot resolve the review's tools in {}: {error}",
            repo.display()
        )
    })?;
    let eval = config
        .evals
        .iter()
        .find(|eval| eval.id == request.eval_id)
        .ok_or_else(|| {
            format!(
                "Eval {} is no longer declared in {}; its tools cannot be resolved.",
                request.eval_id,
                repo.display()
            )
        })?;
    let output =
        workspace::prepare_directory(&state.join("runs").join(&request.run_id), &config.root)
            .map_err(|e| e.to_string())?;
    let files_changed = files_changed(&config, request, &output, cancellation.clone()).await;
    let header = conversation.header();
    let sent = Sent {
        number: conversation.sends() + 1,
        model: format!(
            "{} {}{}",
            header["backend"].as_str().unwrap_or("?"),
            header["model"].as_str().unwrap_or("?"),
            header["reasoning"]
                .as_str()
                .map_or(String::new(), |reasoning| format!(
                    " (reasoning {reasoning})"
                ))
        ),
        files_changed,
    };
    let mut recorder = Recorder::append(&conversation.path, &located.id, sent.number)?;
    let answer = agent::follow_up(
        &config,
        eval,
        agent::Continuation {
            conversation: &conversation,
            files_changed,
            output: &output,
            state,
        },
        message,
        &mut recorder,
        cancellation,
    )
    .await;
    Ok((sent, answer))
}

/// Whether an Artifact the review's key covered has another fingerprint now; `None` when
/// the review had no key or a fingerprint cannot be computed.
async fn files_changed(
    config: &crate::config::RepoConfig,
    request: &store::Request,
    output: &Path,
    cancellation: tokio_util::sync::CancellationToken,
) -> Option<bool> {
    if request.fingerprints.is_empty() {
        return None;
    }
    if request
        .fingerprints
        .keys()
        .any(|id| !config.artifacts.contains_key(id))
    {
        return Some(true);
    }
    let current = cache::prepare(
        config,
        request.fingerprints.keys().map(String::as_str),
        output,
        &cache::Parallelism::new(cache::Parallelism::available()),
        cancellation,
    )
    .await
    .ok()?;
    Some(request.fingerprints.iter().any(|(id, fingerprint)| {
        current
            .get(id.as_str())
            .is_none_or(|now| &now.value != fingerprint)
    }))
}

/// Long tool results are cut in text; `--json` prints them whole.
const SHOWN_CHARS: usize = 2000;

fn clip(text: &str) -> String {
    let count = text.chars().count();
    if count <= SHOWN_CHARS {
        return text.into();
    }
    let shown: String = text.chars().take(SHOWN_CHARS).collect();
    format!(
        "{shown}\n… {} more characters (--json shows all)",
        count - SHOWN_CHARS
    )
}

fn show(located: &Located, conversation: &Conversation) -> io::Result<()> {
    let mut out = io::stdout().lock();
    let header = conversation.header();
    let request = &located.request.request;
    writeln!(
        out,
        "Session {} · {} {}{}",
        located.id,
        header["backend"].as_str().unwrap_or("?"),
        header["model"].as_str().unwrap_or("?"),
        header["reasoning"]
            .as_str()
            .map_or(String::new(), |reasoning| format!(
                " (reasoning {reasoning})"
            ))
    )?;
    if let Some(reference) = &located.reference {
        writeln!(out, "Reference: {reference}")?;
    }
    writeln!(
        out,
        "Request {} (Run {}), eval {}: {}",
        request.id, request.run_id, request.eval_id, request.status
    )?;
    if let Some(reused_by) = &located.reused_by {
        writeln!(out, "Request {reused_by} reused this review's result.")?;
    }
    for event in &conversation.events {
        let send = event["send"]
            .as_u64()
            .map_or(String::new(), |send| format!(", follow-up {send}"));
        match event["kind"].as_str() {
            Some("message") => {
                let Ok(message) = serde_json::from_value::<Message>(event["message"].clone())
                else {
                    continue;
                };
                let turn = event["turn"].as_u64().unwrap_or_default();
                // A follow-up's question shows the person's words; --json keeps the framing.
                if let Some(question) = event["question"].as_str() {
                    writeln!(out, "\n── Person (turn {turn}{send})\n{question}")?;
                    continue;
                }
                let label = match &message {
                    Message::System { .. } => "System".to_owned(),
                    Message::User { .. } if event["repair"] == true => {
                        format!("Repair prompt (turn {turn})")
                    }
                    Message::User { .. } => format!("User (turn {turn}{send})"),
                    Message::Assistant { .. } => format!("Assistant (turn {turn}{send})"),
                };
                writeln!(out, "\n── {label}")?;
                for line in message_lines(&message) {
                    writeln!(out, "{line}")?;
                }
            }
            Some("end") => match event.get("result") {
                Some(result) => writeln!(out, "\n── Result: {result}")?,
                None => writeln!(
                    out,
                    "\n── Review failed: {} {}",
                    event["errorCode"].as_str().unwrap_or_default(),
                    event["error"].as_str().unwrap_or_default()
                )?,
            },
            Some("send") => writeln!(
                out,
                "\n══ Follow-up {} ({}){}",
                event["send"],
                event["at"].as_str().unwrap_or_default(),
                if event["filesChanged"] == true {
                    " · files changed since this review"
                } else {
                    ""
                }
            )?,
            Some("answer") if event.get("error").is_some() => writeln!(
                out,
                "\n── Follow-up {} failed: {} {}",
                event["send"],
                event["errorCode"].as_str().unwrap_or_default(),
                event["error"].as_str().unwrap_or_default()
            )?,
            _ => {}
        }
    }
    Ok(())
}

/// A message as readable lines: text, tool calls with their arguments, tool results, and
/// reasoning summaries (encrypted reasoning is only noted).
fn message_lines(message: &Message) -> Vec<String> {
    let mut lines = Vec::new();
    match message {
        Message::System { content } => lines.push(content.clone()),
        Message::User { content } => {
            for part in content {
                match part {
                    UserContent::Text(text) => lines.push(text.text.clone()),
                    UserContent::ToolResult(result) => {
                        lines.push(format!("[tool result {}]", result.name));
                        for block in &result.content {
                            lines.push(clip(&match block {
                                ToolResultContent::Text(text) => text.text.clone(),
                                ToolResultContent::Json { value } => value.to_string(),
                                _ => "[image]".into(),
                            }));
                        }
                    }
                    _ => lines.push("[media]".into()),
                }
            }
        }
        Message::Assistant { content, .. } => {
            for part in content {
                match part {
                    AssistantContent::Text(text) => lines.push(text.text.clone()),
                    AssistantContent::ToolCall(call) => lines.push(format!(
                        "[tool call {}] {}",
                        call.function.name, call.function.arguments
                    )),
                    AssistantContent::Reasoning(sealed) => {
                        let summary = sealed
                            .open(sealed.issuer())
                            .map(|reasoning| reasoning.display_text())
                            .unwrap_or_default();
                        lines.push(if summary.trim().is_empty() {
                            "[reasoning, encrypted]".into()
                        } else {
                            format!("[reasoning] {summary}")
                        });
                    }
                    AssistantContent::Image(_) => lines.push("[image]".into()),
                }
            }
        }
    }
    lines
}
