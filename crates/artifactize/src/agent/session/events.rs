//! Known JSONL event shapes parsed once when a conversation is opened.

use crate::{
    config::Backend,
    llm::Attempt,
    types::{RequestId, RunId, SessionId},
};
use rig_core::{completion::ToolDefinition, message::Message};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<crate::types::Timestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub send: Option<usize>,
    #[serde(flatten)]
    pub kind: Kind,
}
impl Event {
    /// The shared typed JSONL boundary. Unknown kinds and malformed known records are errors;
    /// unknown header metadata remains compatible through Header::extra.
    pub fn parse(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Kind {
    Review(Box<Header>),
    Message(MessageEvent),
    Attempt(Attempt),
    End(End),
    Send(Send),
    Answer(Answer),
    /// Display-only provider delivery; never part of replay history or usage calculations.
    Delivery(Delivery),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeliveryKind {
    Text,
    Summary,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeliveryState {
    Delta,
    Complete,
    Interrupted,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Delivery {
    pub turn: usize,
    pub attempt: usize,
    pub block: String,
    #[serde(rename = "contentKind")]
    pub kind: DeliveryKind,
    pub text: String,
    pub state: DeliveryState,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Header {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<SessionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<RequestId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<Backend>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<crate::config::ModelId>,
    #[serde(default)]
    pub reasoning: Option<crate::config::Reasoning>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budgets: Option<Budgets>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<crate::types::StateId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eval_id: Option<crate::types::EvalId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<crate::types::ArtifactName>,
    /// Provider-specific parameters remain dynamic.
    #[serde(default)]
    pub parameters: Value,
    // Unknown saved display metadata is retained for compatibility.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Budgets {
    #[serde(default, with = "crate::config::validation::milliseconds")]
    pub timeout_ms: Option<std::time::Duration>,
    #[serde(default)]
    pub max_tool_calls: Option<u64>,
    #[serde(default)]
    pub max_tokens: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageEvent {
    #[serde(default)]
    pub turn: usize,
    pub message: Message,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub repair: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub is_error: Vec<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Send {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub framing: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files_changed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
}
pub use super::outcome::{Answer, End};
