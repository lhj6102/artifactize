//! Saved eval detail loading; missing evidence is not invented from a summary.
use crate::{
    agent::session::{self, Kind},
    config::ProfileKind,
    store::{self, RequestView},
    types::RequestStatus,
};
use serde::Deserialize;
use std::{fs::File, io::Read, path::Path};

/// Keep a detail modal responsive even when a saved conversation is very large.
const CONVERSATION_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EvidenceStamp {
    status: RequestStatus,
    completed_at: Option<String>,
    session: Option<session::SessionRef>,
    session_id: Option<crate::types::SessionId>,
}
impl EvidenceStamp {
    pub fn new(view: &RequestView) -> Self {
        Self {
            status: view.request.status,
            completed_at: view.request.completed_at.clone(),
            session: view.request.session.clone(),
            session_id: view.request.session_id.clone(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Evidence {
    pub title: String,
    pub text: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeLog {
    stdout: String,
    stderr: String,
    exit_code: Option<i32>,
    #[serde(default)]
    truncated: bool,
}

pub async fn original(state: &Path, view: &RequestView) -> Result<RequestView, String> {
    let original = view
        .execution
        .as_ref()
        .map(|execution| &execution.provenance.request_id)
        .or_else(|| {
            view.request
                .provenance
                .as_ref()
                .map(|source| &source.request_id)
        });
    match original.filter(|id| *id != &view.request.id) {
        Some(id) => store::read_request(state, id).await,
        None => Ok(view.clone()),
    }
}

pub fn evidence(state: &Path, view: &RequestView) -> Evidence {
    match view.request.profile.kind() {
        ProfileKind::Agent => Evidence {
            title: "Saved Agent conversation".into(),
            text: conversation(state, view)
                .unwrap_or_else(|error| format!("Conversation unavailable: {error}")),
        },
        ProfileKind::Runtime => {
            let logs = view
                .request
                .result
                .clone()
                .and_then(|result| serde_json::from_value::<RuntimeLog>(result).ok());
            let text = match logs {
                Some(logs) => format!("exit code: {}\ncapture truncated: {}\n\nstdout\n{}\n\nstderr\n{}", logs.exit_code.map_or("unreported".into(), |code| code.to_string()), logs.truncated, logs.stdout, logs.stderr),
                None if view.request.origin.is_some() => "Logs unavailable: this remote result contains only a summary; stdout/stderr were not saved here.".into(),
                None if view.request.status == RequestStatus::Running => "Logs unavailable while running: only completed runtime output is saved; this modal does not stream live pipes.".into(),
                None => "Logs unavailable: stdout/stderr were not saved for this request (for example timeout, cancellation, operational failure, or a summary-only reused result).".into(),
            };
            Evidence {
                title: "Saved runtime stdout / stderr".into(),
                text,
            }
        }
        ProfileKind::Human => Evidence::default(),
    }
}

fn conversation(state: &Path, view: &RequestView) -> Result<String, String> {
    let request = &view.request;
    let reference = request.session.as_ref().or_else(|| {
        request
            .producer
            .as_ref()
            .and_then(|producer| producer.session.as_ref())
    });
    if let Some(reference) = reference
        && (Some(reference.state.as_str()) != store::read_state_id(state)?.as_deref()
            || reference.producer != store::Producer::current().name)
    {
        return Ok(format!(
            "This conversation lives in {}'s state {} (session {}). It is remote/unavailable here, not removed by local session GC.",
            reference.producer, reference.state, reference.session_id
        ));
    }
    let id = reference
        .map(|reference| &reference.session_id)
        .or(request.session_id.as_ref());
    let Some(id) = id else {
        return Ok("No saved Agent conversation was recorded for this request. It may have reused evidence with no session reference, predate session saving, or never have started an Agent review.".into());
    };
    let path = session::path(state, id)?;
    let file = match crate::platform::open_no_follow(File::options().read(true), &path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(if reference.is_some() {
                "This Agent session was removed by session GC (or the saved file was removed). `agentSessions` in `limits.json` sets how sessions are kept.".into()
            } else {
                "This Agent session was never recorded as saved: saving may have been disabled, failed, or the review never started. This is not evidence of session GC.".into()
            });
        }
        Err(error) => return Err(error.to_string()),
    };
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err("Session path is not a regular file.".into());
    }
    let limited = metadata.len() > CONVERSATION_BYTES;
    let mut text = format!("Session {id}\n");
    let mut bytes = Vec::new();
    file.take(CONVERSATION_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    let saved = String::from_utf8_lossy(&bytes);
    let mut lines = saved.lines().peekable();
    while let Some(line) = lines.next() {
        if limited && lines.peek().is_none() && !bytes.ends_with(b"\n") {
            break;
        }
        let event: session::Event = match serde_json::from_str(line) {
            Ok(event) => event,
            Err(error) if error.is_eof() && lines.peek().is_none() => break,
            Err(error) => return Err(format!("Invalid saved session event: {error}")),
        };
        match event.kind {
            Kind::Message(message) => {
                text.push_str(&format!(
                    "\nTurn {}\n{}\n",
                    message.turn,
                    serde_json::to_string_pretty(&message.message)
                        .map_err(|error| error.to_string())?
                ));
            }
            Kind::End(end) => text.push_str(&format!(
                "\nReview end\n{}\n",
                serde_json::to_string_pretty(&end).map_err(|error| error.to_string())?
            )),
            Kind::Send(send) => text.push_str(&format!(
                "\nFollow-up\n{}\n",
                serde_json::to_string_pretty(&send).map_err(|error| error.to_string())?
            )),
            Kind::Answer(answer) => text.push_str(&format!(
                "\nAnswer\n{}\n",
                serde_json::to_string_pretty(&answer).map_err(|error| error.to_string())?
            )),
            _ => {}
        }
    }
    if limited {
        text.push_str("\n[Conversation display truncated at 2 MiB; use session show for the full saved conversation.]\n");
    }
    Ok(text)
}
