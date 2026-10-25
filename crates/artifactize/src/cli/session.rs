//! `session show` and `session send`: read and continue a saved Agent conversation.

use std::{
    io::{self, Write},
    path::Path,
};

use clap::Subcommand;
use rig_core::message::{AssistantContent, Message, ToolResultContent, UserContent};
use serde_json::json;

use super::{cancellation_listener, print_json};
use artifactize_core::agent::{
    self,
    session::{self, Conversation, Summary},
};
use artifactize_core::inspect::session::{Located, Selector, locate, send};

#[derive(Debug, Subcommand)]
pub enum SessionCommand {
    /// Print an Agent review's saved conversation; --json prints its stored events.
    Show {
        /// A session reference, request id or session id.
        #[arg(value_name = "REF")]
        reference: Selector,
        /// Print the conversation's turns, tokens, tool calls and follow-ups instead,
        /// computed from the saved session.
        #[arg(long)]
        summary: bool,
    },
    /// Ask the reviewer a follow-up in the saved conversation; the recorded verdict stays.
    Send {
        /// A session reference, request id or session id.
        #[arg(value_name = "REF")]
        reference: Selector,
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
            if json {
                print_json(
                    &artifactize_core::inspect::session::show(state, &reference, summary).await?,
                )?;
            } else {
                let located = locate(state, &reference).await?;
                let conversation = located.load()?;
                if summary {
                    show_summary(&located, &Summary::new(&conversation))
                        .map_err(|e| e.to_string())?;
                } else {
                    show(&located, &conversation).map_err(|e| e.to_string())?;
                }
            }
        }
        SessionCommand::Send { reference, message } => {
            let located = locate(state, &reference).await?;
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

/// The backend, model and reasoning a session ran with.
struct Engine {
    backend: Option<artifactize_core::config::Backend>,
    model: Option<artifactize_core::config::ModelId>,
    reasoning: Option<artifactize_core::config::Reasoning>,
}

/// An enum's name as its wire form spells it.
fn json_word(value: &impl serde::Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|word| word.as_str().map(str::to_owned))
        .unwrap_or_else(|| "?".into())
}

/// The session, its backend and model, and the request whose review it is.
fn heading(out: &mut impl Write, located: &Located, engine: &Engine) -> io::Result<()> {
    let request = &located.request.request;
    let word = |value: Option<String>| value.unwrap_or_else(|| "?".into());
    writeln!(
        out,
        "Session {} · {} {}{}",
        located.id,
        word(engine.backend.as_ref().map(json_word)),
        word(engine.model.as_ref().map(ToString::to_string)),
        engine
            .reasoning
            .as_ref()
            .map_or(String::new(), |reasoning| format!(
                " (reasoning {})",
                json_word(reasoning)
            ))
    )?;
    if let Some(reference) = &located.reference {
        writeln!(out, "Reference: {reference}")?;
    }
    writeln!(
        out,
        "Request {} (Run {}), eval {}: {}",
        request.id,
        request.run_id,
        request.eval_id,
        request.status()
    )?;
    if let Some(reused_by) = &located.reused_by {
        writeln!(out, "Request {reused_by} reused this review's result.")?;
    }
    Ok(())
}

fn show_summary(located: &Located, summary: &Summary) -> io::Result<()> {
    let mut out = io::stdout().lock();
    let engine = Engine {
        backend: summary.backend.clone(),
        model: summary.model.clone(),
        reasoning: summary.reasoning,
    };
    heading(&mut out, located, &engine)?;
    if let (Some(start), Some(end)) = (&summary.started_at, &summary.ended_at) {
        let took = summary.duration_ms.map_or(String::new(), |duration| {
            format!(" ({:.1} s)", duration.as_secs_f64())
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
    let header = conversation.header();
    heading(
        &mut out,
        located,
        &Engine {
            backend: header.backend.clone(),
            model: header.model.clone(),
            reasoning: header.reasoning,
        },
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
            session::Kind::End(end) => match end.result() {
                Some(result) => writeln!(out, "\n── Result: {}", result.to_json())?,
                None => writeln!(
                    out,
                    "\n── Review failed: {} {}",
                    end.failure()
                        .and_then(|(code, _)| code)
                        .map(artifactize_core::agent::error::Code::as_str)
                        .unwrap_or_default(),
                    end.failure()
                        .map(|(_, message)| message)
                        .unwrap_or_default()
                )?,
            },
            session::Kind::Send(sent) => writeln!(
                out,
                "\n══ Follow-up {} ({}){}",
                event.send.unwrap_or_default(),
                event.at.map(|at| at.to_string()).unwrap_or_default(),
                if sent.files_changed == Some(true) {
                    " · files changed since this review"
                } else {
                    ""
                }
            )?,
            session::Kind::Answer(answer) if answer.failure().is_some() => writeln!(
                out,
                "\n── Follow-up {} failed: {} {}",
                event.send.unwrap_or_default(),
                answer
                    .failure()
                    .and_then(|(code, _)| code)
                    .map(artifactize_core::agent::error::Code::as_str)
                    .unwrap_or_default(),
                answer
                    .failure()
                    .map(|(_, message)| message)
                    .unwrap_or_default()
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
