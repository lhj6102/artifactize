//! Structured MCP response encoding and limits.
use artifactize_core::verify_tools::{Error, Output};
use rmcp::{
    ErrorData,
    model::{CallToolResponse, CallToolResult},
};
use serde_json::json;

/// Both mirrors count; reserve 1 KiB for JSON-RPC framing under the 64 KiB public cap.
const RESPONSE_BYTES: usize = 63 * 1024;
/// Operational errors are readable diagnostics, not unbounded owner output.
const ERROR_CHARS: usize = 2048;

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
