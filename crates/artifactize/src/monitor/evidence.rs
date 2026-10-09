//! Saved non-Agent eval detail. Agent files belong to the incremental typed session reader.
use crate::{
    agent::session,
    config::ProfileKind,
    store::{self, RequestView},
    types::RequestStatus,
};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EvidenceStamp {
    status: RequestStatus,
    completed_at: Option<String>,
    session: Option<session::SessionRef>,
    session_id: Option<crate::types::SessionId>,
    producer: Option<store::Producer>,
    original: Option<crate::types::RequestId>,
}
impl EvidenceStamp {
    pub fn new(view: &RequestView) -> Self {
        Self {
            status: view.request.status,
            completed_at: view.request.completed_at.clone(),
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
    /// Human and Dependency evals have no evidence beside their sections.
    pub fn is_empty(&self) -> bool {
        self.title.is_empty() && self.text.is_empty()
    }
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
    store::read_original(state, view).await
}

pub fn evidence(_state: &Path, view: &RequestView) -> Evidence {
    match view.request.profile.kind() {
        ProfileKind::Agent => Evidence {
            title: "Agent session".into(),
            text: "Loading session…".into(),
        },
        ProfileKind::Runtime => {
            let logs = view
                .request
                .result
                .clone()
                .and_then(|result| serde_json::from_value::<RuntimeLog>(result).ok());
            let text = match logs {
                Some(logs) => format!(
                    "exit code: {}\ncapture truncated: {}\n\nstdout\n{}\n\nstderr\n{}",
                    logs.exit_code
                        .map_or("unreported".into(), |code| code.to_string()),
                    logs.truncated,
                    logs.stdout,
                    logs.stderr
                ),
                None if view.request.origin.is_some() => {
                    "Logs unavailable: this remote result contains only a summary; stdout/stderr were not saved here."
                        .into()
                }
                None if view.request.status == RequestStatus::Running => {
                    "Logs unavailable while running: only completed runtime output is saved; this view does not stream live pipes."
                        .into()
                }
                None => {
                    "Logs unavailable: stdout/stderr were not saved for this request (for example timeout, cancellation, operational failure, or a summary-only reused result)."
                        .into()
                }
            };
            Evidence {
                title: "Saved runtime stdout / stderr".into(),
                text,
            }
        }
        ProfileKind::Human | ProfileKind::Dependency => Evidence::default(),
    }
}
