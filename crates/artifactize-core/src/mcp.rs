//! MCP is a bounded transport over the same core operations as the CLI.
mod catalog;
mod input;
mod page;
mod read;

use crate::{project::VerifyOptions, runs};
use input::*;
use rmcp::{ErrorData, ServerHandler, ServiceExt, model::*, service::RequestContext};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Leave room under the host's 45-second deadline for transport serialization.
const MAX_WAIT_SECONDS: u64 = 40;
const DEFAULT_WAIT_SECONDS: u64 = 30;
/// Limit each Run-list query to the agreed maximum record group; offset selects older groups.
const RUN_LIST_LIMIT: u32 = 20;
/// Includes a zero-bound initial read and failure recovery; no call reaches 45 s.
const WAIT_CALL_LIMIT: Duration = Duration::from_secs(44);
/// Both mirrors count; reserve 1 KiB for JSON-RPC framing under the 64 KiB public cap.
const RESPONSE_BYTES: usize = 63 * 1024;
/// Operational errors are readable diagnostics, not unbounded owner output.
const ERROR_CHARS: usize = 2048;

#[derive(Clone)]
pub struct Server {
    context: runs::Context,
}
impl Server {
    pub fn new(context: runs::Context) -> Self {
        Self { context }
    }

    async fn invoke(&self, name: &str, arguments: Value) -> Result<Value, String> {
        let context = &self.context;
        match name {
            "verify_start" => {
                let args: Start = parse(arguments)?;
                let options = VerifyOptions {
                    jobs: args.jobs.map(|limit| limit as usize),
                    modes: runs::RunModes {
                        speculate: args.speculate,
                        defer_human: args.defer_human,
                    },
                    ..Default::default()
                };
                encode(runs::start(context, &args.scope.0, &options, args.request_key).await?)
            }
            "verify_status" => {
                let args: Run = parse(arguments)?;
                encode(runs::status(context, &args.run).await?)
            }
            "verify_wait" => {
                let args: Wait = parse(arguments)?;
                let seconds = args.seconds.unwrap_or(DEFAULT_WAIT_SECONDS);
                if seconds > MAX_WAIT_SECONDS {
                    return Err("seconds must be an integer from 0 through 40.".into());
                }
                encode(
                    tokio::time::timeout(
                        WAIT_CALL_LIMIT,
                        runs::wait(
                            context,
                            &args.run,
                            Some(Duration::from_secs(seconds)),
                            CancellationToken::new(),
                        ),
                    )
                    .await
                    .map_err(|_| "State unavailable within the MCP observation deadline.")??,
                )
            }
            "verify_result" => {
                let args: ResultInput = parse(arguments)?;
                encode(runs::result(context, &args.run, args.cursor).await?)
            }
            "verify_cancel" => {
                let args: Run = parse(arguments)?;
                encode(runs::cancel(context, &args.run).await?)
            }
            name => read::inspect(context, name, arguments).await,
        }
    }
}
fn parse<T: DeserializeOwned>(value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|error| format!("Invalid arguments: {error}"))
}
fn encode(value: impl Serialize) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|error| error.to_string())
}
impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("artifactize", env!("CARGO_PKG_VERSION")))
            .with_instructions("Runs belong to artifactize, never the MCP connection. Retain the Run ID; use verify_wait rather than short polling. Human sign-offs belong to people. Read tools page the exact CLI JSON as UTF-8 text; append entries.text until next_cursor is null.")
    }
    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<rmcp::RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: catalog::tools(),
            ..Default::default()
        })
    }
    fn get_tool(&self, name: &str) -> Option<Tool> {
        catalog::tools().into_iter().find(|tool| tool.name == name)
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let arguments = Value::Object(request.arguments.unwrap_or_default());
        let result = match self.invoke(&request.name, arguments).await {
            Ok(value) => CallToolResult::structured(value),
            Err(error) => CallToolResult::structured_error(
                json!({"code":"operation_failed", "message":error.chars().take(ERROR_CHARS).collect::<String>()}),
            ),
        };
        if serde_json::to_vec(&result)
            .map_err(|error| ErrorData::internal_error(error.to_string(), None))?
            .len()
            > RESPONSE_BYTES
        {
            return Ok(CallToolResult::structured_error(json!({
                "code":"response_too_large", "message":"Response exceeds the detail bound; narrow the inspection."
            })).into());
        }
        Ok(result.into())
    }
}

/// Only rmcp writes stdout; detached Run workers own their own diagnostics.
pub async fn serve(context: runs::Context) -> Result<(), String> {
    match Server::new(context).serve(rmcp::transport::stdio()).await {
        Ok(service) => {
            service.waiting().await.map_err(|e| e.to_string())?;
            Ok(())
        }
        // A modern discovery probe can finish without opening an inline session.
        Err(rmcp::service::ServerInitializeError::ConnectionClosed(_)) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}
