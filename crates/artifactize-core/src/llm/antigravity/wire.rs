//! The subset of ACP used for one interactive Chat turn.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::llm::{models::ListedModel, stdio_mcp::RpcId};

pub(super) const PROTOCOL: u32 = 1;
/// ACP's standard authentication-required JSON-RPC error.
const AUTH_REQUIRED: i64 = -32000;

#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum Incoming {
    Request {
        id: RpcId,
        method: String,
    },
    Update {
        method: SessionUpdate,
        params: Update,
    },
    Result {
        id: u64,
        result: Value,
    },
    Error {
        id: u64,
        error: RpcError,
    },
    Other(serde::de::IgnoredAny),
}

#[derive(Deserialize)]
pub(super) enum SessionUpdate {
    #[serde(rename = "session/update")]
    Update,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Update {
    pub session_id: String,
    pub update: Event,
}

#[derive(Deserialize)]
#[serde(tag = "sessionUpdate", rename_all = "snake_case")]
pub(crate) enum Event {
    AgentMessageChunk {
        content: Content,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Content {
    Text {
        text: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
pub(super) struct RpcError {
    code: i64,
    message: String,
    #[serde(default)]
    data: Value,
}

impl RpcError {
    pub(super) fn into_problem(self, method: &str) -> super::Problem {
        if self.code == AUTH_REQUIRED {
            return super::Problem::SignedOut;
        }
        super::Problem::reported(&format!("{} {} {}", self.code, self.message, self.data))
            .unwrap_or_else(|| {
                super::Problem::Failed(format!(
                    "Antigravity rejected ACP {method} (error {}).",
                    self.code
                ))
            })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Initialized {
    pub protocol_version: u32,
    #[serde(default)]
    pub agent_capabilities: Capabilities,
}

#[derive(Default, Deserialize)]
pub(super) struct Capabilities {
    #[serde(default)]
    pub auth: Auth,
    #[serde(default, rename = "_meta")]
    pub meta: AgentMeta,
}

#[derive(Default, Deserialize)]
pub(super) struct Auth {
    pub logout: Option<serde::de::IgnoredAny>,
}

#[derive(Default, Deserialize)]
pub(super) struct AgentMeta {
    #[serde(default)]
    pub antigravity: AntigravityCapabilities,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AntigravityCapabilities {
    #[serde(default)]
    pub system_prompt: SystemPrompt,
}

#[derive(Default, Deserialize)]
pub(super) struct SystemPrompt {
    #[serde(default)]
    pub append: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Session {
    pub session_id: String,
    #[serde(default)]
    pub config_options: Option<Vec<ConfigOption>>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ConfigOption {
    Select {
        id: String,
        category: Option<String>,
        #[serde(rename = "currentValue")]
        current_value: String,
        options: Vec<Choice>,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum Choice {
    Value { value: String, name: String },
    Group { options: Vec<Choice> },
    Other(serde::de::IgnoredAny),
}

impl Choice {
    fn values(&self, values: &mut Vec<(String, String)>) {
        match self {
            Self::Value { value, name } => {
                if !values.iter().any(|(listed, _)| listed == value) {
                    values.push((value.clone(), name.clone()));
                }
            }
            Self::Group { options } => options.iter().for_each(|choice| choice.values(values)),
            Self::Other(_) => {}
        }
    }
}

pub(super) struct Selection<'a> {
    pub id: &'a str,
    pub current: &'a str,
    choices: &'a [Choice],
}

impl Selection<'_> {
    pub fn values(&self) -> Vec<(String, String)> {
        let mut values = Vec::new();
        self.choices
            .iter()
            .for_each(|choice| choice.values(&mut values));
        values
    }

    pub fn models(&self) -> Vec<ListedModel> {
        self.values()
            .into_iter()
            .filter_map(|(value, name)| Some(ListedModel::new(value.parse().ok()?, name)))
            .collect()
    }
}

pub(super) fn select(
    config: &[ConfigOption],
    matches: impl Fn(&str, Option<&str>) -> bool,
) -> Option<Selection<'_>> {
    config.iter().find_map(|option| match option {
        ConfigOption::Select {
            id,
            category,
            current_value,
            options,
        } if matches(id, category.as_deref()) => Some(Selection {
            id,
            current: current_value,
            choices: options,
        }),
        _ => None,
    })
}

pub(super) fn model_option(config: &[ConfigOption]) -> Result<Selection<'_>, super::Problem> {
    select(config, |id, category| {
        category == Some("model") || id == "model"
    })
    .ok_or_else(|| {
        super::Problem::Failed("Antigravity did not advertise an ACP model option.".into())
    })
}

pub(super) fn thought_option(config: &[ConfigOption]) -> Option<Selection<'_>> {
    select(config, |id, category| {
        category == Some("thought_level")
            || matches!(id, "thought_level" | "thinking" | "thinking_level")
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PromptResult {
    pub stop_reason: StopReason,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StopReason {
    EndTurn,
    Cancelled,
    Refusal,
    MaxTokens,
    MaxTurnRequests,
    #[serde(other)]
    Other,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct NewSession<'a> {
    #[serde(serialize_with = "crate::platform::path_serde::native::serialize_path")]
    pub cwd: &'a std::path::Path,
    pub mcp_servers: &'a [McpServer<'a>],
}

#[derive(Serialize)]
pub(crate) struct McpServer<'a> {
    pub name: &'static str,
    #[serde(serialize_with = "crate::platform::path_serde::native::serialize_path")]
    pub command: &'a std::path::Path,
    pub args: [&'static str; 1],
    pub env: [McpEnv<'a>; 2],
}

#[derive(Serialize)]
pub(crate) struct McpEnv<'a> {
    pub name: &'static str,
    pub value: &'a str,
}

pub(super) fn initialization() -> Value {
    json!({"protocolVersion":PROTOCOL, "clientCapabilities":{}, "clientInfo":{
        "name":"artifactize", "title":"Artifactize", "version":env!("CARGO_PKG_VERSION"),
    }})
}

pub(super) fn refusal(id: RpcId, method: &str) -> Value {
    if method == "session/request_permission" {
        json!({"jsonrpc":"2.0", "id":id,"result":{"outcome":{"outcome":"cancelled"}}})
    } else {
        json!({"jsonrpc":"2.0", "id":id,"error":{"code":crate::llm::stdio_mcp::METHOD_NOT_FOUND,"message":"Artifactize does not provide this ACP capability."}})
    }
}
