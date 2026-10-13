//! Parse MCP arguments once at the transport boundary.
use crate::{project::selection::Selection, runs::ResultCursor, types::*};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Start {
    pub scope: Selection,
    pub request_key: Option<RequestKey>,
    #[serde(default)]
    pub speculate: bool,
    #[serde(default)]
    pub defer_human: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Run {
    pub run: RunId,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Wait {
    pub run: RunId,
    pub seconds: Option<u64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ResultInput {
    pub run: RunId,
    pub cursor: Option<ResultCursor>,
}
// Cursors are consumed by the shared transport projection, not core operations.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Empty {}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Status {
    #[serde(default = "all")]
    pub scope: Selection,
}
fn all() -> Selection {
    Selection::All
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Runs {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RunDetails {
    pub run: RunId,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Requests {
    pub run: Option<RunId>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    pub id: RequestId,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Session {
    pub reference: String,
    #[serde(default)]
    pub summary: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Tools {
    pub eval: Option<EvalId>,
    pub artifact: Option<ArtifactName>,
    pub tool: Option<crate::config::ToolName>,
    pub audience: Option<Audience>,
}
#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Audience {
    Agent,
    Human,
}
impl From<Audience> for crate::diagnostics::Audience {
    fn from(value: Audience) -> Self {
        match value {
            Audience::Agent => Self::Agent,
            Audience::Human => Self::Human,
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Models {
    pub provider: crate::config::Backend,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Cache {
    pub key: ReuseKey,
    #[serde(default)]
    pub history: bool,
}
