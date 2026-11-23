//! Known JSONL event shapes parsed once when a conversation is opened.

use crate::{
    config::Backend,
    llm::Attempt,
    types::{RequestId, RunId, SessionId},
};
use rig_core::{
    completion::ToolDefinition,
    message::{AssistantContent, Message, ToolCall},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf};

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
    /// Typed navigation, not model text or part of replay history.
    RunReference {
        run: RunId,
    },
    /// Display-only single outcome while other tool calls from the same turn still run.
    ToolOutcome(MessageEvent),
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
    pub block: crate::types::DeliveryBlock,
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
    pub producer: Option<crate::types::ProducerId>,
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
#[serde(rename_all = "camelCase", from = "MessageWire")]
pub struct MessageEvent {
    pub turn: usize,
    message: Message,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub repair: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub is_error: Vec<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
    // The complete message stays on the wire for replay; only the safe, known targets
    // are exposed to the transcript. The key is the position in the assistant message.
    #[serde(skip)]
    targets: BTreeMap<usize, BuiltinTarget>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessageWire {
    #[serde(default)]
    turn: usize,
    message: Message,
    #[serde(default)]
    repair: bool,
    #[serde(default)]
    is_error: Vec<bool>,
    question: Option<String>,
}

impl From<MessageWire> for MessageEvent {
    fn from(wire: MessageWire) -> Self {
        Self::new(
            wire.turn,
            wire.message,
            wire.repair,
            wire.is_error,
            wire.question,
        )
    }
}

impl MessageEvent {
    pub fn new(
        turn: usize,
        message: Message,
        repair: bool,
        is_error: Vec<bool>,
        question: Option<String>,
    ) -> Self {
        let targets = match &message {
            Message::Assistant { content, .. } => content
                .iter()
                .enumerate()
                .filter_map(|(index, part)| match part {
                    AssistantContent::ToolCall(call) => {
                        BuiltinTarget::decode(call).map(|target| (index, target))
                    }
                    _ => None,
                })
                .collect(),
            _ => BTreeMap::new(),
        };
        Self {
            turn,
            message,
            repair,
            is_error,
            question,
            targets,
        }
    }

    pub fn message(&self) -> &Message {
        &self.message
    }

    pub(crate) fn builtin_target_parts(
        &self,
        index: usize,
    ) -> (Option<&std::path::Path>, Option<&str>) {
        self.builtin_target(index).map_or((None, None), |target| {
            (target.path.as_deref(), target.pattern.as_deref())
        })
    }

    pub(super) fn builtin_target(&self, index: usize) -> Option<&BuiltinTarget> {
        self.targets.get(&index)
    }
}

#[derive(Debug, Clone)]
pub(super) struct BuiltinTarget {
    pub path: Option<PathBuf>,
    pub pattern: Option<String>,
}

impl BuiltinTarget {
    fn decode(call: &ToolCall) -> Option<Self> {
        let name = call.function.name.as_str();
        let builtin = ["read", "list", "glob", "grep", "view_image"]
            .iter()
            .any(|base| {
                name == *base
                    || name
                        .strip_prefix(base)
                        .is_some_and(|rest| rest.starts_with('_'))
            });
        builtin.then(|| {
            let text = |field| call.function.arguments.get(field).and_then(Value::as_str);
            Self {
                path: text("path").map(PathBuf::from),
                pattern: text("pattern").map(str::to_owned),
            }
        })
    }
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
    pub tools: Option<Vec<crate::types::ToolName>>,
}
pub use super::outcome::{Answer, End};
