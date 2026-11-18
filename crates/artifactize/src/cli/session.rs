//! `session show` and `session send`: read and continue a saved Agent conversation.

use std::{
    io::{self, Write},
    path::Path,
};

use clap::Subcommand;
use serde_json::json;

use super::{cancellation_listener, print_json};
use artifactize_core::agent::{self, session::Summary};
use artifactize_core::inspect::session::{Selector, locate, send};

mod text;

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
                    text::show_summary(&located, &Summary::new(&conversation))
                        .map_err(|e| e.to_string())?;
                } else {
                    text::show(&located, &conversation).map_err(|e| e.to_string())?;
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
