//! The lines Claude Code writes in `stream-json` mode, parsed once into types, and the lines
//! artifactize writes back. Fields artifactize does not use are ignored; an unknown line or
//! block type is `Other`, since Claude Code adds new ones between versions.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// One line from Claude Code.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Line {
    /// Claude Code asks the host something: here, a message for the host's MCP server.
    ControlRequest {
        request_id: String,
        request: ControlRequest,
    },
    /// The answer to a request the host sent: here, `initialize`.
    ControlResponse {
        response: ControlResponse,
    },
    System(System),
    Assistant(Assistant),
    User(User),
    StreamEvent {
        event: StreamEvent,
    },
    RateLimitEvent {
        rate_limit_info: RateLimit,
    },
    Result(Finished),
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "subtype", rename_all = "snake_case")]
pub enum ControlRequest {
    McpMessage {
        server_name: String,
        message: Rpc,
    },
    #[serde(other)]
    Other,
}

/// A JSON-RPC message for the host's MCP server: a request the server answers, or a
/// notification (no `id`).
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Rpc {
    Known(McpRequest),
    Unknown {
        #[serde(default)]
        id: Option<RpcId>,
        method: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "method")]
pub enum McpRequest {
    #[serde(rename = "initialize")]
    Initialize { id: RpcId, params: InitializeParams },
    #[serde(rename = "tools/list")]
    ToolsList { id: RpcId },
    #[serde(rename = "tools/call")]
    ToolsCall { id: RpcId, params: CallParams },
    #[serde(rename = "ping")]
    Ping { id: RpcId },
}

/// A JSON-RPC id: a number or a string, echoed back unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RpcId {
    Number(i64),
    Text(String),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    pub protocol_version: String,
}

#[derive(Debug, Deserialize)]
pub struct CallParams {
    pub name: String,
    #[serde(default)]
    pub arguments: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "subtype", rename_all = "snake_case")]
pub enum ControlResponse {
    Success {
        request_id: String,
        #[serde(default)]
        response: Option<Initialized>,
    },
    Error {
        request_id: String,
        #[serde(default)]
        error: String,
    },
}

/// What `initialize` answers: the session's account, Claude Code's version and the models
/// it offers. No message has been sent, so no tokens were used.
#[derive(Debug, Default, Deserialize)]
pub struct Initialized {
    #[serde(default)]
    pub models: Vec<Model>,
    #[serde(default)]
    pub account: Option<Account>,
    #[serde(default)]
    pub claude_code_version: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    /// What `--model` takes: an alias such as `opus`, or a model id.
    pub value: String,
    /// The model id the value runs.
    #[serde(default)]
    pub resolved_model: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    #[serde(default)]
    pub subscription_type: Option<String>,
    #[serde(default)]
    pub api_provider: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct System {
    #[serde(default)]
    pub subtype: String,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Assistant {
    pub message: AssistantMessage,
    #[serde(default)]
    pub error: Option<AssistantError>,
    #[serde(default)]
    pub parent_tool_use_id: Option<String>,
}

/// Why Claude Code could not answer: it writes the reason as a synthetic assistant message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssistantError {
    AuthenticationFailed,
    BillingError,
    RateLimit,
    InvalidRequest,
    ServerError,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
pub struct AssistantMessage {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub content: Vec<Block>,
    #[serde(default)]
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Text {
        text: String,
    },
    Thinking {
        #[serde(default)]
        thinking: String,
        #[serde(default)]
        signature: String,
    },
    ToolUse {
        id: String,
        name: String,
        #[serde(default)]
        input: Value,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
pub struct User {
    pub message: UserMessage,
    #[serde(default)]
    pub parent_tool_use_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UserMessage {
    pub content: UserContent,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum UserContent {
    Text(String),
    Blocks(Vec<UserBlock>),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UserBlock {
    ToolResult {
        tool_use_id: String,
        #[serde(default)]
        content: ResultContent,
        #[serde(default)]
        is_error: bool,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ResultContent {
    Text(String),
    Blocks(Vec<ResultBlock>),
}

impl Default for ResultContent {
    fn default() -> Self {
        Self::Text(String::new())
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResultBlock {
    Text {
        text: String,
    },
    Image {
        source: ImageSource,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
pub struct ImageSource {
    pub media_type: String,
    pub data: String,
}

/// The Messages API's streaming events, which Claude Code passes on.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    MessageStart {
        message: StartedMessage,
    },
    ContentBlockStart {
        index: usize,
        content_block: Block,
    },
    ContentBlockDelta {
        index: usize,
        delta: Delta,
    },
    ContentBlockStop {
        index: usize,
    },
    MessageDelta {
        #[serde(default)]
        usage: Option<Usage>,
    },
    MessageStop,
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
pub struct StartedMessage {
    pub id: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Delta {
    TextDelta {
        text: String,
    },
    ThinkingDelta {
        thinking: String,
    },
    #[serde(other)]
    Other,
}

/// One response's token counters, as the Messages API reports them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub cache_read_input_tokens: Option<u64>,
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u64>,
    #[serde(default)]
    pub cache_creation: Option<CacheCreation>,
    #[serde(default)]
    pub output_tokens_details: Option<OutputDetails>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub struct CacheCreation {
    #[serde(default)]
    pub ephemeral_1h_input_tokens: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub struct OutputDetails {
    #[serde(default)]
    pub thinking_tokens: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimit {
    pub status: LimitStatus,
    /// When the window resets, in Unix seconds.
    #[serde(default)]
    pub resets_at: Option<i64>,
    /// The window: `five_hour`, `seven_day` and others.
    #[serde(default)]
    pub rate_limit_type: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitStatus {
    Allowed,
    AllowedWarning,
    Rejected,
    #[serde(other)]
    Other,
}

/// The end of one user turn.
#[derive(Debug, Deserialize)]
pub struct Finished {
    #[serde(default)]
    pub subtype: String,
    #[serde(default)]
    pub is_error: bool,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub structured_output: Option<Value>,
    #[serde(default)]
    pub errors: Vec<String>,
}

/// The id of the one control request artifactize sends.
pub const INITIALIZE: &str = "initialize";

/// `initialize`, naming the MCP server artifactize hosts in this process.
pub fn initialize(server: Option<&str>) -> Value {
    json!({
        "type": "control_request",
        "request_id": INITIALIZE,
        "request": {"subtype": "initialize", "sdkMcpServers": server.into_iter().collect::<Vec<_>>()},
    })
}

/// A user message sent verbatim: Claude Code adds none of its per-turn context to it.
pub fn user(text: &str) -> Value {
    json!({
        "type": "user",
        "session_id": "",
        "message": {"role": "user", "content": [{"type": "text", "text": text}]},
        "parent_tool_use_id": null,
        "client_composed": true,
    })
}

/// The answer to an MCP control request: the server's JSON-RPC `reply`.
pub fn mcp_reply(request_id: &str, reply: Value) -> Value {
    json!({
        "type": "control_response",
        "response": {"subtype": "success", "request_id": request_id, "response": {"mcp_response": reply}},
    })
}

/// The refusal of a control request artifactize does not serve.
pub fn refusal(request_id: &str, error: &str) -> Value {
    json!({
        "type": "control_response",
        "response": {"subtype": "error", "request_id": request_id, "error": error},
    })
}
