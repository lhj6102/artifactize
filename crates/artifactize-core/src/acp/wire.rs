//! Owned ACP v1 JSON-RPC 2.0 stdio wire layer (`T-PARSE-AT-EDGE`, `C-ACP-EDGE-ISOLATION`).
//!
//! Enforces strict FIFO receive-order barriers via a single stdout reader task, bounds all
//! incoming payloads, normalizes `configOptions` and legacy `models`/`modes` at the edge,
//! rejects unknown server requests with `-32601`, and validates ACP v1 contract shapes.

use std::{
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{ChildStdin, ChildStdout},
    sync::mpsc,
    task::JoinHandle,
};

use crate::{
    chat::ToolKind,
    llm::{models::ListedModel, stdio_mcp::RpcId},
    platform,
};

use super::redactor::StreamRedactor;

use super::{
    ConfigCategory, ConfigChoice, ConfigOption, ConfigSelection, PermissionOption,
    PermissionOptionKind, PlanStep, PlanStepStatus, Problem, ToolDiff, ToolLocation, ToolStatus,
};

/// Pinned ACP protocol version.
pub const PROTOCOL_VERSION: u32 = 1;
/// ACP authentication-required JSON-RPC error code.
const AUTH_REQUIRED: i64 = -32000;
/// Timeout for the ACP `initialize` handshake.
pub const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(15);
/// Timeout for `session/new` and `session/load`.
pub const SESSION_OPEN_TIMEOUT: Duration = Duration::from_secs(30);
/// Timeout for `session/set_config_option` and legacy model/mode setters.
pub const CONFIG_TIMEOUT: Duration = Duration::from_secs(10);
/// Grace window to drain stdout after sending `session/cancel` before terminating the child.
pub const PROMPT_CANCEL_GRACE: Duration = Duration::from_secs(2);
/// Best-effort write timeout for the `session/cancel` notification itself.
const CANCEL_WRITE_GRACE: Duration = Duration::from_millis(250);
/// Maximum size of one JSON-RPC line on ACP stdio.
pub const LINE_BYTES: u64 = 32 * 1024 * 1024;
/// Maximum persisted tool output per tool call.
pub const TOOL_OUTPUT_BYTES: usize = 256 * 1024;
/// Maximum persisted inline diff size per tool call.
pub const DIFF_BYTES: usize = 512 * 1024;
/// Maximum persisted final answer text per turn.
pub const FINAL_ANSWER_BYTES: usize = 1024 * 1024;
/// Maximum total persisted bytes of display events per turn.
pub const TURN_PERSISTED_BYTES: usize = 8 * 1024 * 1024;
/// Maximum number of display events persisted per turn.
pub const TURN_DISPLAY_EVENTS: usize = 2_000;
/// Maximum concurrent pending permission prompts per turn.
pub const MAX_PENDING_PERMISSIONS: usize = 16;
/// Maximum size of sanitized permission detail text persisted in SQLite.
pub const PERMISSION_DETAIL_BYTES: usize = 16 * 1024;
/// Bounded FIFO channel capacity between the stdout reader task and the turn/connection owner.
pub const UPDATE_CHANNEL_CAPACITY: usize = 256;
/// Maximum buffered config/mode updates accumulated while awaiting an ACP control response.
const MAX_CONFIG_UPDATES: usize = 64;
/// Bounded deadline for writing a JSON-RPC frame to the child's stdin pipe.
const STDIN_WRITE_TIMEOUT: Duration = Duration::from_secs(10);
/// Some Antigravity CLI versions print this line before returning an auth error.
pub const AUTH_LINK_MARKER: &str = "Open the following link to authenticate the ACP server:";

static NEXT_CONNECTION_ID: AtomicU64 = AtomicU64::new(1);

/// MCP server descriptor sent in `session/new` and `session/load`.
#[derive(Debug, Clone, Serialize)]
pub struct McpServer {
    pub name: String,
    #[serde(serialize_with = "crate::platform::path_serde::native::serialize_path")]
    pub command: std::path::PathBuf,
    pub args: Vec<String>,
    pub env: Vec<McpEnv>,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpEnv {
    pub name: String,
    pub value: String,
}

/// Typed agent capabilities extracted from `initialize`.
#[derive(Debug, Clone, Default)]
pub struct AgentCapabilities {
    pub load_session: bool,
    pub logout_supported: bool,
    pub system_prompt_append: bool,
}

/// Typed result of `initialize`.
#[derive(Debug, Clone)]
pub struct Initialized {
    pub capabilities: AgentCapabilities,
}

/// Typed result of `session/new` or `session/load`.
#[derive(Debug, Clone)]
pub struct OpenedSession {
    pub session_id: String,
    pub config_options: Vec<ConfigOption>,
    pub uses_legacy_models: bool,
    pub uses_legacy_modes: bool,
}

/// Typed ACP prompt completion reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    Cancelled,
    Refusal,
    MaxTokens,
    MaxTurnRequests,
    #[serde(other)]
    Other,
}

/// Typed ACP `session/request_permission` payload parsed at the edge.
#[derive(Debug, Clone)]
pub struct IncomingPermission {
    pub rpc_id: RpcId,
    pub session_id: String,
    pub title: String,
    pub tool_kind: ToolKind,
    pub summary: String,
    pub detail: Option<String>,
    pub options: Vec<PermissionOption>,
}

/// Typed ACP `session/update` event normalized at the edge (no raw `serde_json::Value`).
#[derive(Debug, Clone)]
pub enum SessionEvent {
    AgentMessageChunk {
        text: String,
    },
    AgentThoughtChunk {
        text: String,
    },
    ToolCall {
        call_id: String,
        title: String,
        tool_kind: ToolKind,
        status: ToolStatus,
        summary: String,
        locations: Vec<ToolLocation>,
        output: Option<String>,
        diff: Option<ToolDiff>,
        output_truncated: bool,
    },
    ToolCallUpdate {
        call_id: String,
        title: Option<String>,
        tool_kind: Option<ToolKind>,
        status: Option<ToolStatus>,
        summary: Option<String>,
        locations: Option<Vec<ToolLocation>>,
        output: Option<String>,
        diff: Option<ToolDiff>,
        output_truncated: bool,
    },
    Plan {
        steps: Vec<PlanStep>,
    },
    ConfigOptionsUpdated {
        options: Vec<ConfigOption>,
    },
    ModeChanged {
        mode_id: String,
    },
    Notice {
        text: String,
    },
}

/// Replay suppression state bound to `(connection_id, load_request_id, remote_session_id)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadPhase {
    pub connection_id: u64,
    pub load_request_id: u64,
    pub remote_session_id: String,
}

/// Outcome of applying saved `configOptions` in dependency order before `session/prompt`.
#[derive(Debug, Clone)]
pub struct AppliedConfig {
    pub live_options: Vec<ConfigOption>,
    pub effective_selections: Vec<ConfigSelection>,
    pub notices: Vec<String>,
}

/// Frames delivered in strict FIFO receive order from the single stdout reader task.
#[derive(Debug)]
pub enum WireFrame {
    Update {
        session_id: String,
        event: SessionEvent,
    },
    PermissionRequest(IncomingPermission),
    UnknownServerRequest {
        id: RpcId,
        #[cfg(test)]
        method: String,
    },
    Response {
        id: u64,
        result: Value,
    },
    Error {
        id: u64,
        code: i64,
        message: String,
        data: String,
    },
    SignedOut,
    Eof,
    ProtocolError(String),
}

/// Owned ACP JSON-RPC 2.0 stdio client with strict FIFO stdout reader barrier.
pub struct Connection {
    id: u64,
    agent_name: String,
    auth_hint: String,
    redactor: StreamRedactor,
    write: ChildStdin,
    frames: mpsc::Receiver<WireFrame>,
    buffered_frames: std::collections::VecDeque<WireFrame>,
    reader_task: JoinHandle<()>,
    next_request_id: u64,
    replaying: Option<LoadPhase>,
    capabilities: Option<AgentCapabilities>,
    /// Option and mode changes the agent reported while a configuration request waited for its
    /// answer, for [`Self::apply_config_options`] to take into account.
    config_updates: Vec<(String, SessionEvent)>,
}

impl Connection {
    pub fn new(
        agent_name: impl Into<String>,
        auth_hint: impl Into<String>,
        stdout: ChildStdout,
        stdin: ChildStdin,
        hide_always_options: bool,
        known_secrets: Vec<String>,
    ) -> Self {
        let id = NEXT_CONNECTION_ID.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel(UPDATE_CHANNEL_CAPACITY);
        let redactor = StreamRedactor::new(known_secrets);
        let reader_task = tokio::spawn(stdout_reader_loop(
            stdout,
            tx,
            hide_always_options,
            redactor.clone(),
        ));
        Self {
            id,
            agent_name: agent_name.into(),
            auth_hint: auth_hint.into(),
            redactor,
            write: stdin,
            frames: rx,
            buffered_frames: std::collections::VecDeque::new(),
            reader_task,
            next_request_id: 0,
            replaying: None,
            capabilities: None,
            config_updates: Vec::new(),
        }
    }

    /// Whether the parked child has stayed completely silent since the previous turn ended:
    /// reader task alive, no queued or buffered unsolicited frames/requests, and channel open.
    pub fn is_quiescent(&self) -> bool {
        !self.reader_task.is_finished()
            && self.buffered_frames.is_empty()
            && self.frames.is_empty()
            && !self.frames.is_closed()
    }

    /// Non-blockingly drain any config/mode updates already queued on stdout for `session_id`
    /// before approving configuration or sending `session/prompt` (`C-NO-SILENT-REGRESSION`).
    /// Non-config frames are preserved in FIFO order in `buffered_frames`, strictly bounded by
    /// `UPDATE_CHANNEL_CAPACITY` (`C-ACP-EDGE-ISOLATION`).
    pub(crate) fn drain_pending_config_updates(
        &mut self,
        session_id: &str,
        live: &mut Vec<ConfigOption>,
    ) -> Result<(), Problem> {
        let mut kept = std::collections::VecDeque::new();
        while let Some(frame) = self.buffered_frames.pop_front() {
            if let Some(other) = self.apply_or_keep_config_frame(session_id, live, frame)? {
                kept.push_back(other);
            }
        }
        for _ in 0..UPDATE_CHANNEL_CAPACITY {
            match self.frames.try_recv() {
                Ok(frame) => {
                    if let Some(other) = self.apply_or_keep_config_frame(session_id, live, frame)? {
                        if kept.len() >= UPDATE_CHANNEL_CAPACITY {
                            self.buffered_frames = kept;
                            return Err(Problem::Failed(format!(
                                "{} flooded ACP output before starting the turn.",
                                self.agent_name
                            )));
                        }
                        kept.push_back(other);
                    }
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    self.buffered_frames = kept;
                    return Err(Problem::Failed(format!(
                        "{} exited before starting the ACP turn.",
                        self.agent_name
                    )));
                }
            }
        }
        self.buffered_frames = kept;
        Ok(())
    }

    fn apply_or_keep_config_frame(
        &self,
        session_id: &str,
        live: &mut Vec<ConfigOption>,
        frame: WireFrame,
    ) -> Result<Option<WireFrame>, Problem> {
        match frame {
            WireFrame::Update {
                session_id: update_sid,
                event: SessionEvent::ConfigOptionsUpdated { options },
            } if update_sid == session_id => {
                *live = options;
                Ok(None)
            }
            WireFrame::Update {
                session_id: update_sid,
                event: SessionEvent::ModeChanged { mode_id },
            } if update_sid == session_id => {
                if let Some(mode) = live
                    .iter_mut()
                    .find(|opt| opt.category == ConfigCategory::Mode)
                {
                    mode.current_value = mode_id;
                }
                Ok(None)
            }
            WireFrame::SignedOut => Err(Problem::SignedOut {
                agent: self.agent_name.clone(),
                hint: self.auth_hint.clone(),
            }),
            WireFrame::Eof => Err(Problem::Failed(format!(
                "{} exited before starting the ACP turn.",
                self.agent_name
            ))),
            WireFrame::ProtocolError(msg) => Err(Problem::Failed(self.redactor.redact_all(&msg))),
            other => Ok(Some(other)),
        }
    }

    pub fn capabilities(&self) -> Option<&AgentCapabilities> {
        self.capabilities.as_ref()
    }

    pub async fn initialize(&mut self) -> Result<Initialized, Problem> {
        let result: RawInitialized = self
            .request_with_timeout("initialize", initialization_params(), INITIALIZE_TIMEOUT)
            .await?;
        if result.protocol_version != PROTOCOL_VERSION {
            return Err(Problem::Failed(format!(
                "{} offered unsupported ACP version {} (expected {PROTOCOL_VERSION}).",
                self.agent_name, result.protocol_version
            )));
        }
        let capabilities = AgentCapabilities {
            load_session: result.agent_capabilities.load_session,
            logout_supported: result.agent_capabilities.auth.logout.is_some(),
            system_prompt_append: result
                .agent_capabilities
                .meta
                .antigravity
                .system_prompt
                .append
                || result.agent_capabilities.meta.system_prompt.append,
        };
        self.capabilities = Some(capabilities.clone());
        Ok(Initialized { capabilities })
    }

    pub async fn session_new(
        &mut self,
        cwd: &Path,
        mcp_servers: &[McpServer],
        system_append: Option<&str>,
    ) -> Result<OpenedSession, Problem> {
        let mut params =
            serde_json::to_value(NewSessionParams { cwd, mcp_servers }).map_err(|error| {
                Problem::Failed(format!(
                    "Cannot encode ACP session/new paths for {}: {error}",
                    self.agent_name
                ))
            })?;
        if let Some(system) = system_append {
            params["_meta"] = json!({
                "antigravity": { "systemPrompt": { "append": system } },
                "systemPrompt": { "append": system }
            });
        }
        let raw: RawSession = self
            .request_with_timeout("session/new", params, SESSION_OPEN_TIMEOUT)
            .await?;
        Ok(normalize_session(raw))
    }

    /// Call `session/load` with replay suppression bound to `(connection_id, load_request_id, remote_session_id)`.
    pub async fn session_load(
        &mut self,
        remote_session_id: &str,
        cwd: &Path,
        mcp_servers: &[McpServer],
    ) -> Result<OpenedSession, Problem> {
        let params = serde_json::to_value(LoadSessionParams {
            session_id: remote_session_id,
            cwd,
            mcp_servers,
        })
        .map_err(|error| {
            Problem::Failed(format!(
                "Cannot encode ACP session/load params for {}: {error}",
                self.agent_name
            ))
        })?;
        let request_id = self.send_request("session/load", params).await?;
        self.replaying = Some(LoadPhase {
            connection_id: self.id,
            load_request_id: request_id,
            remote_session_id: remote_session_id.to_owned(),
        });
        let outcome = tokio::time::timeout(SESSION_OPEN_TIMEOUT, async {
            loop {
                let frame = self.next_raw_frame("session/load").await?;
                match frame {
                    WireFrame::Update { session_id, .. }
                        if self.is_suppressed_replay(&session_id, request_id) =>
                    {
                        // Suppress historical replay updates until `session/load` completes.
                        continue;
                    }
                    WireFrame::UnknownServerRequest { id, .. } => {
                        self.send_method_not_found(id).await?;
                    }
                    WireFrame::PermissionRequest(req) => {
                        // Never grant permission during session/load replay.
                        self.send_permission_cancelled(req.rpc_id).await?;
                    }
                    WireFrame::Response { id, result } if id == request_id => {
                        let raw: RawSessionLoadResult =
                            serde_json::from_value(result).map_err(|_| {
                                Problem::Failed(format!(
                                    "{} returned an invalid ACP session/load response.",
                                    self.agent_name
                                ))
                            })?;
                        return Ok(normalize_session(RawSession {
                            session_id: raw
                                .session_id
                                .unwrap_or_else(|| remote_session_id.to_owned()),
                            config_options: raw.config_options,
                            models: raw.models,
                            modes: raw.modes,
                        }));
                    }
                    WireFrame::Error {
                        id,
                        code,
                        message,
                        data,
                    } if id == request_id => {
                        return Err(self.rpc_error("session/load", code, &message, &data));
                    }
                    WireFrame::SignedOut => {
                        return Err(Problem::SignedOut {
                            agent: self.agent_name.clone(),
                            hint: self.auth_hint.clone(),
                        });
                    }
                    WireFrame::Eof => {
                        return Err(Problem::Failed(format!(
                            "{} exited while handling ACP session/load.",
                            self.agent_name
                        )));
                    }
                    WireFrame::ProtocolError(msg) => {
                        return Err(Problem::Failed(self.redactor.redact_all(&msg)));
                    }
                    _ => {}
                }
            }
        })
        .await;
        self.replaying = None;
        outcome.unwrap_or_else(|_| {
            Err(Problem::Failed(format!(
                "{} timed out while handling ACP session/load.",
                self.agent_name
            )))
        })
    }

    pub async fn set_config_option(
        &mut self,
        session_id: &str,
        config_id: &str,
        value: &str,
    ) -> Result<Vec<ConfigOption>, Problem> {
        let updated: RawConfigOptionsReply = self
            .request_with_timeout(
                "session/set_config_option",
                json!({
                    "sessionId": session_id,
                    "configId": config_id,
                    "value": value,
                }),
                CONFIG_TIMEOUT,
            )
            .await?;
        Ok(normalize_config_options(updated.config_options))
    }

    pub async fn set_legacy_model(
        &mut self,
        session_id: &str,
        model_id: &str,
    ) -> Result<(), Problem> {
        let _: Value = self
            .request_with_timeout(
                "session/set_model",
                json!({ "sessionId": session_id, "modelId": model_id }),
                CONFIG_TIMEOUT,
            )
            .await?;
        Ok(())
    }

    pub async fn set_legacy_mode(
        &mut self,
        session_id: &str,
        mode_id: &str,
    ) -> Result<Option<String>, Problem> {
        let request_id = self
            .send_request(
                "session/set_mode",
                json!({ "sessionId": session_id, "modeId": mode_id }),
            )
            .await?;
        let mut observed_mode: Option<String> = None;
        let outcome = tokio::time::timeout(CONFIG_TIMEOUT, async {
            loop {
                match self.next_raw_frame("session/set_mode").await? {
                    WireFrame::UnknownServerRequest { id, .. } => {
                        self.send_method_not_found(id).await?;
                    }
                    WireFrame::PermissionRequest(req) => {
                        self.send_permission_cancelled(req.rpc_id).await?;
                    }
                    WireFrame::Update {
                        session_id: update_sid,
                        event,
                    } if update_sid == session_id => match event {
                        SessionEvent::ModeChanged { ref mode_id } => {
                            observed_mode = (!mode_id.is_empty()).then(|| mode_id.clone());
                            self.record_config_update(update_sid, event);
                        }
                        // The newest list says which mode is current, or that none is.
                        SessionEvent::ConfigOptionsUpdated { ref options } => {
                            observed_mode = options
                                .iter()
                                .find(|opt| opt.category == ConfigCategory::Mode)
                                .map(|opt| opt.current_value.clone());
                            self.record_config_update(update_sid, event);
                        }
                        _ => {}
                    },
                    WireFrame::Response { id, result } if id == request_id => {
                        return Ok(extract_confirmed_legacy_mode(&result).or(observed_mode));
                    }
                    WireFrame::Error {
                        id,
                        code,
                        message,
                        data,
                    } if id == request_id => {
                        return Err(self.rpc_error("session/set_mode", code, &message, &data));
                    }
                    WireFrame::SignedOut => {
                        return Err(Problem::SignedOut {
                            agent: self.agent_name.clone(),
                            hint: self.auth_hint.clone(),
                        });
                    }
                    WireFrame::Eof => {
                        return Err(Problem::Failed(format!(
                            "{} exited while handling ACP session/set_mode.",
                            self.agent_name
                        )));
                    }
                    WireFrame::ProtocolError(msg) => {
                        return Err(Problem::Failed(self.redactor.redact_all(&msg)));
                    }
                    WireFrame::Update { .. }
                    | WireFrame::Response { .. }
                    | WireFrame::Error { .. } => {}
                }
            }
        })
        .await;
        outcome.unwrap_or_else(|_| {
            Err(Problem::Failed(format!(
                "{} timed out while handling ACP session/set_mode.",
                self.agent_name
            )))
        })
    }

    /// Apply user-selected `configOptions` in dependency order:
    /// `model` -> refresh live options -> `mode` -> `thought_level` -> rest.
    ///
    /// Falls back with a `Notice` only when a previously saved `model` or `thought_level`
    /// disappeared from the live list; **aborts with `Problem::ModeMismatch`** if a user-selected
    /// `mode` is missing or mismatched (never silently downgrades approval level).
    pub async fn apply_config_options(
        &mut self,
        session: &OpenedSession,
        requested: &[ConfigSelection],
    ) -> Result<AppliedConfig, Problem> {
        let mut live = session.config_options.clone();
        self.config_updates.clear();
        self.drain_pending_config_updates(&session.session_id, &mut live)?;
        let mut notices = Vec::new();
        // Each selection's category as first read: a mode stays a mode even if a later refresh
        // drops the option that named it.
        // Whether the kind is known at all: saved with the selection or read off a live option.
        let mut ordered: Vec<(u8, ConfigCategory, bool, &ConfigSelection)> = requested
            .iter()
            .map(|sel| {
                let known = sel.category.or_else(|| {
                    live.iter()
                        .find(|opt| opt.id == sel.id)
                        .map(|opt| opt.category)
                });
                let category = known.unwrap_or_else(|| infer_category(&sel.id, None));
                (category.apply_rank(), category, known.is_some(), sel)
            })
            .collect();
        ordered.sort_by_key(|(rank, _, _, _)| *rank);

        let mut effective = Vec::new();
        // The modes asked for, by option ID: a later setter can replace the whole list.
        let mut modes = Vec::new();
        for (_, first_category, known, selection) in ordered {
            let refreshed = selection
                .category
                .or_else(|| {
                    live.iter()
                        .find(|opt| opt.id == selection.id)
                        .map(|opt| opt.category)
                })
                .unwrap_or_else(|| infer_category(&selection.id, None));
            let inferred_cat = if first_category == ConfigCategory::Mode {
                ConfigCategory::Mode
            } else {
                refreshed
            };

            // Match the exact selected option ID first. Only non-Mode options may fall back to
            // matching by category (e.g. a migrated legacy `model` / `thought_level` selection);
            // a Mode selection never silently substitutes another Mode option (`C-NO-SILENT-REGRESSION`).
            let Some(current_opt) = live
                .iter()
                .find(|opt| opt.id == selection.id)
                .or_else(|| {
                    (inferred_cat != ConfigCategory::Mode)
                        .then_some(selection.category)
                        .flatten()
                        .filter(|cat| *cat != ConfigCategory::Other)
                        .and_then(|cat| live.iter().find(|opt| opt.category == cat))
                })
                .cloned()
            else {
                if inferred_cat == ConfigCategory::Mode {
                    return Err(Problem::ModeMismatch {
                        requested: selection.value.clone(),
                        actual: None,
                    });
                }
                // Neither saved nor advertised with a kind, it could have been the mode the
                // person chose; the turn stops rather than run under the agent default.
                if !known {
                    return Err(Problem::Failed(format!(
                        "{} no longer offers the saved option {:?}, which could have been its mode; choose the agent's options again.",
                        self.agent_name, selection.id
                    )));
                }
                notices.push(format!(
                    "Saved option {:?} is no longer advertised by {}; using the agent default.",
                    selection.id, self.agent_name
                ));
                continue;
            };

            // A mode is satisfied only by an option the agent calls a mode; another option with
            // the same ID never stands in for it. Likewise, a legacy saved selection with no
            // recorded category (whose ID is not unambiguously model/thought_level) could have
            // been saved as a mode: refuse to let the agent reclassify it as non-mode.
            if inferred_cat == ConfigCategory::Mode && current_opt.category != ConfigCategory::Mode
            {
                return Err(Problem::ModeMismatch {
                    requested: selection.value.clone(),
                    actual: None,
                });
            }
            if selection.category.is_none()
                && current_opt.category != ConfigCategory::Mode
                && !matches!(
                    infer_category(&selection.id, None),
                    ConfigCategory::Model | ConfigCategory::ThoughtLevel
                )
            {
                return Err(Problem::Failed(format!(
                    "Saved option {:?} on {} has no recorded category and may have been a mode; choose the agent's options again.",
                    selection.id, self.agent_name
                )));
            }
            let is_mode = current_opt.category == ConfigCategory::Mode;
            let choice_exists = current_opt
                .choices
                .iter()
                .any(|choice| choice.value == selection.value);
            if !choice_exists {
                if is_mode {
                    return Err(Problem::ModeMismatch {
                        requested: selection.value.clone(),
                        actual: Some(current_opt.current_value.clone()),
                    });
                }
                notices.push(format!(
                    "Saved {} {:?} is no longer available on {}; fell back to {:?}.",
                    current_opt.name, selection.value, self.agent_name, current_opt.current_value
                ));
                effective.push(ConfigSelection {
                    id: current_opt.id.clone(),
                    value: current_opt.current_value.clone(),
                    category: Some(current_opt.category),
                });
                continue;
            }

            if current_opt.current_value != selection.value {
                if session.uses_legacy_models && current_opt.category == ConfigCategory::Model {
                    self.set_legacy_model(&session.session_id, &selection.value)
                        .await?;
                    if let Some(opt) = live.iter_mut().find(|opt| opt.id == current_opt.id) {
                        opt.current_value = selection.value.clone();
                    }
                    self.absorb_config_updates(&session.session_id, &mut live);
                } else if session.uses_legacy_modes && is_mode {
                    let confirmed = self
                        .set_legacy_mode(&session.session_id, &selection.value)
                        .await?;
                    if confirmed.as_deref() != Some(selection.value.as_str()) {
                        return Err(Problem::ModeMismatch {
                            requested: selection.value.clone(),
                            actual: confirmed,
                        });
                    }
                    if let Some(opt) = live.iter_mut().find(|opt| opt.id == current_opt.id) {
                        opt.current_value = selection.value.clone();
                    }
                    self.absorb_config_updates(&session.session_id, &mut live);
                } else {
                    live = self
                        .set_config_option(&session.session_id, &current_opt.id, &selection.value)
                        .await?;
                    // The answer is newer than anything reported while it was awaited.
                    self.config_updates.clear();
                    let confirmed = live
                        .iter()
                        .find(|opt| opt.id == current_opt.id)
                        .map(|opt| opt.current_value.as_str());
                    if confirmed != Some(selection.value.as_str()) {
                        if is_mode {
                            return Err(Problem::ModeMismatch {
                                requested: selection.value.clone(),
                                actual: confirmed.map(str::to_owned),
                            });
                        }
                        return Err(Problem::Failed(format!(
                            "{} did not confirm {} = {:?}.",
                            self.agent_name, current_opt.id, selection.value
                        )));
                    }
                }
            }
            if is_mode {
                modes.push((current_opt.id.clone(), selection.value.clone()));
            }
            effective.push(ConfigSelection {
                id: current_opt.id,
                value: selection.value.clone(),
                category: Some(current_opt.category),
            });
        }
        // Drain any mode or config updates already queued on stdout before approving the final
        // configuration so a late mode change after `session/new` / `session/load` or setter
        // aborts before `session/prompt` is sent (`C-NO-SILENT-REGRESSION`).
        self.drain_pending_config_updates(&session.session_id, &mut live)?;
        for (id, requested) in modes {
            let actual = live
                .iter()
                .find(|opt| opt.id == id && opt.category == ConfigCategory::Mode)
                .map(|opt| opt.current_value.clone());
            if actual.as_deref() != Some(requested.as_str()) {
                return Err(Problem::ModeMismatch { requested, actual });
            }
        }
        // Recheck already-confirmed non-mode options (model, thought level, etc.): a later setter
        // or update may replace the full live option list and reset an earlier choice, which must
        // surface explicitly rather than proceed silently.
        notices.extend(reconcile_non_mode_options(
            &self.agent_name,
            &live,
            &mut effective,
        ));

        Ok(AppliedConfig {
            live_options: live,
            effective_selections: effective,
            notices,
        })
    }

    /// Take in what the agent reported for `session_id` while a legacy setter waited: a replaced
    /// option list, or a mode it switched to. Another session's updates say nothing of this one.
    fn absorb_config_updates(&mut self, session_id: &str, live: &mut Vec<ConfigOption>) {
        let updates = std::mem::take(&mut self.config_updates);
        for (_, event) in updates
            .into_iter()
            .filter(|(update_session, _)| update_session == session_id)
        {
            match event {
                SessionEvent::ConfigOptionsUpdated { options } => *live = options,
                SessionEvent::ModeChanged { mode_id } => {
                    if let Some(mode) = live
                        .iter_mut()
                        .find(|opt| opt.category == ConfigCategory::Mode)
                    {
                        mode.current_value = mode_id;
                    }
                }
                _ => {}
            }
        }
    }

    /// Inspect advertised models (and per-model thought levels when supported) for probes/CLI.
    pub async fn inspect_models(
        &mut self,
        cwd: &Path,
    ) -> Result<(Vec<ListedModel>, Vec<ConfigOption>), Problem> {
        self.initialize().await?;
        let session = self.session_new(cwd, &[], None).await?;
        let mut config = session.config_options.clone();
        let Some(model_opt) = config
            .iter()
            .find(|opt| opt.category == ConfigCategory::Model)
            .cloned()
        else {
            return Ok((Vec::new(), config));
        };
        let mut models: Vec<ListedModel> = model_opt
            .choices
            .iter()
            .filter_map(|choice| {
                Some(ListedModel::new(
                    choice.value.parse().ok()?,
                    choice.name.clone(),
                ))
            })
            .collect();
        for model in models
            .iter_mut()
            .filter(|model| model.slug.as_str() == model_opt.current_value)
        {
            *model = listed_with_thought(model, &config);
        }
        if !session.uses_legacy_models {
            for model in models
                .iter_mut()
                .filter(|model| model.slug.as_str() != model_opt.current_value)
            {
                if let Ok(updated) = self
                    .set_config_option(&session.session_id, &model_opt.id, model.slug.as_str())
                    .await
                {
                    config = updated;
                    *model = listed_with_thought(model, &config);
                }
            }
        }
        Ok((models, session.config_options))
    }

    pub async fn logout(&mut self) -> Result<(), Problem> {
        let init = self.initialize().await?;
        if !init.capabilities.logout_supported {
            return Err(Problem::Failed(format!(
                "This {} CLI does not offer ACP sign-out. Sign out directly in the CLI (for example `/logout`).",
                self.agent_name
            )));
        }
        let _: Value = self
            .request_with_timeout("logout", json!({}), INITIALIZE_TIMEOUT)
            .await?;
        Ok(())
    }

    pub async fn start_prompt(&mut self, session_id: &str, text: &str) -> Result<u64, Problem> {
        self.send_request(
            "session/prompt",
            json!({
                "sessionId": session_id,
                "prompt": [{ "type": "text", "text": text }],
            }),
        )
        .await
    }

    /// Receive the next FIFO frame while a prompt is active. Automatically answers unknown
    /// server requests with `-32601` so the caller only sees updates, permissions, and prompt end.
    pub async fn next_prompt_frame(
        &mut self,
        prompt_request_id: u64,
        session_id: &str,
    ) -> Result<PromptFrame, Problem> {
        loop {
            match self.next_raw_frame("session/prompt").await? {
                WireFrame::Update {
                    session_id: sid,
                    event,
                } if sid == session_id => return Ok(PromptFrame::Update(event)),
                WireFrame::Update { .. } => {}
                WireFrame::PermissionRequest(req) if req.session_id == session_id => {
                    return Ok(PromptFrame::Permission(req));
                }
                WireFrame::PermissionRequest(req) => {
                    self.send_permission_cancelled(req.rpc_id).await?;
                }
                WireFrame::UnknownServerRequest { id, .. } => {
                    self.send_method_not_found(id).await?;
                }
                WireFrame::Response { id, result } if id == prompt_request_id => {
                    let parsed: RawPromptResult = serde_json::from_value(result).map_err(|_| {
                        Problem::Failed(format!(
                            "{} returned an invalid ACP session/prompt response.",
                            self.agent_name
                        ))
                    })?;
                    return Ok(PromptFrame::Finished(parsed.stop_reason));
                }
                WireFrame::Error {
                    id,
                    code,
                    message,
                    data,
                } if id == prompt_request_id => {
                    return Err(self.rpc_error("session/prompt", code, &message, &data));
                }
                WireFrame::SignedOut => {
                    return Err(Problem::SignedOut {
                        agent: self.agent_name.clone(),
                        hint: self.auth_hint.clone(),
                    });
                }
                WireFrame::Eof => {
                    return Err(Problem::Failed(format!(
                        "{} exited while handling ACP session/prompt.",
                        self.agent_name
                    )));
                }
                WireFrame::ProtocolError(msg) => return Err(Problem::Failed(msg)),
                WireFrame::Response { .. } | WireFrame::Error { .. } => {}
            }
        }
    }

    /// Send ACP `session/request_permission` `selected` outcome on the wire.
    pub async fn send_permission_selected(
        &mut self,
        rpc_id: RpcId,
        option_id: &str,
    ) -> Result<(), Problem> {
        self.send_json(&permission_selected_reply(rpc_id, option_id))
            .await
    }

    /// Send ACP `session/request_permission` `cancelled` outcome per ACP v1 spec.
    pub async fn send_permission_cancelled(&mut self, rpc_id: RpcId) -> Result<(), Problem> {
        self.send_json(&permission_cancelled_reply(rpc_id)).await
    }

    /// Send `session/cancel` notification with bounded write timeout.
    pub async fn send_cancel(&mut self, session_id: &str) {
        let _ = tokio::time::timeout(
            CANCEL_WRITE_GRACE,
            self.send_json(&json!({
                "jsonrpc": "2.0",
                "method": "session/cancel",
                "params": { "sessionId": session_id }
            })),
        )
        .await;
    }

    /// Drain stdout up to `PROMPT_CANCEL_GRACE` after `session/cancel` so a warm child can
    /// return to quiescence with `StopReason::Cancelled` instead of needing process termination.
    pub async fn drain_after_cancel(&mut self, prompt_request_id: u64, session_id: &str) -> bool {
        let result = tokio::time::timeout(PROMPT_CANCEL_GRACE, async {
            loop {
                match self.next_prompt_frame(prompt_request_id, session_id).await {
                    Ok(PromptFrame::Finished(_)) => return true,
                    Ok(PromptFrame::Permission(req)) => {
                        if self.send_permission_cancelled(req.rpc_id).await.is_err() {
                            return false;
                        }
                    }
                    Ok(PromptFrame::Update(_)) => {}
                    Err(_) => return false,
                }
            }
        })
        .await;
        matches!(result, Ok(true))
    }

    async fn send_method_not_found(&mut self, id: RpcId) -> Result<(), Problem> {
        self.send_json(&method_not_found_reply(id)).await
    }

    async fn request_with_timeout<T: serde::de::DeserializeOwned>(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<T, Problem> {
        let request_id = self.send_request(method, params).await?;
        let outcome = tokio::time::timeout(timeout, async {
            loop {
                match self.next_raw_frame(method).await? {
                    WireFrame::UnknownServerRequest { id, .. } => {
                        self.send_method_not_found(id).await?;
                    }
                    WireFrame::PermissionRequest(req) => {
                        self.send_permission_cancelled(req.rpc_id).await?;
                    }
                    WireFrame::Response { id, result } if id == request_id => {
                        return serde_json::from_value(result).map_err(|_| {
                            Problem::Failed(format!(
                                "{} returned an invalid ACP {method} response.",
                                self.agent_name
                            ))
                        });
                    }
                    WireFrame::Error {
                        id,
                        code,
                        message,
                        data,
                    } if id == request_id => {
                        return Err(self.rpc_error(method, code, &message, &data));
                    }
                    WireFrame::SignedOut => {
                        return Err(Problem::SignedOut {
                            agent: self.agent_name.clone(),
                            hint: self.auth_hint.clone(),
                        });
                    }
                    WireFrame::Eof => {
                        return Err(Problem::Failed(format!(
                            "{} exited while handling ACP {method}.",
                            self.agent_name
                        )));
                    }
                    WireFrame::ProtocolError(msg) => {
                        return Err(Problem::Failed(self.redactor.redact_all(&msg)));
                    }
                    WireFrame::Update {
                        session_id,
                        event:
                            event @ (SessionEvent::ConfigOptionsUpdated { .. }
                            | SessionEvent::ModeChanged { .. }),
                    } => self.record_config_update(session_id, event),
                    WireFrame::Update { .. }
                    | WireFrame::Response { .. }
                    | WireFrame::Error { .. } => {}
                }
            }
        })
        .await;
        outcome.unwrap_or_else(|_| {
            Err(Problem::Failed(format!(
                "{} timed out while handling ACP {method}.",
                self.agent_name
            )))
        })
    }

    fn record_config_update(&mut self, session_id: String, event: SessionEvent) {
        match &event {
            SessionEvent::ConfigOptionsUpdated { .. } => {
                // A full option list supersedes every earlier config/mode update for this session.
                self.config_updates.retain(|(sid, _)| sid != &session_id);
            }
            SessionEvent::ModeChanged { .. } => {
                // Only the latest mode change for this session matters after any full list.
                self.config_updates.retain(|(sid, existing)| {
                    !(sid == &session_id && matches!(existing, SessionEvent::ModeChanged { .. }))
                });
            }
            _ => return,
        }
        if self.config_updates.len() >= MAX_CONFIG_UPDATES {
            self.config_updates.remove(0);
        }
        self.config_updates.push((session_id, event));
    }

    async fn send_request(&mut self, method: &str, params: Value) -> Result<u64, Problem> {
        self.next_request_id += 1;
        let id = self.next_request_id;
        self.send_json(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))
        .await?;
        Ok(id)
    }

    async fn send_json(&mut self, value: &Value) -> Result<(), Problem> {
        let mut bytes = value.to_string().into_bytes();
        bytes.push(b'\n');
        let agent_name = self.agent_name.clone();
        tokio::time::timeout(STDIN_WRITE_TIMEOUT, async {
            self.write
                .write_all(&bytes)
                .await
                .map_err(|_| Problem::Failed(format!("{agent_name} stopped reading ACP input.")))?;
            self.write
                .flush()
                .await
                .map_err(|_| Problem::Failed(format!("{agent_name} stopped reading ACP input.")))
        })
        .await
        .unwrap_or_else(|_| {
            Err(Problem::Failed(format!(
                "{} timed out reading ACP input.",
                self.agent_name
            )))
        })
    }

    async fn next_raw_frame(&mut self, method: &str) -> Result<WireFrame, Problem> {
        if let Some(frame) = self.buffered_frames.pop_front() {
            return Ok(frame);
        }
        self.frames.recv().await.ok_or_else(|| {
            Problem::Failed(format!(
                "{} exited while handling ACP {method}.",
                self.agent_name
            ))
        })
    }

    fn is_suppressed_replay(&self, session_id: &str, request_id: u64) -> bool {
        self.replaying.as_ref().is_some_and(|phase| {
            phase.connection_id == self.id
                && phase.load_request_id == request_id
                && phase.remote_session_id == session_id
        })
    }

    fn rpc_error(&self, method: &str, code: i64, message: &str, data: &str) -> Problem {
        classify_rpc_error(
            &self.agent_name,
            &self.auth_hint,
            &self.redactor,
            method,
            code,
            message,
            data,
        )
    }
}

fn classify_rpc_error(
    agent_name: &str,
    auth_hint: &str,
    redactor: &StreamRedactor,
    method: &str,
    code: i64,
    message: &str,
    data: &str,
) -> Problem {
    if code == AUTH_REQUIRED {
        return Problem::SignedOut {
            agent: agent_name.to_owned(),
            hint: auth_hint.to_owned(),
        };
    }
    let redacted_message = redactor.redact_all(message.trim());
    let redacted_data = redactor.redact_all(data);
    Problem::classify_text(
        agent_name,
        auth_hint,
        &format!("{code} {redacted_message} {redacted_data}"),
    )
    .unwrap_or_else(|| {
        let detail: String = redacted_message
            .chars()
            .filter(|c| !c.is_control())
            .collect();
        if detail.is_empty() {
            Problem::Failed(format!(
                "{agent_name} rejected ACP {method} (error {code})."
            ))
        } else {
            Problem::Failed(format!(
                "{agent_name} rejected ACP {method} (error {code}): {detail}"
            ))
        }
    })
}

/// Extract a confirmed mode ID from a legacy `session/set_mode` JSON-RPC result when present.
fn extract_confirmed_legacy_mode(result: &Value) -> Option<String> {
    if let Some(mode) = result
        .get("currentModeId")
        .or_else(|| result.get("modeId"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        return Some(mode.to_owned());
    }
    if let Some(mode) = result
        .get("modes")
        .and_then(|m| m.get("currentModeId"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        return Some(mode.to_owned());
    }
    if let Some(raw_opts) = result.get("configOptions").and_then(Value::as_array) {
        let parsed: Vec<RawConfigOption> = raw_opts
            .iter()
            .filter_map(|v| serde_json::from_value(v.clone()).ok())
            .collect();
        let options = normalize_config_options(parsed);
        if let Some(opt) = options
            .into_iter()
            .find(|opt| opt.category == ConfigCategory::Mode && !opt.current_value.is_empty())
        {
            return Some(opt.current_value);
        }
    }
    None
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.reader_task.abort();
    }
}

/// Reconcile active non-mode selections (model, thought level, etc.) against a newly received
/// `live` option list, returning explicit fallback notices for any option that changed or was
/// dropped (`C-NO-SILENT-REGRESSION`).
pub(crate) fn reconcile_non_mode_options(
    agent_name: &str,
    live: &[ConfigOption],
    selections: &mut Vec<ConfigSelection>,
) -> Vec<String> {
    let mut notices = Vec::new();
    let mut reconciled = Vec::with_capacity(selections.len());
    for mut selection in std::mem::take(selections) {
        if selection.category == Some(ConfigCategory::Mode) {
            reconciled.push(selection);
            continue;
        }
        match live.iter().find(|opt| opt.id == selection.id) {
            Some(opt) if opt.current_value != selection.value => {
                notices.push(format!(
                    "{agent_name} changed {} from {:?} to {:?}.",
                    opt.name, selection.value, opt.current_value
                ));
                selection.value = opt.current_value.clone();
                reconciled.push(selection);
            }
            Some(_) => reconciled.push(selection),
            None => {
                notices.push(format!(
                    "{agent_name} dropped {} {:?}.",
                    selection.id, selection.value
                ));
            }
        }
    }
    *selections = reconciled;
    notices
}

/// Frames surfaced to the turn runner during an active `session/prompt`.
#[derive(Debug)]
pub enum PromptFrame {
    Update(SessionEvent),
    Permission(IncomingPermission),
    Finished(StopReason),
}

/// Single stdout reader task enforcing strict FIFO receive-order barriers.
async fn stdout_reader_loop(
    stdout: ChildStdout,
    tx: mpsc::Sender<WireFrame>,
    hide_always_options: bool,
    redactor: StreamRedactor,
) {
    let mut reader = BufReader::new(stdout);
    let mut line = Vec::new();
    let mut dropped_display_deltas: usize = 0;

    loop {
        line.clear();
        let read = match (&mut reader)
            .take(LINE_BYTES + 1)
            .read_until(b'\n', &mut line)
            .await
        {
            Ok(count) => count,
            Err(error) => {
                let _ = tx
                    .send(WireFrame::ProtocolError(format!(
                        "Cannot read ACP output: {error}."
                    )))
                    .await;
                return;
            }
        };
        if read == 0 {
            let _ = tx.send(WireFrame::Eof).await;
            return;
        }
        if line.len() as u64 > LINE_BYTES {
            let _ = tx
                .send(WireFrame::ProtocolError(
                    "ACP agent wrote an oversized message (> 32 MiB).".into(),
                ))
                .await;
            return;
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        if line.starts_with(AUTH_LINK_MARKER.as_bytes()) {
            let _ = tx.send(WireFrame::SignedOut).await;
            return;
        }
        let parsed = match parse_wire_frame(&line, hide_always_options, &redactor) {
            Ok(Some(frame)) => frame,
            Ok(None) => continue,
            Err(error) => {
                let _ = tx.send(WireFrame::ProtocolError(error)).await;
                return;
            }
        };

        let non_essential_delta = is_non_essential_display_delta(&parsed);
        if non_essential_delta {
            match tx.try_send(parsed) {
                Ok(()) => {}
                Err(mpsc::error::TrySendError::Full(_)) => {
                    dropped_display_deltas += 1;
                }
                Err(mpsc::error::TrySendError::Closed(_)) => return,
            }
        } else {
            if dropped_display_deltas > 0 {
                let count = std::mem::take(&mut dropped_display_deltas);
                let sid = match &parsed {
                    WireFrame::Update { session_id, .. } => session_id.clone(),
                    _ => String::new(),
                };
                if !sid.is_empty() {
                    let _ = tx
                        .send(WireFrame::Update {
                            session_id: sid,
                            event: SessionEvent::Notice {
                                text: format!(
                                    "Coalesced {count} rapid display update(s) to preserve ACP stream responsiveness."
                                ),
                            },
                        })
                        .await;
                }
            }
            if tx.send(parsed).await.is_err() {
                return;
            }
        }
    }
}

fn is_non_essential_display_delta(frame: &WireFrame) -> bool {
    matches!(
        frame,
        WireFrame::Update {
            event: SessionEvent::AgentThoughtChunk { .. }
                | SessionEvent::ToolCallUpdate { status: None, .. },
            ..
        }
    )
}

/// Pure edge parser turning one JSON-RPC line into a typed [`WireFrame`] (`T-PARSE-AT-EDGE`).
pub(crate) fn parse_wire_frame(
    bytes: &[u8],
    hide_always_options: bool,
    redactor: &StreamRedactor,
) -> Result<Option<WireFrame>, String> {
    let incoming: RawIncoming = serde_json::from_slice(bytes)
        .map_err(|error| format!("ACP agent wrote an invalid JSON-RPC message: {error}."))?;
    match incoming {
        RawIncoming::ServerRequest { id, method, params } => {
            if method == "session/request_permission" {
                let parsed: RawPermissionParams = serde_json::from_value(params).map_err(|_| {
                    "ACP agent sent malformed session/request_permission parameters.".to_owned()
                })?;
                Ok(Some(WireFrame::PermissionRequest(normalize_permission(
                    id,
                    parsed,
                    hide_always_options,
                    redactor,
                ))))
            } else {
                #[cfg(not(test))]
                let _ = method;
                Ok(Some(WireFrame::UnknownServerRequest {
                    id,
                    #[cfg(test)]
                    method,
                }))
            }
        }
        RawIncoming::Notification { method, params } => {
            if method != "session/update" {
                return Ok(None);
            }
            let update: RawUpdateNotification = serde_json::from_value(params)
                .map_err(|_| "ACP agent sent malformed session/update notification.".to_owned())?;
            Ok(
                normalize_update(update.update, redactor).map(|event| WireFrame::Update {
                    session_id: update.session_id,
                    event,
                }),
            )
        }
        RawIncoming::Result { id, result } => Ok(Some(WireFrame::Response { id, result })),
        RawIncoming::Error { id, error } => Ok(Some(WireFrame::Error {
            id,
            code: error.code,
            message: error.message,
            data: if error.data.is_null() {
                String::new()
            } else {
                error.data.to_string()
            },
        })),
        RawIncoming::Other(_) => Ok(None),
    }
}

pub(crate) fn initialization_params() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "clientCapabilities": {
            "fs": {
                "readTextFile": false,
                "writeTextFile": false
            },
            "terminal": false
        },
        "clientInfo": {
            "name": "artifactize",
            "title": "Artifactize",
            "version": env!("CARGO_PKG_VERSION"),
        }
    })
}

pub(crate) fn permission_selected_reply(id: RpcId, option_id: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "outcome": {
                "outcome": "selected",
                "optionId": option_id
            }
        }
    })
}

pub(crate) fn permission_cancelled_reply(id: RpcId) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "outcome": {
                "outcome": "cancelled"
            }
        }
    })
}

pub(crate) fn method_not_found_reply(id: RpcId) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": crate::llm::stdio_mcp::METHOD_NOT_FOUND,
            "message": "Artifactize does not provide this ACP capability."
        }
    })
}

fn listed_with_thought(model: &ListedModel, config: &[ConfigOption]) -> ListedModel {
    let base = ListedModel::new(model.slug.clone(), model.display_name.clone());
    let Some(option) = config
        .iter()
        .find(|opt| opt.category == ConfigCategory::ThoughtLevel)
    else {
        return base;
    };
    base.with_efforts(
        option.choices.iter().map(|choice| choice.value.clone()),
        Some(option.current_value.clone()),
    )
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NewSessionParams<'a> {
    #[serde(serialize_with = "crate::platform::path_serde::native::serialize_path")]
    cwd: &'a Path,
    mcp_servers: &'a [McpServer],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LoadSessionParams<'a> {
    session_id: &'a str,
    #[serde(serialize_with = "crate::platform::path_serde::native::serialize_path")]
    cwd: &'a Path,
    mcp_servers: &'a [McpServer],
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawIncoming {
    ServerRequest {
        id: RpcId,
        method: String,
        #[serde(default)]
        params: Value,
    },
    Notification {
        method: String,
        #[serde(default)]
        params: Value,
    },
    Result {
        id: u64,
        result: Value,
    },
    Error {
        id: u64,
        error: RawRpcError,
    },
    Other(serde::de::IgnoredAny),
}

#[derive(Deserialize)]
struct RawRpcError {
    code: i64,
    #[serde(default)]
    message: String,
    #[serde(default)]
    data: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawInitialized {
    protocol_version: u32,
    #[serde(default)]
    agent_capabilities: RawCapabilities,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCapabilities {
    #[serde(default)]
    load_session: bool,
    #[serde(default)]
    auth: RawAuthCapabilities,
    #[serde(default, rename = "_meta")]
    meta: RawAgentMeta,
}

#[derive(Default, Deserialize)]
struct RawAuthCapabilities {
    logout: Option<serde::de::IgnoredAny>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawAgentMeta {
    #[serde(default)]
    antigravity: RawAntigravityCapabilities,
    #[serde(default)]
    system_prompt: RawSystemPrompt,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawAntigravityCapabilities {
    #[serde(default)]
    system_prompt: RawSystemPrompt,
}

#[derive(Default, Deserialize)]
struct RawSystemPrompt {
    #[serde(default)]
    append: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawSession {
    session_id: String,
    #[serde(default)]
    config_options: Option<Vec<RawConfigOption>>,
    #[serde(default)]
    models: Option<RawLegacyModels>,
    #[serde(default)]
    modes: Option<RawLegacyModes>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawSessionLoadResult {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    config_options: Option<Vec<RawConfigOption>>,
    #[serde(default)]
    models: Option<RawLegacyModels>,
    #[serde(default)]
    modes: Option<RawLegacyModes>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawConfigOptionsReply {
    #[serde(default)]
    config_options: Vec<RawConfigOption>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawLegacyModels {
    #[serde(default)]
    current_model_id: String,
    #[serde(default)]
    available_models: Vec<RawLegacyModelEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawLegacyModelEntry {
    model_id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawLegacyModes {
    #[serde(default)]
    current_mode_id: String,
    #[serde(default)]
    available_modes: Vec<RawLegacyModeEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawLegacyModeEntry {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RawConfigOption {
    Select {
        id: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        category: Option<String>,
        #[serde(rename = "currentValue")]
        current_value: String,
        #[serde(default)]
        options: Vec<RawChoice>,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawChoice {
    Value {
        value: String,
        name: String,
        #[serde(default)]
        description: Option<String>,
    },
    Group {
        options: Vec<RawChoice>,
    },
    Other(serde::de::IgnoredAny),
}

fn flatten_choices(raw: &[RawChoice], category: ConfigCategory, out: &mut Vec<ConfigChoice>) {
    for item in raw {
        match item {
            RawChoice::Value {
                value,
                name,
                description,
            } => {
                if !out.iter().any(|existing| existing.value == *value) {
                    out.push(ConfigChoice {
                        value: value.clone(),
                        name: name.clone(),
                        description: description.clone(),
                        bypass_warning: category == ConfigCategory::Mode
                            && is_bypass_mode(value, name),
                    });
                }
            }
            RawChoice::Group { options } => flatten_choices(options, category, out),
            RawChoice::Other(_) => {}
        }
    }
}

fn is_bypass_mode(value: &str, name: &str) -> bool {
    let combined = format!("{value} {name}").to_ascii_lowercase();
    [
        "bypass",
        "yolo",
        "dangerously",
        "auto_approve",
        "auto-approve",
        "full_auto",
        "full-auto",
        "dontask",
        "don't ask",
    ]
    .iter()
    .any(|marker| combined.contains(marker))
}

fn infer_category(id: &str, category: Option<&str>) -> ConfigCategory {
    match category {
        Some("model") => ConfigCategory::Model,
        Some("mode") => ConfigCategory::Mode,
        Some("thought_level") => ConfigCategory::ThoughtLevel,
        _ => match id {
            "model" => ConfigCategory::Model,
            "mode" => ConfigCategory::Mode,
            "thought_level" | "thinking" | "thinking_level" | "effort" => {
                ConfigCategory::ThoughtLevel
            }
            _ => ConfigCategory::Other,
        },
    }
}

fn normalize_config_options(raw: Vec<RawConfigOption>) -> Vec<ConfigOption> {
    raw.into_iter()
        .filter_map(|opt| match opt {
            RawConfigOption::Select {
                id,
                name,
                category,
                current_value,
                options,
            } => {
                let category = infer_category(&id, category.as_deref());
                let mut choices = Vec::new();
                flatten_choices(&options, category, &mut choices);
                Some(ConfigOption {
                    name: name.unwrap_or_else(|| id.clone()),
                    id,
                    category,
                    current_value,
                    choices,
                })
            }
            RawConfigOption::Other => None,
        })
        .collect()
}

fn normalize_session(raw: RawSession) -> OpenedSession {
    let mut config_options = raw
        .config_options
        .map(normalize_config_options)
        .unwrap_or_default();
    let mut uses_legacy_models = false;
    let mut uses_legacy_modes = false;

    if !config_options
        .iter()
        .any(|opt| opt.category == ConfigCategory::Model)
        && let Some(models) = raw.models
    {
        uses_legacy_models = true;
        config_options.push(ConfigOption {
            id: "model".into(),
            name: "Model".into(),
            category: ConfigCategory::Model,
            current_value: models.current_model_id,
            choices: models
                .available_models
                .into_iter()
                .map(|entry| ConfigChoice {
                    name: entry.name.unwrap_or_else(|| entry.model_id.clone()),
                    value: entry.model_id,
                    description: entry.description,
                    bypass_warning: false,
                })
                .collect(),
        });
    }
    if !config_options
        .iter()
        .any(|opt| opt.category == ConfigCategory::Mode)
        && let Some(modes) = raw.modes
    {
        uses_legacy_modes = true;
        config_options.push(ConfigOption {
            id: "mode".into(),
            name: "Mode".into(),
            category: ConfigCategory::Mode,
            current_value: modes.current_mode_id,
            choices: modes
                .available_modes
                .into_iter()
                .map(|entry| {
                    let name = entry.name.unwrap_or_else(|| entry.id.clone());
                    let bypass_warning = is_bypass_mode(&entry.id, &name);
                    ConfigChoice {
                        value: entry.id,
                        name,
                        description: entry.description,
                        bypass_warning,
                    }
                })
                .collect(),
        });
    }

    OpenedSession {
        session_id: raw.session_id,
        config_options,
        uses_legacy_models,
        uses_legacy_modes,
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawUpdateNotification {
    session_id: String,
    update: RawUpdate,
}

#[derive(Deserialize)]
#[serde(tag = "sessionUpdate", rename_all = "snake_case")]
enum RawUpdate {
    AgentMessageChunk {
        content: RawContentBlock,
    },
    AgentThoughtChunk {
        content: RawContentBlock,
    },
    ToolCall {
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        #[serde(default)]
        title: String,
        #[serde(default)]
        kind: Option<String>,
        #[serde(default)]
        status: Option<String>,
        #[serde(default)]
        locations: Vec<RawToolLocation>,
        #[serde(default)]
        content: Vec<RawToolContent>,
    },
    ToolCallUpdate {
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        kind: Option<String>,
        #[serde(default)]
        status: Option<String>,
        #[serde(default)]
        locations: Option<Vec<RawToolLocation>>,
        #[serde(default)]
        content: Option<Vec<RawToolContent>>,
    },
    Plan {
        #[serde(default)]
        entries: Vec<RawPlanEntry>,
    },
    ConfigOptionUpdate {
        #[serde(rename = "configOptions", default)]
        config_options: Vec<RawConfigOption>,
    },
    CurrentModeUpdate {
        #[serde(rename = "currentModeId", default)]
        current_mode_id: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RawContentBlock {
    Text {
        text: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawToolLocation {
    path: String,
    #[serde(default)]
    line: Option<u32>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RawToolContent {
    Content {
        content: RawContentBlock,
    },
    Diff {
        path: String,
        #[serde(rename = "oldText", default)]
        old_text: Option<String>,
        #[serde(rename = "newText", default)]
        new_text: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPlanEntry {
    content: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    priority: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPermissionParams {
    session_id: String,
    #[serde(default)]
    tool_call: Option<RawPermissionToolCall>,
    #[serde(default)]
    options: Vec<RawPermissionOption>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPermissionToolCall {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    locations: Vec<RawToolLocation>,
    #[serde(default)]
    raw_input: Option<RawPermissionInput>,
}

/// Typed display subset extracted from ACP `toolCall.rawInput` at the edge; unknown JSON shapes
/// and unparsed fields are discarded rather than forwarded into public `Block::Permission.detail`.
#[derive(Deserialize)]
#[serde(untagged)]
enum RawPermissionInput {
    Text(String),
    Fields(RawPermissionFields),
    Other(serde::de::IgnoredAny),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawCommandField {
    Text(String),
    Argv(Vec<String>),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPermissionFields {
    #[serde(default, alias = "cmd")]
    command: Option<RawCommandField>,
    #[serde(default)]
    program: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default, alias = "filePath", alias = "file_path")]
    path: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPermissionOption {
    option_id: String,
    name: String,
    kind: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPromptResult {
    stop_reason: StopReason,
}

fn map_tool_kind(kind: Option<&str>) -> ToolKind {
    match kind {
        Some("read" | "search") => ToolKind::Read,
        Some("edit" | "delete" | "move") => ToolKind::Edit,
        Some("execute") => ToolKind::Command,
        _ => ToolKind::Other,
    }
}

fn map_tool_status(status: Option<&str>) -> Option<ToolStatus> {
    match status {
        Some("pending") => Some(ToolStatus::Pending),
        Some("in_progress" | "running") => Some(ToolStatus::Running),
        Some("completed") => Some(ToolStatus::Completed),
        Some("failed" | "error") => Some(ToolStatus::Failed),
        Some("cancelled" | "canceled") => Some(ToolStatus::Cancelled),
        _ => None,
    }
}

fn map_plan_status(status: Option<&str>) -> PlanStepStatus {
    match status {
        Some("in_progress") => PlanStepStatus::InProgress,
        Some("completed") => PlanStepStatus::Completed,
        Some("failed") => PlanStepStatus::Failed,
        _ => PlanStepStatus::Pending,
    }
}

fn extract_tool_content(
    items: Vec<RawToolContent>,
    redactor: &StreamRedactor,
) -> (Option<String>, Option<ToolDiff>, bool) {
    let mut output_parts = Vec::new();
    let mut diff = None;
    for item in items {
        match item {
            RawToolContent::Content {
                content: RawContentBlock::Text { text },
            } => output_parts.push(text),
            RawToolContent::Diff {
                path,
                old_text,
                new_text,
            } => {
                let (old_text, new_text, truncated) = bound_diff(old_text, new_text, redactor);
                diff = Some(ToolDiff {
                    path: redactor.redact_all(&platform::normalize_external_path(&path)),
                    old_text,
                    new_text,
                    truncated,
                });
            }
            _ => {}
        }
    }
    if output_parts.is_empty() {
        (None, diff, false)
    } else {
        // Redact BEFORE truncating so a secret straddling the cap boundary is replaced cleanly
        // and `[REDACTED]` expansion never exceeds `TOOL_OUTPUT_BYTES` (`C-ACP-EDGE-ISOLATION`).
        let joined = redactor.redact_all(&output_parts.join("\n"));
        let (bounded, truncated) = bound_text(&joined, TOOL_OUTPUT_BYTES);
        (Some(bounded), diff, truncated)
    }
}

fn bound_diff(
    old_text: Option<String>,
    new_text: String,
    redactor: &StreamRedactor,
) -> (Option<String>, String, bool) {
    let half = DIFF_BYTES / 2;
    let (old_bounded, old_trunc) = match old_text {
        Some(text) => {
            let redacted = redactor.redact_all(&text);
            let (b, t) = bound_text(&redacted, half);
            (Some(b), t)
        }
        None => (None, false),
    };
    let redacted_new = redactor.redact_all(&new_text);
    let (new_bounded, new_trunc) = bound_text(&redacted_new, DIFF_BYTES.saturating_sub(half));
    (old_bounded, new_bounded, old_trunc || new_trunc)
}

pub(crate) fn bound_text(text: &str, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text.to_owned(), false);
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_owned(), true)
}

fn normalize_update(raw: RawUpdate, redactor: &StreamRedactor) -> Option<SessionEvent> {
    match raw {
        RawUpdate::AgentMessageChunk {
            content: RawContentBlock::Text { text },
        } => Some(SessionEvent::AgentMessageChunk { text }),
        RawUpdate::AgentThoughtChunk {
            content: RawContentBlock::Text { text },
        } => Some(SessionEvent::AgentThoughtChunk { text }),
        RawUpdate::ToolCall {
            tool_call_id,
            title,
            kind,
            status,
            locations,
            content,
        } => {
            let (output, diff, output_truncated) = extract_tool_content(content, redactor);
            let locations: Vec<ToolLocation> = locations
                .into_iter()
                .map(|loc| ToolLocation {
                    path: platform::normalize_external_path(&loc.path),
                    line: loc.line,
                })
                .collect();
            let summary = if !title.trim().is_empty() {
                title.clone()
            } else if let Some(first) = locations.first() {
                first.path.clone()
            } else {
                "Tool call".into()
            };
            Some(SessionEvent::ToolCall {
                call_id: tool_call_id,
                title,
                tool_kind: map_tool_kind(kind.as_deref()),
                status: map_tool_status(status.as_deref()).unwrap_or(ToolStatus::Running),
                summary,
                locations,
                output,
                diff,
                output_truncated,
            })
        }
        RawUpdate::ToolCallUpdate {
            tool_call_id,
            title,
            kind,
            status,
            locations,
            content,
        } => {
            let (output, diff, output_truncated) = content
                .map(|items| extract_tool_content(items, redactor))
                .unwrap_or((None, None, false));
            let locations = locations.map(|locs| {
                locs.into_iter()
                    .map(|loc| ToolLocation {
                        path: platform::normalize_external_path(&loc.path),
                        line: loc.line,
                    })
                    .collect()
            });
            let summary = title.clone();
            Some(SessionEvent::ToolCallUpdate {
                call_id: tool_call_id,
                title,
                tool_kind: kind.as_deref().map(|k| map_tool_kind(Some(k))),
                status: map_tool_status(status.as_deref()),
                summary,
                locations,
                output,
                diff,
                output_truncated,
            })
        }
        RawUpdate::Plan { entries } => Some(SessionEvent::Plan {
            steps: entries
                .into_iter()
                .map(|entry| PlanStep {
                    content: entry.content,
                    status: map_plan_status(entry.status.as_deref()),
                    priority: entry.priority,
                })
                .collect(),
        }),
        RawUpdate::ConfigOptionUpdate { config_options } => {
            Some(SessionEvent::ConfigOptionsUpdated {
                options: normalize_config_options(config_options),
            })
        }
        RawUpdate::CurrentModeUpdate { current_mode_id } => Some(SessionEvent::ModeChanged {
            mode_id: current_mode_id,
        }),
        _ => None,
    }
}

fn format_permission_detail(
    input: RawPermissionInput,
    redactor: &StreamRedactor,
) -> Option<String> {
    let raw = match input {
        RawPermissionInput::Text(text) => text,
        RawPermissionInput::Fields(fields) => {
            let mut lines = Vec::new();
            if let Some(command) = fields.command {
                let text = match command {
                    RawCommandField::Text(s) => s,
                    RawCommandField::Argv(argv) => argv.join(" "),
                };
                if !text.trim().is_empty() {
                    lines.push(("Command", text));
                }
            } else if let Some(program) = fields.program
                && !program.trim().is_empty()
            {
                let text = if fields.args.is_empty() {
                    program
                } else {
                    format!("{program} {}", fields.args.join(" "))
                };
                lines.push(("Command", text));
            }
            if let Some(path) = fields.path
                && !path.trim().is_empty()
            {
                lines.push(("Path", platform::normalize_external_path(&path)));
            }
            if let Some(cwd) = fields.cwd
                && !cwd.trim().is_empty()
            {
                lines.push(("Directory", platform::normalize_external_path(&cwd)));
            }
            if let Some(url) = fields.url
                && !url.trim().is_empty()
            {
                lines.push(("URL", url));
            }
            if let Some(description) = fields.description
                && !description.trim().is_empty()
            {
                lines.push(("Description", description));
            }
            match lines.as_slice() {
                [] => return None,
                [(_, only)] => only.clone(),
                many => many
                    .iter()
                    .map(|(label, value)| format!("{label}: {value}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            }
        }
        RawPermissionInput::Other(_) => return None,
    };
    let cleaned = platform::clean_program_text(raw.as_bytes());
    let redacted = redactor.redact_all(&cleaned);
    let (bounded, _) = bound_text(&redacted, PERMISSION_DETAIL_BYTES);
    (!bounded.trim().is_empty()).then_some(bounded)
}

fn normalize_permission(
    rpc_id: RpcId,
    raw: RawPermissionParams,
    hide_always_options: bool,
    redactor: &StreamRedactor,
) -> IncomingPermission {
    let tool = raw.tool_call.unwrap_or(RawPermissionToolCall {
        title: None,
        kind: None,
        locations: Vec::new(),
        raw_input: None,
    });
    let title = tool
        .title
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "Agent permission request".into());
    let tool_kind = map_tool_kind(tool.kind.as_deref());
    let summary = if !tool.locations.is_empty() {
        let paths: Vec<_> = tool
            .locations
            .iter()
            .map(|loc| {
                let path = platform::normalize_external_path(&loc.path);
                match loc.line {
                    Some(line) => format!("{path}:{line}"),
                    None => path,
                }
            })
            .collect();
        format!("{title} ({})", paths.join(", "))
    } else {
        title.clone()
    };
    let detail = tool
        .raw_input
        .and_then(|input| format_permission_detail(input, redactor));
    let options = raw
        .options
        .into_iter()
        .filter_map(|opt| {
            let kind = match opt.kind.as_str() {
                "allow_once" => PermissionOptionKind::AllowOnce,
                "allow_always" => PermissionOptionKind::AllowAlways,
                "reject_once" => PermissionOptionKind::RejectOnce,
                "reject_always" => PermissionOptionKind::RejectAlways,
                _ => return None,
            };
            if hide_always_options && kind.may_persist_in_agent_settings() {
                return None;
            }
            Some(PermissionOption {
                option_id: opt.option_id,
                name: opt.name,
                kind,
                persists_in_agent_settings: kind.may_persist_in_agent_settings(),
            })
        })
        .collect();
    IncomingPermission {
        rpc_id,
        session_id: raw.session_id,
        title,
        tool_kind,
        summary,
        detail,
        options,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ACP v1 JSON Schema contract test for outbound client messages (`initialize`,
    /// permission responses, method-not-found) and inbound updates.
    #[test]
    fn acp_v1_schema_contract_validates_outbound_and_inbound_frames() {
        let init_schema = json!({
            "type": "object",
            "required": ["protocolVersion", "clientCapabilities", "clientInfo"],
            "properties": {
                "protocolVersion": { "const": 1 },
                "clientCapabilities": {
                    "type": "object",
                    "properties": {
                        "fs": {
                            "type": "object",
                            "properties": {
                                "readTextFile": { "const": false },
                                "writeTextFile": { "const": false }
                            },
                            "required": ["readTextFile", "writeTextFile"]
                        },
                        "terminal": { "const": false }
                    },
                    "required": ["fs", "terminal"]
                },
                "clientInfo": {
                    "type": "object",
                    "required": ["name", "version"]
                }
            }
        });
        let validator = jsonschema::validator_for(&init_schema).unwrap();
        assert!(validator.is_valid(&initialization_params()));

        let perm_reply_schema = json!({
            "type": "object",
            "required": ["jsonrpc", "id", "result"],
            "properties": {
                "jsonrpc": { "const": "2.0" },
                "result": {
                    "type": "object",
                    "required": ["outcome"],
                    "properties": {
                        "outcome": {
                            "oneOf": [
                                {
                                    "type": "object",
                                    "required": ["outcome", "optionId"],
                                    "properties": {
                                        "outcome": { "const": "selected" },
                                        "optionId": { "type": "string", "minLength": 1 }
                                    }
                                },
                                {
                                    "type": "object",
                                    "required": ["outcome"],
                                    "properties": {
                                        "outcome": { "const": "cancelled" }
                                    }
                                }
                            ]
                        }
                    }
                }
            }
        });
        let perm_validator = jsonschema::validator_for(&perm_reply_schema).unwrap();
        assert!(
            perm_validator.is_valid(&permission_selected_reply(RpcId::Number(7), "allow-once"))
        );
        assert!(perm_validator.is_valid(&permission_cancelled_reply(RpcId::Number(8))));

        // Inbound tool_call + diff + thought + plan + permission parsing at the edge.
        let thought_line = json!({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {
                "sessionId": "s1",
                "update": {
                    "sessionUpdate": "agent_thought_chunk",
                    "content": { "type": "text", "text": "Analyzing diff..." }
                }
            }
        });
        let no_secrets = StreamRedactor::new(Vec::<String>::new());
        let frame = parse_wire_frame(thought_line.to_string().as_bytes(), false, &no_secrets)
            .unwrap()
            .unwrap();
        assert!(matches!(
            frame,
            WireFrame::Update {
                event: SessionEvent::AgentThoughtChunk { text },
                ..
            } if text == "Analyzing diff..."
        ));

        let perm_line = json!({
            "jsonrpc": "2.0",
            "id": 99,
            "method": "session/request_permission",
            "params": {
                "sessionId": "s1",
                "toolCall": {
                    "title": "Run cargo test",
                    "kind": "execute",
                    "rawInput": { "cmd": "cargo test" }
                },
                "options": [
                    { "optionId": "once", "name": "Allow once", "kind": "allow_once" },
                    { "optionId": "always", "name": "Always allow", "kind": "allow_always" }
                ]
            }
        });
        let frame = parse_wire_frame(perm_line.to_string().as_bytes(), false, &no_secrets)
            .unwrap()
            .unwrap();
        let WireFrame::PermissionRequest(req) = frame else {
            panic!("expected permission request");
        };
        assert_eq!(req.options.len(), 2);
        assert!(!req.options[0].persists_in_agent_settings);
        assert!(req.options[1].persists_in_agent_settings);

        // When a preset hides always options with unverified persistence scopes:
        let frame_hidden = parse_wire_frame(perm_line.to_string().as_bytes(), true, &no_secrets)
            .unwrap()
            .unwrap();
        let WireFrame::PermissionRequest(req_hidden) = frame_hidden else {
            panic!("expected permission request");
        };
        assert_eq!(req_hidden.options.len(), 1);
        assert_eq!(req_hidden.options[0].option_id, "once");

        // Unknown server request maps to UnknownServerRequest (answered with -32601).
        let unknown = json!({
            "jsonrpc": "2.0",
            "id": 12,
            "method": "fs/read_text_file",
            "params": {}
        });
        let frame = parse_wire_frame(unknown.to_string().as_bytes(), false, &no_secrets)
            .unwrap()
            .unwrap();
        assert!(matches!(
            frame,
            WireFrame::UnknownServerRequest { method, .. } if method == "fs/read_text_file"
        ));
    }

    #[test]
    fn normalize_session_handles_legacy_models_and_modes_with_bypass_flag() {
        let raw: RawSession = serde_json::from_value(json!({
            "sessionId": "s-legacy",
            "models": {
                "currentModelId": "m1",
                "availableModels": [{ "modelId": "m1", "name": "Model One" }]
            },
            "modes": {
                "currentModeId": "ask",
                "availableModes": [
                    { "id": "ask", "name": "Ask" },
                    { "id": "bypassPermissions", "name": "Bypass Permissions" }
                ]
            }
        }))
        .unwrap();
        let session = normalize_session(raw);
        assert!(session.uses_legacy_models);
        assert!(session.uses_legacy_modes);
        let mode = session
            .config_options
            .iter()
            .find(|opt| opt.category == ConfigCategory::Mode)
            .unwrap();
        assert!(!mode.choices[0].bypass_warning);
        assert!(mode.choices[1].bypass_warning);
    }

    #[test]
    fn legacy_mode_confirmation_requires_reported_mode_and_rpc_errors_redact_secrets() {
        assert_eq!(extract_confirmed_legacy_mode(&json!({})), None);
        assert_eq!(
            extract_confirmed_legacy_mode(&json!({ "currentModeId": "ask" })),
            Some("ask".into())
        );
        assert_eq!(
            extract_confirmed_legacy_mode(&json!({ "modes": { "currentModeId": "plan" } })),
            Some("plan".into())
        );

        let redactor = StreamRedactor::new(["sk-acp-super-secret-value-999".to_owned()]);
        let problem = classify_rpc_error(
            "Fixture",
            "sign in",
            &redactor,
            "session/prompt",
            -32001,
            "Upstream refused key sk-acp-super-secret-value-999 immediately",
            "",
        );
        let Problem::Failed(msg) = problem else {
            panic!("expected Problem::Failed, got {problem:?}");
        };
        assert!(!msg.contains("sk-acp-super-secret-value-999"), "{msg}");
        assert!(msg.contains("[REDACTED]"), "{msg}");
    }

    #[test]
    fn empty_current_mode_update_is_forwarded_and_raw_input_is_typed() {
        let no_secrets = StreamRedactor::new(Vec::<String>::new());
        let mode_update = json!({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {
                "sessionId": "s1",
                "update": {
                    "sessionUpdate": "current_mode_update",
                    "currentModeId": ""
                }
            }
        });
        let frame = parse_wire_frame(mode_update.to_string().as_bytes(), false, &no_secrets)
            .unwrap()
            .unwrap();
        assert!(matches!(
            frame,
            WireFrame::Update {
                event: SessionEvent::ModeChanged { ref mode_id },
                ..
            } if mode_id.is_empty()
        ));

        let perm_typed = json!({
            "jsonrpc": "2.0",
            "id": "rpc-1",
            "method": "session/request_permission",
            "params": {
                "sessionId": "s1",
                "toolCall": {
                    "title": "Run tests",
                    "kind": "execute",
                    "rawInput": { "cmd": "cargo test", "arbitraryBlob": { "nested": true } }
                },
                "options": [
                    { "optionId": "once", "name": "Allow once", "kind": "allow_once" }
                ]
            }
        });
        let WireFrame::PermissionRequest(req) =
            parse_wire_frame(perm_typed.to_string().as_bytes(), false, &no_secrets)
                .unwrap()
                .unwrap()
        else {
            panic!("expected permission request");
        };
        assert_eq!(req.detail.as_deref(), Some("cargo test"));

        let perm_unknown = json!({
            "jsonrpc": "2.0",
            "id": "rpc-2",
            "method": "session/request_permission",
            "params": {
                "sessionId": "s1",
                "toolCall": {
                    "title": "Opaque tool",
                    "kind": "other",
                    "rawInput": { "opaque": [1, 2, 3] }
                },
                "options": [
                    { "optionId": "once", "name": "Allow once", "kind": "allow_once" }
                ]
            }
        });
        let WireFrame::PermissionRequest(req_unknown) =
            parse_wire_frame(perm_unknown.to_string().as_bytes(), false, &no_secrets)
                .unwrap()
                .unwrap()
        else {
            panic!("expected permission request");
        };
        assert_eq!(req_unknown.detail, None);

        // Secret straddling TOOL_OUTPUT_BYTES is redacted BEFORE truncation.
        let secret = "sk-boundary-secret-token-xyz";
        let boundary_redactor = StreamRedactor::new([secret.to_owned()]);
        let prefix = "a".repeat(TOOL_OUTPUT_BYTES - 5);
        let tool_line = json!({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {
                "sessionId": "s1",
                "update": {
                    "sessionUpdate": "tool_call",
                    "toolCallId": "tc1",
                    "title": "Dump",
                    "kind": "execute",
                    "status": "completed",
                    "content": [{ "type": "content", "content": { "type": "text", "text": format!("{prefix}{secret}") } }]
                }
            }
        });
        let WireFrame::Update {
            event:
                SessionEvent::ToolCall {
                    output: Some(output),
                    output_truncated,
                    ..
                },
            ..
        } = parse_wire_frame(tool_line.to_string().as_bytes(), false, &boundary_redactor)
            .unwrap()
            .unwrap()
        else {
            panic!("expected tool_call update");
        };
        assert!(output_truncated);
        assert!(output.len() <= TOOL_OUTPUT_BYTES);
        assert!(
            !output.contains("sk-bo"),
            "secret prefix leaked across cap boundary"
        );
    }
}
