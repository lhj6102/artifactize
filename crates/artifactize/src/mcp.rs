//! Scoped stdio MCP serving and durable tool-call audit.

use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
};

use rmcp::{
    ErrorData, RoleServer, ServerHandler, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
        ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
    },
    service::RequestContext,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncRead, ReadBuf},
    sync::Mutex,
};
use tokio_util::sync::CancellationToken;

use crate::{
    config::{Eval, Profile, RepoConfig},
    scope,
    store::Receipts,
    tools::{Content, Registry, ToolResult},
    workspace,
};

mod input;

pub const REQUEST_LIMIT: usize = 64 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    execution_id: String,
    eval_id: String,
    repo: PathBuf,
    state: PathBuf,
    output: PathBuf,
    profile: Value,
}

/// Write private launcher files for an effective eval (including any selected profile variant).
/// The caller owns the external output directory and the execution's eventual completion.
pub async fn write_config(
    config: &RepoConfig,
    eval: &Eval,
    execution: &str,
    state: &Path,
    output: &Path,
) -> Result<PathBuf, String> {
    if execution.is_empty() || execution.len() > 256 || execution.chars().any(char::is_control) {
        return Err("Invalid MCP execution ID.".into());
    }
    let state = crate::store::state_dir(Some(state))?;
    let output = workspace::prepare_directory(output, &config.root).map_err(|e| e.to_string())?;
    let manifest = Manifest {
        execution_id: execution.into(),
        eval_id: eval.id.clone(),
        repo: config.root.clone(),
        state,
        output: output.clone(),
        profile: json!(eval.declaration.profile),
    };
    let registry = Registry::new(config, &eval.id)?;
    let receipts = Receipts::open(&manifest.state, &config.root).await?;
    receipts
        .register_mcp(
            execution,
            &binding(config, eval, &manifest, &registry)?,
            max_calls(eval)?,
        )
        .await?;
    let manifest_path = output.join("mcp-manifest.json");
    write_private(&manifest_path, &json!(manifest))?;
    let path = output.join("mcp-config.json");
    let program = std::env::current_exe().map_err(|e| e.to_string())?;
    write_private(
        &path,
        &json!({"mcpServers":{"artifactize":{"type":"stdio","command":program,"args":["mcp","--manifest",manifest_path]}}}),
    )?;
    Ok(path)
}

fn write_private(path: &Path, data: &Value) -> Result<(), String> {
    let mut file = tempfile::NamedTempFile::new_in(path.parent().ok_or("Missing MCP directory.")?)
        .map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut file, data).map_err(|e| e.to_string())?;
    file.flush().map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

fn max_calls(eval: &Eval) -> Result<Option<u64>, String> {
    match eval.declaration.profile {
        Profile::Agent { max_tool_calls, .. } => Ok(max_tool_calls),
        _ => Err("MCP requires an Agent eval.".into()),
    }
}

fn binding(
    config: &RepoConfig,
    eval: &Eval,
    manifest: &Manifest,
    registry: &Registry<'_>,
) -> Result<Value, String> {
    let scope = scope::eval_scope(config, eval).map_err(|e| e.to_string())?;
    Ok(
        json!({"manifest":manifest,"eval":eval,"artifacts":scope.artifacts,"tools":registry.list().collect::<Vec<_>>()}),
    )
}

pub async fn serve(manifest_path: &Path, cancellation: CancellationToken) -> Result<(), String> {
    let mut bytes = Vec::new();
    File::open(manifest_path)
        .map_err(|e| e.to_string())?
        .take(REQUEST_LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > REQUEST_LIMIT {
        return Err("MCP manifest is too large.".into());
    }
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let mut config =
        crate::config::read_workspace_config(&manifest.repo).map_err(|e| e.to_string())?;
    let index = config
        .evals
        .iter()
        .position(|eval| eval.id == manifest.eval_id)
        .ok_or("Unknown MCP eval.")?;
    if !matches!(
        config.evals[index].declaration.profile,
        Profile::Agent { .. }
    ) {
        return Err("MCP requires an Agent eval.".into());
    }
    let mut profile = manifest.profile.clone();
    // Declarations reject explicit nulls; saved profiles serialize absent options as null.
    if let Some(profile) = profile.as_object_mut() {
        profile.retain(|_, value| !value.is_null());
    }
    config.evals[index].declaration.profile =
        serde_json::from_value(profile).map_err(|e| e.to_string())?;
    let eval = &config.evals[index];
    let registry = Registry::new(&config, &eval.id)?;
    let tools = registry
        .list()
        .map(|tool| {
            Tool::new(
                tool.name.clone(),
                tool.description.clone(),
                tool.input_schema
                    .as_object()
                    .expect("validated tool schema")
                    .clone(),
            )
        })
        .collect();
    let output =
        workspace::prepare_directory(&manifest.output, &config.root).map_err(|e| e.to_string())?;
    let _lock = session_lock(&output)?;
    let receipts = Receipts::open(&manifest.state, &config.root).await?;
    receipts
        .register_mcp(
            &manifest.execution_id,
            &binding(&config, eval, &manifest, &registry)?,
            max_calls(eval)?,
        )
        .await?;
    let server = Server {
        config,
        manifest,
        receipts,
        tools,
        serial: Mutex::new(()),
        cancellation: cancellation.clone(),
    };
    let oversized = Arc::new(AtomicBool::new(false));
    let input = BoundedInput {
        inner: input::Input::new().map_err(|e| e.to_string())?,
        length: 0,
        oversized: oversized.clone(),
        cancellation: cancellation.clone(),
    };
    let running = server
        .serve_with_ct((input, tokio::io::stdout()), cancellation.clone())
        .await
        .map_err(|e| e.to_string())?;
    let outcome = running.waiting().await.map_err(|e| e.to_string());
    cancellation.cancel();
    if oversized.load(Ordering::Relaxed) {
        return Err("MCP request exceeds 64 KiB.".into());
    }
    match outcome? {
        rmcp::service::QuitReason::JoinError(error) => Err(error.to_string()),
        _ => Ok(()),
    }
}

fn session_lock(output: &Path) -> Result<File, String> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(output.join("mcp.lock"))
        .map_err(|e| e.to_string())?;
    // Held for the server lifetime, never across a SQLite transaction.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err("An MCP server is already connected to this execution.".into());
    }
    Ok(file)
}

struct Server {
    config: RepoConfig,
    manifest: Manifest,
    receipts: Receipts,
    tools: Vec<Tool>,
    serial: Mutex<()>,
    cancellation: CancellationToken,
}

impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "artifactize",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions("Read-only Agent tools scoped to this eval's declared Artifacts.")
    }

    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(self.tools.clone()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let _serial = self.serial.lock().await;
        let arguments = Value::Object(request.arguments.unwrap_or_default());
        let (order, denied) = self
            .receipts
            .begin_tool_call(&self.manifest.execution_id, &request.name, &arguments)
            .await
            .map_err(audit_error)?;
        let result = if let Some(error) = denied {
            ToolResult::error(error)
        } else {
            let registry =
                Registry::new(&self.config, &self.manifest.eval_id).map_err(audit_error)?;
            let token = self.cancellation.child_token();
            let _cancel_on_drop = token.clone().drop_guard();
            let call = registry.call(
                &request.name,
                arguments,
                &self.manifest.output,
                token.clone(),
            );
            tokio::pin!(call);
            tokio::select! {
                biased;
                _ = context.ct.cancelled() => { token.cancel(); call.await }
                result = &mut call => result,
            }
        };
        self.receipts
            .finish_tool_call(&self.manifest.execution_id, order, &result)
            .await
            .map_err(audit_error)?;
        let content = result
            .content
            .into_iter()
            .map(|block| match block {
                Content::Text { text } => ContentBlock::text(text),
                Content::Json { data } => ContentBlock::text(data.to_string()),
                Content::Image { data, mime_type } => ContentBlock::image(data, mime_type),
            })
            .collect();
        Ok(if result.is_error {
            CallToolResult::error(content)
        } else {
            CallToolResult::success(content)
        }
        .into())
    }
}

fn audit_error(message: String) -> ErrorData {
    ErrorData::internal_error(message, None)
}

/// Bound each inbound line before rmcp's read_until can accumulate it. Outbound images are uncapped here.
struct BoundedInput<R> {
    inner: R,
    length: usize,
    oversized: Arc<AtomicBool>,
    cancellation: CancellationToken,
}

impl<R: AsyncRead + Unpin> AsyncRead for BoundedInput<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if output.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        let mut bytes = [0; 8192];
        let size = output.remaining().min(bytes.len());
        let mut buffer = ReadBuf::new(&mut bytes[..size]);
        match Pin::new(&mut self.inner).poll_read(cx, &mut buffer) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(error)) => {
                self.cancellation.cancel();
                Poll::Ready(Err(error))
            }
            Poll::Ready(Ok(())) => {
                if buffer.filled().is_empty() {
                    self.cancellation.cancel();
                }
                for byte in buffer.filled() {
                    if *byte == b'\n' {
                        self.length = 0;
                    } else {
                        self.length += 1;
                    }
                    if self.length > REQUEST_LIMIT {
                        self.oversized.store(true, Ordering::Relaxed);
                        self.cancellation.cancel();
                        return Poll::Ready(Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "MCP request exceeds 64 KiB.",
                        )));
                    }
                }
                output.put_slice(buffer.filled());
                Poll::Ready(Ok(()))
            }
        }
    }
}
