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
    task::{JoinHandle, JoinSet},
};
use tokio_util::sync::CancellationToken;

use crate::{
    agent::error::Failure,
    llm::stdio_mcp,
    platform,
    tools::ToolResult,
    types::{RequestKey, SessionId},
    verify_tools,
};

use super::{
    VerifyBridgeState,
    redactor::StreamRedactor,
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

/// Reply returned by the ACP turn engine to one incoming Verify bridge call without exposing
/// raw JSON-RPC frames outside `src/acp/` (`C-ACP-EDGE-ISOLATION`).
pub(crate) enum BridgeReply {
    Result(ToolResult),
    NoActiveTurn,
}

/// One incoming allowlisted MCP tool call parsed and redacted at the ACP Verify bridge edge.
pub(crate) struct BridgeCall {
    pub(super) name: String,
    pub(super) call: Result<verify_tools::Call, String>,
    pub(super) summary: String,
    pub(super) arguments: String,
    pub(super) reply: oneshot::Sender<BridgeReply>,
}

/// Recursively redact every string value and object key inside a JSON value BEFORE serialization
/// so secrets containing characters that JSON escapes (`"`, `\`, newlines) are still matched.
pub(super) fn redact_json_value(value: &Value, redactor: &StreamRedactor) -> Value {
    match value {
        Value::String(text) => Value::String(redactor.redact_all(text)),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| redact_json_value(item, redactor))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, val)| (redactor.redact_all(key), redact_json_value(val, redactor)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// One-turn loopback Verify MCP bridge.
pub struct VerifyBridge {
    port: String,
    capability: String,
    calls: mpsc::Receiver<BridgeCall>,
    active_turn: Arc<AtomicBool>,
    connected: Arc<AtomicBool>,
    in_flight: Arc<AtomicUsize>,
    shutdown: CancellationToken,
    task: Option<JoinHandle<()>>,
}

impl VerifyBridge {
    /// Start a fresh loopback listener with a unique one-time capability token.
    /// Starts with `active_turn = false` so `tools/call` during `session/load` replay is
    /// rejected with `-32000 (No active Chat turn)` while `initialize`/`tools/list` succeed.
    pub(crate) fn start(
        session_id: SessionId,
        send: usize,
        capability: String,
        redactor: StreamRedactor,
        tools: stdio_mcp::Server,
    ) -> Result<Self, Failure> {
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
        let listener = platform::net::exclusive_listener(address).map_err(bridge_error)?;
        let port = listener
            .local_addr()
            .map_err(bridge_error)?
            .port()
            .to_string();
        let (call_tx, calls) = mpsc::channel(QUEUED_CALLS);
        let active_turn = Arc::new(AtomicBool::new(false));
        let connected = Arc::new(AtomicBool::new(false));
        let in_flight = Arc::new(AtomicUsize::new(0));
        let shutdown = CancellationToken::new();
        let task = tokio::spawn(serve(
            listener,
            capability.clone(),
            session_id,
            send,
            redactor,
            Arc::new(tools),
            call_tx,
            active_turn.clone(),
            connected.clone(),
            in_flight.clone(),
            shutdown.clone(),
        ));
        Ok(Self {
            port,
            capability,
            calls,
            active_turn,
            connected,
            in_flight,
            shutdown,
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

    pub(crate) async fn next(&mut self) -> Option<BridgeCall> {
        self.calls.recv().await
    }

    /// Close and retire the turn's bridge listener, accepted sockets, and token immediately.
    /// Idempotent so both normal turn teardown and error paths can await complete shutdown.
    pub async fn stop(&mut self) {
        self.disarm_turn();
        self.shutdown.cancel();
        if let Some(task) = self.task.take() {
            let _ = crate::task::joined(task.await);
        }
    }
}

impl Drop for VerifyBridge {
    fn drop(&mut self) {
        self.disarm_turn();
        self.shutdown.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

fn bridge_error(error: io::Error) -> Failure {
    Failure::from(format!("Cannot open the ACP Verify MCP bridge: {error}."))
}

#[expect(
    clippy::too_many_arguments,
    reason = "The bridge task binds listener, capability, session context, tools, channel, turn flags and shutdown token"
)]
async fn serve(
    listener: TcpListener,
    capability: String,
    session_id: SessionId,
    send: usize,
    redactor: StreamRedactor,
    tools: Arc<stdio_mcp::Server>,
    calls: mpsc::Sender<BridgeCall>,
    active_turn: Arc<AtomicBool>,
    connected: Arc<AtomicBool>,
    in_flight: Arc<AtomicUsize>,
    shutdown: CancellationToken,
) {
    let slots = Arc::new(Semaphore::new(MAX_BRIDGE_CONNECTIONS));
    let capability_consumed = Arc::new(AtomicBool::new(false));
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            biased;
            () = shutdown.cancelled() => break,
            Some(_) = connections.join_next(), if !connections.is_empty() => {}
            accepted = listener.accept() => {
                let Ok((mut stream, _)) = accepted else {
                    break;
                };
                let Ok(permit) = slots.clone().try_acquire_owned() else {
                    continue;
                };
                let capability = capability.clone();
                let capability_consumed = capability_consumed.clone();
                let session_id = session_id.clone();
                let redactor = redactor.clone();
                let tools = tools.clone();
                let calls = calls.clone();
                let active_turn = active_turn.clone();
                let connected = connected.clone();
                let in_flight = in_flight.clone();
                let shutdown = shutdown.clone();
                connections.spawn(async move {
                    let _permit = permit;
                    let mut presented = vec![0u8; capability.len()];
                    let read = tokio::select! {
                        biased;
                        () = shutdown.cancelled() => return,
                        res = tokio::time::timeout(CONNECT_TIMEOUT, stream.read_exact(&mut presented)) => res,
                    };
                    // Strictly consume the one-time capability token on first valid authentication
                    // so no second peer can ever authenticate with it (`C-ONE-TURN-OWNERSHIP`).
                    if matches!(read, Ok(Ok(_)))
                        && presented == capability.as_bytes()
                        && !capability_consumed.swap(true, Ordering::SeqCst)
                    {
                        let _ = serve_stdio(
                            stream,
                            &session_id,
                            send,
                            &redactor,
                            &tools,
                            &calls,
                            &active_turn,
                            &connected,
                            &in_flight,
                            &shutdown,
                        )
                        .await;
                    }
                });
            }
        }
    }
    drop(listener);
    connections.abort_all();
    while connections.join_next().await.is_some() {}
}

fn parse_bridge_call(
    session_id: &SessionId,
    send: usize,
    name: &str,
    arguments: Value,
    raw_request_key: Option<String>,
    raw_args_hash: String,
) -> Result<verify_tools::Call, String> {
    let mut map = match arguments {
        Value::Object(map) => map,
        _ => return Err("Tool arguments must be an object.".into()),
    };
    if raw_request_key.is_some() {
        // Removed before `Call::parse` so a redacted `[REDACTED]` value does not trip
        // `RequestKey` syntax validation; `remap_acp_chat_key` validates and hashes the raw key.
        map.remove("request_key");
        map.remove("requestKey");
    }
    let mut call = verify_tools::Call::parse(name, map, time::OffsetDateTime::now_utc())
        .map_err(|error| error.to_string())?;
    if call.project.is_some() {
        return Err("Chat Verify tools are scoped to this session's worktree.".into());
    }
    call.remap_acp_chat_key(session_id, send, raw_request_key, raw_args_hash)?;
    Ok(call)
}

#[expect(
    clippy::too_many_arguments,
    reason = "Each authenticated bridge socket needs session context, redactor, tools, channel and turn flags"
)]
async fn serve_stdio(
    stream: tokio::net::TcpStream,
    session_id: &SessionId,
    send: usize,
    redactor: &StreamRedactor,
    tools: &stdio_mcp::Server,
    calls: &mpsc::Sender<BridgeCall>,
    active_turn: &AtomicBool,
    connected: &AtomicBool,
    in_flight: &AtomicUsize,
    shutdown: &CancellationToken,
) -> io::Result<()> {
    let (input, mut output) = stream.into_split();
    let mut input = BufReader::new(input);
    loop {
        let mut line = Vec::new();
        let read = {
            let mut limited = (&mut input).take(LINE_BYTES + 1);
            tokio::select! {
                biased;
                () = shutdown.cancelled() => return Ok(()),
                res = limited.read_until(b'\n', &mut line) => res?,
            }
        };
        if read == 0 {
            return Ok(());
        }
        if line.len() as u64 > LINE_BYTES {
            return Err(io::ErrorKind::InvalidData.into());
        }
        let request = serde_json::from_slice(&line);
        line.clear();
        // Hold the in-flight guard across the full reply write so quiescence never observes
        // zero while a tool reply is still being written to the socket.
        let mut _in_flight_guard = None;
        let reply = match request {
            Ok(stdio_mcp::Rpc::Unknown { id: None, .. }) => continue,
            Ok(rpc) => match tools.answer(rpc) {
                stdio_mcp::Answer::Reply(reply) => {
                    if shutdown.is_cancelled() {
                        return Ok(());
                    }
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
                    if !active_turn.load(Ordering::SeqCst) || shutdown.is_cancelled() {
                        drop(guard);
                        no_active_turn_error(&id)
                    } else if !is_allowlisted(&name) {
                        drop(guard);
                        stdio_mcp::call_error(
                            &id,
                            &format!("Tool {name:?} is not allowlisted on the ACP Verify bridge."),
                        )
                    } else {
                        _in_flight_guard = Some(guard);
                        // Hash the raw agent-supplied arguments and extract any raw `request_key`
                        // BEFORE redacting so distinct calls whose differing values redact to the
                        // same text still detect key conflicts and valid secret-bearing keys still
                        // hash cleanly into `chat/*` (`C-ONE-TURN-OWNERSHIP`). Derive display
                        // arguments solely from the parsed typed `verify_tools::Call` so no raw
                        // JSON payload ever leaves `src/acp/` (`C-ACP-EDGE-ISOLATION`).
                        let raw_args_hash = canonical_args_hash(&arguments);
                        let raw_request_key = arguments
                            .as_object()
                            .and_then(|map| {
                                map.get("request_key").or_else(|| map.get("requestKey"))
                            })
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned);
                        let redacted_value = redact_json_value(&arguments, redactor);
                        let parsed = parse_bridge_call(
                            session_id,
                            send,
                            &name,
                            redacted_value,
                            raw_request_key,
                            raw_args_hash,
                        );
                        let (summary, redacted_args) = match &parsed {
                            Ok(typed_call) => {
                                let typed_args =
                                    redactor.redact_all(&typed_call.display_arguments());
                                let summary_full = format!("{name} {typed_args}");
                                let summary = redactor.redact_all(
                                    &summary_full.chars().take(120).collect::<String>(),
                                );
                                (summary, typed_args)
                            }
                            Err(_) => (name.clone(), String::new()),
                        };
                        let (reply_tx, receive) = oneshot::channel();
                        if calls
                            .send(BridgeCall {
                                name,
                                call: parsed,
                                summary,
                                arguments: redacted_args,
                                reply: reply_tx,
                            })
                            .await
                            .is_err()
                        {
                            no_active_turn_error(&id)
                        } else {
                            tokio::select! {
                                biased;
                                () = shutdown.cancelled() => return Ok(()),
                                res = receive => match res {
                                    Ok(BridgeReply::Result(result)) => stdio_mcp::call_reply(&id, result),
                                    Ok(BridgeReply::NoActiveTurn) | Err(_) => no_active_turn_error(&id),
                                },
                            }
                        }
                    }
                }
            },
            Err(_) => return Err(io::ErrorKind::InvalidData.into()),
        };
        serde_json::to_writer(&mut line, &reply)?;
        line.push(b'\n');
        tokio::select! {
            biased;
            () = shutdown.cancelled() => return Ok(()),
            res = output.write_all(&line) => res?,
        }
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

    #[tokio::test]
    async fn stop_retires_accepted_socket_connections() {
        let session: SessionId = "chat-bridge-test".parse().unwrap();
        let mut bridge = VerifyBridge::start(
            session,
            1,
            "one-time-cap-token".to_owned(),
            StreamRedactor::new(Vec::<String>::new()),
            stdio_mcp::Server::new(std::iter::empty()),
        )
        .unwrap();
        let port: u16 = bridge.port.parse().unwrap();
        let capability = bridge.capability.clone();
        let mut client = tokio::net::TcpStream::connect((Ipv4Addr::LOCALHOST, port))
            .await
            .unwrap();
        client.write_all(capability.as_bytes()).await.unwrap();
        // Send initialize and read reply while bridge is live.
        let init = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n";
        client.write_all(init).await.unwrap();
        let mut reader = BufReader::new(client);
        let mut reply = String::new();
        assert!(reader.read_line(&mut reply).await.unwrap() > 0);
        assert!(reply.contains("\"id\":1"), "{reply}");

        // Retiring the bridge aborts accepted socket tasks so further non-tool requests get EOF.
        bridge.stop().await;
        let mut client = reader.into_inner();
        let _ = client
            .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\",\"params\":{}}\n")
            .await;
        let mut reader = BufReader::new(client);
        let mut after = String::new();
        let read = reader.read_line(&mut after).await;
        assert!(
            matches!(read, Ok(0) | Err(_)),
            "expected EOF or reset after stop, got {read:?} ({after:?})"
        );
    }

    #[test]
    fn redact_json_value_catches_secrets_with_json_escaped_characters() {
        let secret = "tok-\"quote\"\\slash\nline";
        let redactor = StreamRedactor::new([secret.to_owned()]);
        let raw = json!({ "note": format!("before {secret} after") });
        let serialized = redact_json_value(&raw, &redactor).to_string();
        assert!(!serialized.contains("quote"), "{serialized}");
        assert!(serialized.contains("[REDACTED]"), "{serialized}");
    }
}
