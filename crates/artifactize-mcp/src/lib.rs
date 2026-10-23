//! MCP is a bounded transport over the same core operations as the CLI.
mod catalog;
mod input;
mod page;
mod read;

use artifactize_core::{platform, project::VerifyOptions, runs};
use input::*;
use rmcp::{
    ErrorData, ServerHandler, ServiceExt,
    model::*,
    service::{NotificationContext, RequestContext},
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// Leave room under the host's 45-second deadline for transport serialization.
const MAX_WAIT_SECONDS: u64 = 40;
/// Observe for half a minute by default, reserving extra host-deadline headroom for routine calls.
const DEFAULT_WAIT_SECONDS: u64 = 30;
/// Limit each Run-list query to the agreed maximum record group; offset selects older groups.
const RUN_LIST_LIMIT: u32 = 20;
/// Includes a zero-bound initial read and failure recovery; no call reaches 45 s.
const WAIT_CALL_LIMIT: Duration = Duration::from_secs(44);
/// Both mirrors count; reserve 1 KiB for JSON-RPC framing under the 64 KiB public cap.
const RESPONSE_BYTES: usize = 63 * 1024;
/// Operational errors are readable diagnostics, not unbounded owner output.
const ERROR_CHARS: usize = 2048;
/// A client lists its roots from memory; this bound only stops one that never answers.
const ROOTS_WAIT: Duration = Duration::from_secs(10);

/// What the server was started with.
#[derive(Clone)]
pub struct Launch {
    /// The project named with `--repo`.
    pub repo: Option<runs::Context>,
    pub state: Option<PathBuf>,
    pub executable: PathBuf,
}

#[derive(Clone)]
pub struct Server {
    launch: Launch,
    /// The project the client's roots or the working directory named, until the roots change.
    bound: Arc<Mutex<Option<runs::Context>>>,
}
impl Server {
    pub fn new(launch: Launch) -> Self {
        Self {
            launch,
            bound: Arc::default(),
        }
    }

    /// The project a call serves: its own `project` argument, else the one named with `--repo`,
    /// else the client's first local root, else the working directory.
    async fn context(
        &self,
        call: &RequestContext<rmcp::RoleServer>,
        project: Option<PathBuf>,
    ) -> Result<runs::Context, String> {
        let Launch {
            repo,
            state,
            executable,
        } = &self.launch;
        if let Some(project) = project {
            return runs::Context::new(&project, state.as_deref(), executable);
        }
        if let Some(repo) = repo {
            return Ok(repo.clone());
        }
        let mut bound = self.bound.lock().await;
        if let Some(context) = bound.as_ref() {
            return Ok(context.clone());
        }
        let repo = match root(call).await? {
            Some(root) => root,
            None => working_folder()?,
        };
        let context = runs::Context::new(&repo, state.as_deref(), executable)?;
        *bound = Some(context.clone());
        Ok(context)
    }

    async fn watch(
        &self,
        arguments: Value,
        project: Option<PathBuf>,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<Value, String> {
        let args: Run = parse(arguments)?;
        let project = self.context(&context, project).await?;
        let token = context.meta.get_progress_token();
        let mut sequence = 0_u32;
        let observed = tokio::time::timeout(
            WAIT_CALL_LIMIT,
            runs::watch(
                &project,
                &args.run,
                Some(Duration::from_secs(DEFAULT_WAIT_SECONDS)),
                context.ct,
                |event| {
                    sequence += 1;
                    let notification = token.clone().map(|token| {
                        ProgressNotificationParam::new(token, f64::from(sequence)).with_message(
                            serde_json::to_string(&event).expect("Run event serializes"),
                        )
                    });
                    let peer = context.peer.clone();
                    async move {
                        if let Some(notification) = notification {
                            peer.notify_progress(notification)
                                .await
                                .map_err(|e| e.to_string())?;
                        }
                        Ok(())
                    }
                },
            ),
        )
        .await
        .map_err(|_| "State unavailable within the MCP observation deadline.")??;
        encode(observed)
    }

    async fn invoke(
        &self,
        name: &str,
        arguments: Value,
        project: Option<PathBuf>,
        call: &RequestContext<rmcp::RoleServer>,
    ) -> Result<Value, String> {
        let context = &self.context(call, project).await?;
        match name {
            "verify_start" => {
                let args: Start = parse(arguments)?;
                let options = VerifyOptions {
                    jobs: args.jobs.map(|limit| limit as usize),
                    modes: runs::RunModes {
                        speculate: args.speculate,
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
/// The client's first `file:` root, if the client has roots; an error when it has roots but
/// names no local folder, since its working directory is then the plugin's, not a project.
#[expect(
    deprecated,
    reason = "SEP-2577 deprecates roots without a replacement, and every spec version still serves them for a year after 2026-07-28; the `project` argument covers hosts without them"
)]
async fn root(call: &RequestContext<rmcp::RoleServer>) -> Result<Option<PathBuf>, String> {
    let declared = call
        .meta
        .client_capabilities()
        .or_else(|| call.peer.peer_info().map(|info| info.capabilities.clone()));
    if declared
        .and_then(|capabilities| capabilities.roots)
        .is_none()
    {
        return Ok(None);
    }
    let listed = tokio::time::timeout(ROOTS_WAIT, call.peer.list_roots())
        .await
        .map_err(|_| "The client did not list its roots in time; pass `project`, the absolute project folder.")?
        .map_err(|e| format!("The client's roots could not be read: {e}"))?;
    listed
        .roots
        .iter()
        .find_map(|root| platform::file_uri_path(&root.uri))
        .map(Some)
        .ok_or_else(|| {
            "The client's roots name no local folder; pass `project`, the absolute project folder."
                .into()
        })
}

/// The server's working directory, unless it is the root of the Agent Plugins package that
/// started it: a host without roots starts a plugin's server there, never in the project.
fn working_folder() -> Result<PathBuf, String> {
    let folder = std::env::current_dir().map_err(|e| e.to_string())?;
    if folder.join("plugin.json").is_file() && folder.join("mcp.json").is_file() {
        return Err("This host names no project: pass `project`, the absolute project folder, with each call.".into());
    }
    Ok(folder)
}

/// Takes the `project` argument every tool accepts out of a call's arguments.
fn project(arguments: &mut Value) -> Result<Option<PathBuf>, String> {
    match arguments
        .as_object_mut()
        .and_then(|map| map.remove("project"))
    {
        None => Ok(None),
        Some(Value::String(folder)) => platform::absolute_folder(&folder)
            .map(Some)
            .ok_or_else(|| "project must be an absolute folder.".into()),
        Some(_) => Err("Invalid arguments: project must be a string.".into()),
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
            .with_instructions("Every tool takes an optional project, the absolute project folder; pass it when a call reports that the host names no project. Runs belong to artifactize, never the MCP connection. Retain the Run ID; use verify_watch with _meta.progressToken for bounded live notifications, or verify_wait rather than short polling. Notifications require client display/queue support, never inject conversation turns. Human sign-offs belong to people. Read tools page the exact CLI JSON as UTF-8 text; append entries.text until next_cursor is null.")
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
    async fn on_roots_list_changed(&self, _: NotificationContext<rmcp::RoleServer>) {
        // The next call asks the client again.
        self.bound.lock().await.take();
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let mut arguments = Value::Object(request.arguments.unwrap_or_default());
        let operation = match project(&mut arguments) {
            Err(error) => Err(error),
            Ok(project) if request.name == "verify_watch" => {
                self.watch(arguments, project, context).await
            }
            Ok(project) => {
                self.invoke(&request.name, arguments, project, &context)
                    .await
            }
        };
        let result = match operation {
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
pub async fn serve(launch: Launch) -> Result<(), String> {
    match Server::new(launch).serve(rmcp::transport::stdio()).await {
        Ok(service) => {
            service.waiting().await.map_err(|e| e.to_string())?;
            Ok(())
        }
        // A modern discovery probe can finish without opening an inline session.
        Err(rmcp::service::ServerInitializeError::ConnectionClosed(_)) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}
