//! Stdio MCP adapter launched by ACP. The port and one-turn capability lead only to
//! the Chat recorder's checked dispatch, not the general Artifactize MCP server.
use std::{
    io::{self, BufRead, Read, Write},
    net::{Ipv4Addr, Shutdown, SocketAddr},
    path::Path,
    time::Duration,
};

use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

use crate::{agent::error::Failure, llm::stdio_mcp, platform};

use super::{LINE_BYTES, McpEnv, McpServer};

const PORT: &str = "ARTIFACTIZE_ACP_MCP_PORT";
const CAPABILITY: &str = "ARTIFACTIZE_ACP_MCP_CAPABILITY";
/// Local connections are immediate; do not let an idle peer monopolize the listener.
const CONNECT: Duration = Duration::from_secs(2);
/// Keep only one pending tool call; a stopped turn must not admit queued work.
const QUEUED_CALLS: usize = 1;

pub(crate) struct Bridge {
    port: String,
    capability: String,
    calls: mpsc::Receiver<Call>,
    task: Option<JoinHandle<()>>,
}

pub(crate) struct Call {
    pub id: stdio_mcp::RpcId,
    pub name: String,
    pub arguments: Value,
    pub reply: oneshot::Sender<Value>,
}

impl Bridge {
    pub fn start(tools: stdio_mcp::Server) -> Result<Self, Failure> {
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
        let listener = platform::net::exclusive_listener(address).map_err(bridge_error)?;
        let port = listener
            .local_addr()
            .map_err(bridge_error)?
            .port()
            .to_string();
        let capability = crate::agent::uuid()?;
        let (send, calls) = mpsc::channel(QUEUED_CALLS);
        let task = tokio::spawn(serve(listener, capability.clone(), tools, send));
        Ok(Self {
            port,
            capability,
            calls,
            task: Some(task),
        })
    }

    pub fn server<'a>(&'a self, executable: &'a Path) -> McpServer<'a> {
        McpServer {
            name: stdio_mcp::NAME,
            command: executable,
            args: ["__chat-mcp"],
            env: [
                McpEnv {
                    name: PORT,
                    value: &self.port,
                },
                McpEnv {
                    name: CAPABILITY,
                    value: &self.capability,
                },
            ],
        }
    }

    pub async fn next(&mut self) -> Option<Call> {
        self.calls.recv().await
    }

    pub async fn stop(mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
            let _ = crate::task::joined(task.await);
        }
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

fn bridge_error(error: io::Error) -> Failure {
    Failure::from(format!(
        "Cannot open the Antigravity Chat tool bridge: {error}."
    ))
}

async fn serve(
    listener: TcpListener,
    capability: String,
    tools: stdio_mcp::Server,
    calls: mpsc::Sender<Call>,
) {
    while let Ok((mut stream, _)) = listener.accept().await {
        let mut presented = vec![0; capability.len()];
        let read = tokio::time::timeout(CONNECT, stream.read_exact(&mut presented)).await;
        if matches!(read, Ok(Ok(_))) && presented == capability.as_bytes() {
            let _ = serve_stdio(stream, &tools, &calls).await;
        }
    }
}

async fn serve_stdio(
    stream: tokio::net::TcpStream,
    tools: &stdio_mcp::Server,
    calls: &mpsc::Sender<Call>,
) -> io::Result<()> {
    let (input, mut output) = stream.into_split();
    let mut input = BufReader::new(input);
    loop {
        let mut line = Vec::new();
        if (&mut input)
            .take(LINE_BYTES + 1)
            .read_until(b'\n', &mut line)
            .await?
            == 0
        {
            return Ok(());
        }
        if line.len() as u64 > LINE_BYTES {
            return Err(io::ErrorKind::InvalidData.into());
        }
        let request = serde_json::from_slice(&line);
        line.clear();
        let reply = match request {
            Ok(stdio_mcp::Rpc::Unknown { id: None, .. }) => continue,
            Ok(rpc) => match tools.answer(rpc) {
                stdio_mcp::Answer::Reply(reply) => reply,
                stdio_mcp::Answer::Call {
                    id,
                    name,
                    arguments,
                } => {
                    let (reply, receive) = oneshot::channel();
                    if calls
                        .send(Call {
                            id,
                            name,
                            arguments,
                            reply,
                        })
                        .await
                        .is_err()
                    {
                        return Ok(());
                    }
                    match receive.await {
                        Ok(reply) => reply,
                        Err(_) => return Ok(()),
                    }
                }
            },
            Err(_) => return Err(io::ErrorKind::InvalidData.into()),
        };
        serde_json::to_writer(&mut line, &reply)?;
        line.push(b'\n');
        output.write_all(&line).await?;
    }
}

/// Forward the agent's stdio to its one-turn server. This subprocess does not load workspace
/// configuration or shared state and cannot open a Chat or Verify context on its own.
#[doc(hidden)]
pub fn stdio() -> Result<(), String> {
    let unavailable = || "No Antigravity Chat tool bridge is available.".to_owned();
    let port = platform::environment::var_text(PORT)
        .and_then(|port| port.parse::<u16>().ok())
        .filter(|port| *port != 0)
        .ok_or_else(unavailable)?;
    let capability = platform::environment::var_text(CAPABILITY).ok_or_else(unavailable)?;
    let mut stream = platform::net::connect_loopback(port, CONNECT).map_err(|_| unavailable())?;
    stream
        .write_all(capability.as_bytes())
        .map_err(|_| unavailable())?;
    let mut send = stream.try_clone().map_err(|_| unavailable())?;
    // The process exits when the turn server closes, even if the agent still holds stdin.
    std::thread::Builder::new()
        .name("chat-mcp-stdin".into())
        .spawn(move || {
            let _ = relay(io::stdin().lock(), &mut send);
            let _ = send.shutdown(Shutdown::Write);
        })
        .map_err(|_| unavailable())?;
    relay(io::BufReader::new(&mut stream), &mut io::stdout().lock()).map_err(|_| unavailable())?;
    Ok(())
}

/// Forward complete MCP lines promptly in either direction, including to piped stdout.
fn relay(mut read: impl BufRead, write: &mut impl Write) -> io::Result<()> {
    let mut line = Vec::new();
    while (&mut read)
        .take(LINE_BYTES + 1)
        .read_until(b'\n', &mut line)?
        != 0
    {
        if line.len() as u64 > LINE_BYTES {
            return Err(io::ErrorKind::InvalidData.into());
        }
        write.write_all(&line)?;
        write.flush()?;
        line.clear();
    }
    Ok(())
}
