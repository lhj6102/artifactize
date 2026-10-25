//! Saved conversation lookup and continuation shared by interfaces.
mod send;
use crate::{
    agent::session::{self, Conversation, SessionRef, Summary},
    store::{self, Producer, RequestView},
};
pub use send::{Sent, send};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// What names a saved conversation: its full reference, or a bare id, which is read as a
/// request id and else as a session id (the two have the same form).
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(try_from = "String")]
pub enum Selector {
    Reference(SessionRef),
    Id {
        request: crate::types::RequestId,
        session: crate::types::SessionId,
    },
}

impl std::str::FromStr for Selector {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        if let Some(reference) = SessionRef::parse(text) {
            return Ok(Self::Reference(reference));
        }
        match (text.parse(), text.parse()) {
            (Ok(request), Ok(session)) => Ok(Self::Id { request, session }),
            _ => Err(format!(
                "{text} is not a session reference, a request id or a session id."
            )),
        }
    }
}

impl TryFrom<String> for Selector {
    type Error = String;

    fn try_from(text: String) -> Result<Self, String> {
        text.parse()
    }
}

/// The review whose conversation a reference names, in this state.
pub struct Located {
    /// The request whose Agent review ran as the session.
    pub request: RequestView,
    /// The request the reference named, when it reused that review's result.
    pub reused_by: Option<crate::types::RequestId>,
    pub id: crate::types::SessionId,
    /// Set once the conversation was saved; absent when saving was off.
    pub reference: Option<SessionRef>,
    pub path: PathBuf,
}

impl Located {
    /// The saved conversation, or why there is none.
    pub fn load(&self) -> Result<Conversation, String> {
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
    if reference.producer == producer
        && Some(reference.state.as_str()) == state_id.as_ref().map(crate::types::StateId::as_str)
    {
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
pub async fn locate(state: &Path, selector: &Selector) -> Result<Located, String> {
    let mut view = match selector {
        Selector::Reference(reference) => {
            here(state, reference)?;
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
        }
        Selector::Id { request, session } => {
            match store::read_request(state, request.as_str()).await {
                Ok(view) => view,
                Err(_) => store::read_session_request(state, session.as_str())
                    .await?
                    .ok_or_else(|| {
                        format!(
                            "No Agent request or session {session} in this state ({}).",
                            crate::platform::path_text(state)
                        )
                    })?,
            }
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

/// Project the same saved events or summary that `session show --json` prints.
pub async fn show(state: &Path, selector: &Selector, summary: bool) -> Result<Value, String> {
    let located = locate(state, selector).await?;
    let conversation = located.load()?;
    if summary {
        let mut value = json!(Summary::new(&conversation));
        value["reference"] = json!(located.reference.as_ref().map(ToString::to_string));
        value["sessionId"] = json!(located.id);
        value["requestId"] = json!(located.request.request.id);
        Ok(value)
    } else {
        Ok(
            json!({"reference": located.reference.as_ref().map(ToString::to_string),
            "file": crate::platform::path_text(&conversation.path), "events": conversation.wire_events}),
        )
    }
}
