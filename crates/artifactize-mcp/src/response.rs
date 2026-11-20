//! Structured MCP response encoding and limits.
use super::{error::Error, page::Page};
use artifactize_core::runs;
use rmcp::{
    ErrorData,
    model::{CallToolResponse, CallToolResult},
};
use serde::Serialize;
use serde_json::json;

/// Both mirrors count; reserve 1 KiB for JSON-RPC framing under the 64 KiB public cap.
const RESPONSE_BYTES: usize = 63 * 1024;
/// Operational errors are readable diagnostics, not unbounded owner output.
const ERROR_CHARS: usize = 2048;

#[derive(Serialize)]
#[serde(untagged)]
pub(super) enum Output {
    Observation(runs::RunObservation),
    Result(Box<runs::RunResultPage>),
    Cancellation(runs::CancelAcknowledgement),
    Inspection(Page),
    Plugin(PluginResult),
}

#[derive(Serialize)]
#[serde(tag = "action", rename_all = "lowercase")]
pub(super) enum PluginResult {
    Added(artifactize_core::config::plugins::PluginInfo),
    Removed {
        #[serde(rename = "type")]
        name: artifactize_core::config::ArtifactType,
    },
}

pub(super) fn encode(operation: Result<Output, Error>) -> Result<CallToolResponse, ErrorData> {
    let result = match operation.and_then(|output| Ok(serde_json::to_value(output)?)) {
        Ok(value) => CallToolResult::structured(value),
        Err(error) => CallToolResult::structured_error(json!({
            "code":"operation_failed",
            "message":error.to_string().chars().take(ERROR_CHARS).collect::<String>(),
        })),
    };
    let bytes = serde_json::to_vec(&result)
        .map_err(|error| ErrorData::internal_error(error.to_string(), None))?;
    let bounded = if bytes.len() > RESPONSE_BYTES {
        CallToolResult::structured_error(json!({
            "code":"response_too_large", "message":"Response exceeds the detail bound; narrow the inspection."
        }))
    } else {
        result
    };
    Ok(bounded.into())
}
