//! The shared MCP server for tools hosted by Artifactize.

use serde_json::{Value, json};

use crate::tools::{Content, ToolDefinition, ToolResult};
use serde::{Deserialize, Serialize};

/// The JSON-RPC error code of an unknown method.
pub(crate) const METHOD_NOT_FOUND: i64 = -32601;

/// The offered tools, in MCP's shape.
pub struct Server {
    tools: Vec<Value>,
}

/// What a message for the server needs: a reply now, or a tool run the review makes.
pub enum Answer {
    Reply(Value),
    Call {
        id: RpcId,
        name: String,
        arguments: Value,
    },
}

impl Server {
    pub fn new<'a>(tools: impl Iterator<Item = &'a ToolDefinition>) -> Self {
        Self {
            tools: tools
                .map(|tool| {
                    json!({
                        "name": tool.name,
                        "description": tool.description,
                        "inputSchema": tool.input_schema,
                    })
                })
                .collect(),
        }
    }

    pub fn answer(&self, message: Rpc) -> Answer {
        let request = match message {
            Rpc::Known(request) => request,
            // A notification needs no answer; Claude Code still expects the control reply.
            Rpc::Unknown { id: None, .. } => {
                return Answer::Reply(json!({"jsonrpc": "2.0", "result": {}}));
            }
            Rpc::Unknown {
                id: Some(id),
                method,
            } => {
                return Answer::Reply(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {"code": METHOD_NOT_FOUND, "message": format!("Unknown method {method}.")},
                }));
            }
        };
        Answer::Reply(match request {
            McpRequest::Initialize { id, params } => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": params.protocol_version,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": NAME, "version": env!("CARGO_PKG_VERSION")},
                },
            }),
            McpRequest::ToolsList { id } => {
                json!({"jsonrpc": "2.0", "id": id, "result": {"tools": self.tools}})
            }
            McpRequest::Ping { id } => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
            McpRequest::ToolsCall { id, params } => {
                return Answer::Call {
                    id,
                    name: params.name,
                    arguments: params.arguments.unwrap_or_else(|| json!({})),
                };
            }
        })
    }
}

/// A tool's result as MCP content: text, JSON as text, and images.
pub fn call_reply(id: &RpcId, result: ToolResult) -> Value {
    let is_error = result.is_error();
    let content: Vec<Value> = result
        .into_content()
        .into_iter()
        .map(|block| match block {
            Content::Text { text } => json!({"type": "text", "text": text}),
            Content::Json { data } => json!({"type": "text", "text": data.to_string()}),
            Content::Image { data, mime_type } => {
                use artifactize_tools::image::Mime;
                let mime = match mime_type {
                    Mime::Png => "image/png",
                    Mime::Jpeg => "image/jpeg",
                    Mime::Webp => "image/webp",
                };
                json!({"type": "image", "data": data, "mimeType": mime})
            }
        })
        .collect();
    json!({"jsonrpc": "2.0", "id": id, "result": {"content": content, "isError": is_error}})
}

/// A tool call refused before it ran.
pub fn call_error(id: &RpcId, message: &str) -> Value {
    call_reply(id, ToolResult::error(message))
}

pub(crate) const NAME: &str = "artifactize";

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
