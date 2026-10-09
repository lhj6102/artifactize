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
        session::{self, Conversation, Recorder, SessionRef, Summary},
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
        /// Print the conversation's turns, tokens, tool calls and follow-ups instead,
        /// computed from the saved session.
        #[arg(long)]
        summary: bool,
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
        SessionCommand::Show { reference, summary } => {
            let located = locate(state, &reference).await?;
            let conversation = located.load()?;
            if summary {
                let summary = Summary::new(&conversation);
                if json {
                    let mut value = json!(summary);
                    value["reference"] = json!(located.reference.as_ref().map(ToString::to_string));
                    value["sessionId"] = json!(located.id);
                    value["requestId"] = json!(located.request.request.id);
                    print_json(&value)?;
                } else {
                    show_summary(&located, &summary).map_err(|e| e.to_string())?;
                }
            } else if json {
                print_json(&json!({
                    "reference": located.reference.as_ref().map(ToString::to_string),
                    "file": conversation.path,
                    "events": conversation.wire_events,
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
            let (answer_text, attempts) = match answer {
                agent::FollowUp::Started {
                    answer: Ok(text),
                    attempts,
                } => (text, attempts),
                agent::FollowUp::NotStarted(failure)
                | agent::FollowUp::Started {
                    answer: Err(failure),
                    ..
                } => {
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
                    "sessionId": located.id.as_str(),
                    "requestId": located.request.request.id.as_str(),
                    "send": send.number,
                    "filesChanged": send.files_changed,
                    "answer": answer_text,
                    "usage": attempts,
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
    reused_by: Option<crate::types::RequestId>,
    id: crate::types::SessionId,
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
        () = cancellation.cancelled() => {
            return Err("Cancelled while waiting for another send to this session.".into());
        }
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
    let output = workspace::prepare_directory(
        &state.join("runs").join(request.run_id.as_str()),
        &config.root,
    )
    .map_err(|e| e.to_string())?;
    let files_changed = files_changed(&config, request, &output, cancellation.clone()).await;
    let header = conversation.header();
    let sent = Sent {
        number: conversation.sends() + 1,
        model: format!(
            "{} {}{}",
            header
                .backend
                .map(|backend| serde_json::to_value(backend)
                    .expect("backend is JSON")
                    .as_str()
                    .expect("backend is text")
                    .to_owned())
                .unwrap_or_else(|| "?".into()),
            header.model.as_deref().unwrap_or("?"),
            header
                .reasoning
                .as_deref()
                .map_or(String::new(), |reasoning| format!(
                    " (reasoning {reasoning})"
                ))
        ),
        files_changed,
    };
    let mut recorder = Recorder::append(state, &conversation.path, &located.id, sent.number)?;
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

/// Keep text session transcripts readable with bounded tool excerpts;
/// the explicit remainder marker directs readers to complete `--json` output.
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

/// The session, its backend and model, and the request whose review it is.
fn heading(out: &mut impl Write, located: &Located, header: &serde_json::Value) -> io::Result<()> {
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
    Ok(())
}

fn show_summary(located: &Located, summary: &Summary) -> io::Result<()> {
    let mut out = io::stdout().lock();
    let header = serde_json::json!({
        "backend": summary.backend, "model": summary.model, "reasoning": summary.reasoning,
    });
    heading(&mut out, located, &header)?;
    if let (Some(start), Some(end)) = (&summary.started_at, &summary.ended_at) {
        let took = summary.duration_ms.map_or(String::new(), |ms| {
            format!(" ({:.1} s)", ms as f64 / 1000.0)
        });
        writeln!(out, "Time: {start} to {end}{took}")?;
    }
    writeln!(
        out,
        "Turns: {} · follow-ups: {}",
        summary.turns.len(),
        summary.follow_ups
    )?;
    let none = || "none reported".to_owned();
    writeln!(
        out,
        "Tokens: {}",
        session::tokens_text(&summary.tokens).unwrap_or_else(none)
    )?;
    for turn in &summary.turns {
        let label = match turn.follow_up {
            Some(send) => format!("follow-up {send}, turn {}", turn.turn),
            None => format!("turn {}", turn.turn),
        };
        let retries = match turn.attempts {
            0 | 1 => String::new(),
            attempts => format!(" ({attempts} attempts)"),
        };
        writeln!(
            out,
            "  {label}: {}{retries}",
            session::tokens_text(&turn.tokens).unwrap_or_else(none)
        )?;
    }
    let failed = |count: u64| {
        if count == 0 {
            String::new()
        } else {
            format!(" ({count} failed)")
        }
    };
    let (calls, failures) = summary
        .tool_calls
        .values()
        .fold((0, 0), |(calls, failed), tool| {
            (calls + tool.calls, failed + tool.failed)
        });
    writeln!(out, "Tool calls: {calls}{}", failed(failures))?;
    for (name, tool) in &summary.tool_calls {
        writeln!(out, "  {name}: {}{}", tool.calls, failed(tool.failed))?;
    }
    Ok(())
}

fn show(located: &Located, conversation: &Conversation) -> io::Result<()> {
    let mut out = io::stdout().lock();
    heading(
        &mut out,
        located,
        &serde_json::to_value(conversation.header()).expect("header is JSON"),
    )?;
    for event in &conversation.events {
        let send = event
            .send
            .map_or(String::new(), |send| format!(", follow-up {send}"));
        match &event.kind {
            session::Kind::Message(event) => {
                let message = &event.message;
                let turn = event.turn;
                if let Some(question) = &event.question {
                    writeln!(out, "\n── Person (turn {turn}{send})\n{question}")?;
                    continue;
                }
                let label = match message {
                    Message::System { .. } => "System".to_owned(),
                    Message::User { .. } if event.repair => format!("Repair prompt (turn {turn})"),
                    Message::User { .. } => format!("User (turn {turn}{send})"),
                    Message::Assistant { .. } => format!("Assistant (turn {turn}{send})"),
                };
                writeln!(out, "\n── {label}")?;
                for line in message_lines(message) {
                    writeln!(out, "{line}")?;
                }
            }
            session::Kind::End(end) => match &end.result {
                Some(result) => writeln!(out, "\n── Result: {result}")?,
                None => writeln!(
                    out,
                    "\n── Review failed: {} {}",
                    end.error_code.as_deref().unwrap_or_default(),
                    end.error.as_deref().unwrap_or_default()
                )?,
            },
            session::Kind::Send(sent) => writeln!(
                out,
                "\n══ Follow-up {} ({}){}",
                event.send.unwrap_or_default(),
                event.at.as_deref().unwrap_or_default(),
                if sent.files_changed == Some(true) {
                    " · files changed since this review"
                } else {
                    ""
                }
            )?,
            session::Kind::Answer(answer) if answer.error.is_some() => writeln!(
                out,
                "\n── Follow-up {} failed: {} {}",
                event.send.unwrap_or_default(),
                answer.error_code.as_deref().unwrap_or_default(),
                answer.error.as_deref().unwrap_or_default()
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
