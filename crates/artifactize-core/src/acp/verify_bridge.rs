//! One-turn Verify/read-only MCP bridge for ACP Chat agents (`C-ONE-TURN-OWNERSHIP`).
//!
//! Even when a warm ACP child stays alive across turns, every turn opens a fresh loopback
//! listener and one-time capability token, passes it via `session/load` or `session/new`, and
//! immediately closes and retires the listener on turn end, cancellation, error, `set_worktree`,
//! or `Chat::close()`. Exposes only the allowlisted read-only + Verify tools (never file/shell
//! or plugin-mutation tools), strips `project`, and maps agent-supplied `verify_start` keys
//! into the disjoint internal `chat/<sha256[..32]>` namespace with canonical `request_args_hash`.

use std::{
    io::{self, BufRead, Read, Write},
    net::{Ipv4Addr, Shutdown, SocketAddr},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    sync::{Semaphore, mpsc, oneshot},
    task::JoinHandle,
};

use crate::{
    agent::error::Failure,
    llm::stdio_mcp,
    platform,
    types::{RequestKey, SessionId},
};

use super::{
    VerifyBridgeState,
    wire::{LINE_BYTES, McpEnv, McpServer},
};

const PORT_ENV: &str = "ARTIFACTIZE_ACP_MCP_PORT";
const CAPABILITY_ENV: &str = "ARTIFACTIZE_ACP_MCP_CAPABILITY";
/// Local connections are immediate; do not let an idle peer monopolize the listener.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// Keep only one pending tool call; a stopped turn must not admit queued work.
const QUEUED_CALLS: usize = 1;
/// Allow authenticated reconnects within a single turn without unbounded socket growth.
const MAX_BRIDGE_CONNECTIONS: usize = 4;
/// JSON-RPC error returned for `tools/call` outside an active Chat turn.
pub const NO_ACTIVE_TURN_CODE: i64 = -32000;
/// Length of the hex digest suffix in internal `chat/<32-hex>` request keys (37 bytes total).
const INTERNAL_KEY_HEX_LEN: usize = 32;

/// Exact allowlist of Artifactize tools exposed to ACP Chat agents over the one-turn bridge
/// (`verify_tools/tool.rs:26-55`, explicitly excluding file/shell tools and `plugin_add`/`plugin_remove`).
pub const ALLOWLISTED_TOOLS: &[&str] = &[
    // Read-only inspection (17 tools)
    "status",
    "config_graph",
    "request_logs",
    "config_check",
    "config_expand",
    "config_symbols",
    "run_list",
    "run_show",
    "request_list",
    "request_show",
    "session_show",
    "tools_check",
    "doctor",
    "models",
    "remote_status",
    "cache_show",
    "plugin_list",
    // Verify execution & control (6 tools)
    "verify_start",
    "verify_status",
    "verify_wait",
    "verify_watch",
    "verify_result",
    "verify_cancel",
];

/// Whether `name` is permitted on the ACP Verify MCP bridge.
pub fn is_allowlisted(name: &str) -> bool {
    ALLOWLISTED_TOOLS.contains(&name)
}

/// Map an agent-supplied `verify_start` `request_key` into the mathematically disjoint internal
/// `chat/<sha256_hex[..32]>` namespace (`/` is forbidden in external `RequestKey`).
pub fn internal_chat_request_key(
    session_id: &SessionId,
    send: usize,
    agent_request_key: &str,
) -> Result<RequestKey, String> {
    let mut hasher = Sha256::new();
    hasher.update(session_id.as_str().as_bytes());
    hasher.update([0]);
    hasher.update(send.to_string().as_bytes());
    hasher.update([0]);
    hasher.update(agent_request_key.as_bytes());
    let digest = hex_prefix(hasher.finalize().as_slice(), INTERNAL_KEY_HEX_LEN);
    RequestKey::internal_chat(format!("chat/{digest}"))
}

/// Canonical SHA-256 digest of `verify_start` arguments (excluding `request_key`) used to
/// detect conflicting reuse of an internal `chat/*` key.
pub fn canonical_args_hash(arguments: &Value) -> String {
    let canonical = canonicalize_json(arguments, true);
    let encoded = serde_json::to_string(&canonical).unwrap_or_default();
    let digest = Sha256::digest(encoded.as_bytes());
    hex_prefix(digest.as_slice(), 64)
}

fn canonicalize_json(value: &Value, strip_request_key: bool) -> Value {
    match value {
        Value::Object(map) => {
            let mut sorted = serde_json::Map::new();
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for key in keys {
                if strip_request_key && (key == "request_key" || key == "requestKey") {
                    continue;
                }
                sorted.insert(key.clone(), canonicalize_json(&map[key], false));
            }
            Value::Object(sorted)
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| canonicalize_json(item, false))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn hex_prefix(bytes: &[u8], chars: usize) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(chars);
    for &byte in bytes {
        if out.len() >= chars {
            break;
        }
        out.push(HEX[(byte >> 4) as usize] as char);
        if out.len() >= chars {
            break;
        }
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

/// One incoming allowlisted MCP tool call from the ACP agent.
pub struct BridgeCall {
    pub id: stdio_mcp::RpcId,
    pub name: String,
    pub arguments: Value,
    pub reply: oneshot::Sender<Value>,
}

/// One-turn loopback Verify MCP bridge.
pub struct VerifyBridge {
    port: String,
    capability: String,
    calls: mpsc::Receiver<BridgeCall>,
    active_turn: Arc<AtomicBool>,
    connected: Arc<AtomicBool>,
    in_flight: Arc<AtomicUsize>,
    task: Option<JoinHandle<()>>,
}

impl VerifyBridge {
    /// Start a fresh loopback listener with a unique one-time capability token.
    /// Starts with `active_turn = false` so `tools/call` during `session/load` replay is
    /// rejected with `-32000 (No active Chat turn)` while `initialize`/`tools/list` succeed.
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
        let active_turn = Arc::new(AtomicBool::new(false));
        let connected = Arc::new(AtomicBool::new(false));
        let in_flight = Arc::new(AtomicUsize::new(0));
        let task = tokio::spawn(serve(
            listener,
            capability.clone(),
            Arc::new(tools),
            send,
            active_turn.clone(),
            connected.clone(),
            in_flight.clone(),
        ));
        Ok(Self {
            port,
            capability,
            calls,
            active_turn,
            connected,
            in_flight,
            task: Some(task),
        })
    }

    pub fn mcp_server(&self, executable: &Path) -> McpServer {
        McpServer {
            name: stdio_mcp::NAME.to_owned(),
            command: executable.to_path_buf(),
            args: vec!["__chat-mcp".to_owned()],
            env: vec![
                McpEnv {
                    name: PORT_ENV.to_owned(),
                    value: self.port.clone(),
                },
                McpEnv {
                    name: CAPABILITY_ENV.to_owned(),
                    value: self.capability.clone(),
                },
            ],
        }
    }

    /// Arm the bridge for an active Chat turn (`session/prompt` about to start).
    pub fn arm_turn(&self) {
        self.active_turn.store(true, Ordering::SeqCst);
    }

    /// Disarm the bridge immediately on cancellation or turn completion so no late `tools/call`
    /// can be admitted.
    pub fn disarm_turn(&self) {
        self.active_turn.store(false, Ordering::SeqCst);
    }

    /// Whether the agent completed MCP `initialize` or `tools/list` on this turn's bridge.
    pub fn state(&self) -> VerifyBridgeState {
        if self.connected.load(Ordering::Relaxed) {
            VerifyBridgeState::Connected
        } else {
            VerifyBridgeState::NotConnected
        }
    }

    /// Number of bridge tool calls currently in flight.
    pub fn in_flight_calls(&self) -> usize {
        self.in_flight.load(Ordering::SeqCst)
    }

    pub async fn next(&mut self) -> Option<BridgeCall> {
        self.calls.recv().await
    }

    /// Close and retire the turn's bridge listener and token immediately.
    pub async fn stop(mut self) {
        self.disarm_turn();
        if let Some(task) = self.task.take() {
            task.abort();
            let _ = crate::task::joined(task.await);
        }
    }
}

impl Drop for VerifyBridge {
    fn drop(&mut self) {
        self.disarm_turn();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

fn bridge_error(error: io::Error) -> Failure {
    Failure::from(format!("Cannot open the ACP Verify MCP bridge: {error}."))
}

async fn serve(
    listener: TcpListener,
    capability: String,
    tools: Arc<stdio_mcp::Server>,
    calls: mpsc::Sender<BridgeCall>,
    active_turn: Arc<AtomicBool>,
    connected: Arc<AtomicBool>,
    in_flight: Arc<AtomicUsize>,
) {
    let slots = Arc::new(Semaphore::new(MAX_BRIDGE_CONNECTIONS));
    while let Ok((mut stream, _)) = listener.accept().await {
        let Ok(permit) = slots.clone().try_acquire_owned() else {
            continue;
        };
        let capability = capability.clone();
        let tools = tools.clone();
        let calls = calls.clone();
        let active_turn = active_turn.clone();
        let connected = connected.clone();
        let in_flight = in_flight.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let mut presented = vec![0u8; capability.len()];
            let read =
                tokio::time::timeout(CONNECT_TIMEOUT, stream.read_exact(&mut presented)).await;
            if matches!(read, Ok(Ok(_))) && presented == capability.as_bytes() {
                let _ =
                    serve_stdio(stream, &tools, &calls, &active_turn, &connected, &in_flight).await;
            }
        });
    }
}

async fn serve_stdio(
    stream: tokio::net::TcpStream,
    tools: &stdio_mcp::Server,
    calls: &mpsc::Sender<BridgeCall>,
    active_turn: &AtomicBool,
    connected: &AtomicBool,
    in_flight: &AtomicUsize,
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
                stdio_mcp::Answer::Reply(reply) => {
                    connected.store(true, Ordering::Relaxed);
                    reply
                }
                stdio_mcp::Answer::Call {
                    id,
                    name,
                    arguments,
                } => {
                    // Counted before the turn check, as the engine disarms before it counts:
                    // a call is either refused here or seen in flight there, never neither.
                    let guard = InFlightGuard::enter(in_flight);
                    if !active_turn.load(Ordering::SeqCst) {
                        drop(guard);
                        no_active_turn_error(&id)
                    } else if !is_allowlisted(&name) {
                        drop(guard);
                        stdio_mcp::call_error(
                            &id,
                            &format!("Tool {name:?} is not allowlisted on the ACP Verify bridge."),
                        )
                    } else {
                        let _guard = guard;
                        let (reply_tx, receive) = oneshot::channel();
                        if calls
                            .send(BridgeCall {
                                id: id.clone(),
                                name,
                                arguments,
                                reply: reply_tx,
                            })
                            .await
                            .is_err()
                        {
                            no_active_turn_error(&id)
                        } else {
                            match receive.await {
                                Ok(reply) => reply,
                                Err(_) => no_active_turn_error(&id),
                            }
                        }
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

struct InFlightGuard<'a>(&'a AtomicUsize);

impl<'a> InFlightGuard<'a> {
    fn enter(counter: &'a AtomicUsize) -> Self {
        counter.fetch_add(1, Ordering::SeqCst);
        Self(counter)
    }
}

impl Drop for InFlightGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn no_active_turn_error(id: &stdio_mcp::RpcId) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": NO_ACTIVE_TURN_CODE,
            "message": "No active Chat turn."
        }
    })
}

/// Forward the ACP agent's stdio to its one-turn Verify MCP bridge (`artifactize __chat-mcp`).
#[doc(hidden)]
pub fn stdio() -> Result<(), String> {
    let unavailable = || "No ACP Chat Verify bridge is available.".to_owned();
    let port = platform::environment::var_text(PORT_ENV)
        .and_then(|port| port.parse::<u16>().ok())
        .filter(|port| *port != 0)
        .ok_or_else(unavailable)?;
    let capability = platform::environment::var_text(CAPABILITY_ENV).ok_or_else(unavailable)?;
    let mut stream =
        platform::net::connect_loopback(port, CONNECT_TIMEOUT).map_err(|_| unavailable())?;
    stream
        .write_all(capability.as_bytes())
        .map_err(|_| unavailable())?;
    let mut send = stream.try_clone().map_err(|_| unavailable())?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_chat_key_uses_disjoint_slash_namespace_and_deterministic_args_hash() {
        let session: SessionId = "chat-abc".parse().unwrap();
        let key = internal_chat_request_key(&session, 2, "agent-key-1").unwrap();
        assert!(key.as_str().starts_with("chat/"));
        assert_eq!(key.as_str().len(), 37);
        // External RequestKey::from_str forbids '/'.
        assert!(key.as_str().parse::<RequestKey>().is_err());

        let hash_a = canonical_args_hash(&json!({
            "request_key": "k1",
            "scope": { "kind": "all" },
            "speculate": true
        }));
        let hash_b = canonical_args_hash(&json!({
            "speculate": true,
            "scope": { "kind": "all" },
            "request_key": "k2"
        }));
        assert_eq!(hash_a, hash_b);
        let hash_c = canonical_args_hash(&json!({
            "scope": { "kind": "eval", "evalId": "a/b" },
            "speculate": true
        }));
        assert_ne!(hash_a, hash_c);
    }

    #[test]
    fn allowlist_excludes_file_shell_and_plugin_mutation_tools() {
        assert!(is_allowlisted("verify_start"));
        assert!(is_allowlisted("status"));
        assert!(is_allowlisted("plugin_list"));
        for excluded in [
            "read",
            "list",
            "glob",
            "grep",
            "write",
            "edit",
            "run_command",
            "plugin_add",
            "plugin_remove",
        ] {
            assert!(!is_allowlisted(excluded), "{excluded} must be excluded");
        }
    }
}
