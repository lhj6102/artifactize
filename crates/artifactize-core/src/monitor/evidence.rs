//! Saved non-Agent eval detail. Agent files belong to the incremental typed session reader.
use crate::{
    agent::session,
    config::ProfileKind,
    store::{self, RequestView},
    types::RequestStatus,
};

use std::path::Path;

/// The session facts of a request: when they change, its evidence (an Agent session) is
/// resolved again; while they hold, an open reader keeps reading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceStamp {
    status: RequestStatus,
    completed_at: Option<crate::types::Timestamp>,
    session: Option<session::SessionRef>,
    session_id: Option<crate::types::SessionId>,
    producer: Option<store::Producer>,
    original: Option<crate::types::RequestId>,
}
impl EvidenceStamp {
    pub fn new(view: &RequestView) -> Self {
        Self {
            status: view.request.status(),
            completed_at: view.request.completed_at(),
            session: view.request.session.clone(),
            session_id: view.request.session_id.clone(),
            producer: view.request.producer.clone(),
            original: view
                .execution
                .as_ref()
                .map(|execution| execution.provenance.request_id.clone())
                .or_else(|| {
                    view.request
                        .provenance
                        .as_ref()
                        .map(|source| source.request_id.clone())
                }),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Evidence {
    pub title: String,
    pub text: String,
}
impl Evidence {
    /// Human evals have no evidence beside their sections.
    pub fn is_empty(&self) -> bool {
        self.title.is_empty() && self.text.is_empty()
    }
}

pub async fn original(state: &Path, view: &RequestView) -> Result<RequestView, String> {
    store::read_original(state, view).await
}

pub fn evidence(state: &Path, view: &RequestView) -> Evidence {
    match view.request.profile.kind() {
        ProfileKind::Agent => Evidence {
            title: "Agent session".into(),
            text: "Loading session…".into(),
        },
        ProfileKind::Runtime => {
            // Output lives in the attempt's logs; the result keeps only its issue.
            let streams = crate::inspect::logs::tails(state, view)
                .ok()
                .filter(|streams| streams.iter().any(|stream| !stream.lines.is_empty()));
            let text = match streams {
                Some(streams) => streams
                    .iter()
                    .map(|stream| {
                        let lines: Vec<_> =
                            stream.lines.iter().map(|line| line.text.as_str()).collect();
                        let name = format!("{:?}", stream.stream).to_lowercase();
                        format!("{name}\n{}", lines.join("\n"))
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n"),
                None if view.request.origin.is_some() => {
                    "Logs unavailable: this remote result carries its exit code, not its output."
                        .into()
                }
                None if view.request.status() == RequestStatus::Running => {
                    "Logs unavailable while running: this view shows a finished attempt's logs."
                        .into()
                }
                None => {
                    "Logs unavailable: this request's attempt left no logs here (for example a timeout, a cancellation, a reused result, or pruned Run output)."
                        .into()
                }
            };
            Evidence {
                title: "Runtime stdout / stderr".into(),
                text,
            }
        }
        ProfileKind::Human => Evidence::default(),
    }
}
