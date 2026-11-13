//! The MCP server artifactize hosts inside its own process for one Claude Code session.
//! Claude Code reaches it through its control protocol, so no socket or second process is
//! involved; the review runs every tool call itself, with its budgets and audit.

use serde_json::{Value, json};

use super::wire::{McpRequest, Rpc, RpcId};
use crate::tools::{Content, ToolDefinition, ToolResult};

/// The JSON-RPC error code of an unknown method.
const METHOD_NOT_FOUND: i64 = -32601;

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
                    "serverInfo": {"name": super::command::SERVER, "version": env!("CARGO_PKG_VERSION")},
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
