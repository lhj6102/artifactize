//! MCP is a bounded transport over the same core operations as the CLI.
mod resolver;
mod response;

pub use resolver::Launch;

use artifactize_core::{
    runs,
    verify_tools::{self, Call, Error, Output},
};
use resolver::Resolver;
use response::encode;
use rmcp::{
    ErrorData, RoleServer, ServerHandler, ServiceExt,
    model::*,
    service::{NotificationContext, RequestContext, ServerInitializeError},
};

#[derive(Clone)]
pub struct Server {
    resolver: Resolver,
}

impl Server {
    pub fn new(launch: Launch) -> Self {
        Self {
            resolver: Resolver::new(launch),
        }
    }

    async fn invoke(
        &self,
        call: Call,
        transport: RequestContext<RoleServer>,
    ) -> Result<Output, Error> {
        let context = self
            .resolver
            .context(&transport, call.project.as_deref())
            .await
            .map_err(|error| Error::Operation(error.to_string()))?
            .with_origin(runs::Origin::Mcp);
        let token = transport.meta.get_progress_token();
        let mut sequence = 0_u32;
        call.execute(&context, transport.ct, |event| {
            sequence += 1;
            let notification = token.clone().map(|token| {
                ProgressNotificationParam::new(token, f64::from(sequence))
                    .with_message(serde_json::to_string(&event).expect("Run event serializes"))
            });
            let peer = transport.peer.clone();
            async move {
                if let Some(notification) = notification {
                    peer.notify_progress(notification)
                        .await
                        .map_err(|error| error.to_string())?;
                }
                Ok(())
            }
        })
        .await
    }
}

fn tools() -> Vec<Tool> {
    verify_tools::tools()
        .into_iter()
        .map(|tool| {
            let hints = tool.hints;
            Tool::new(tool.name, tool.description, tool.schema).with_annotations(
                ToolAnnotations::new()
                    .open_world(hints.open_world)
                    .read_only(hints.read_only)
                    .destructive(hints.destructive)
                    .idempotent(hints.idempotent),
            )
        })
        .collect()
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
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: tools(),
            ..Default::default()
        })
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        tools().into_iter().find(|tool| tool.name == name)
    }

    async fn on_roots_list_changed(&self, _: NotificationContext<RoleServer>) {
        self.resolver.roots_changed().await;
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let call = Call::parse(
            &request.name,
            request.arguments.unwrap_or_default(),
            time::OffsetDateTime::now_utc(),
        );
        let operation = match call {
            Ok(call) => self.invoke(call, context).await,
            Err(error) => Err(Error::Arguments(error)),
        };
        encode(operation)
    }
}

/// A failure to initialize the stdio server or join its service task.
#[derive(Debug)]
pub enum ServeError {
    Initialize(Box<ServerInitializeError>),
    Task(tokio::task::JoinError),
}

impl std::fmt::Display for ServeError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Initialize(error) => error.fmt(out),
            Self::Task(error) => error.fmt(out),
        }
    }
}

impl std::error::Error for ServeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::Initialize(error) => error.as_ref(),
            Self::Task(error) => error,
        })
    }
}

/// Only rmcp writes stdout; detached Run workers own their own diagnostics.
pub async fn serve(launch: Launch) -> Result<(), ServeError> {
    let (reader, writer) = rmcp::transport::stdio();
    serve_io(launch, reader, writer).await
}

/// Serve one MCP connection over an owned byte-stream transport.
pub async fn serve_io(
    launch: Launch,
    reader: impl tokio::io::AsyncRead + Unpin + Send + 'static,
    writer: impl tokio::io::AsyncWrite + Unpin + Send + 'static,
) -> Result<(), ServeError> {
    match Server::new(launch).serve((reader, writer)).await {
        Ok(service) => {
            service.waiting().await.map_err(ServeError::Task)?;
            Ok(())
        }
        // A modern discovery probe can finish without opening an inline session.
        Err(ServerInitializeError::ConnectionClosed(_)) => Ok(()),
        Err(error) => Err(ServeError::Initialize(Box::new(error))),
    }
}
