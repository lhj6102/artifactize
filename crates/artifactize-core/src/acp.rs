//! Generic Agent Client Protocol (ACP) layer for Chat.
//!
//! Confines ACP JSON-RPC framing, protocol version 1 quirks, workspace trust, one-turn Verify
//! MCP bridge isolation, warm process pooling, deterministic handoff, and the 3-tier agent
//! catalog inside this anti-corruption layer. Evals never admit ACP backends.

pub mod catalog;
pub(crate) mod engine;
pub mod handoff;
pub(crate) mod pool;
pub(crate) mod redactor;
pub mod trust;
pub mod verify_bridge;
pub mod wire;

use std::{fmt, path::PathBuf, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::{
    agent::error::{Code, Failure},
    repository,
};

/// Maximum length of an ACP agent identifier (`[a-z][a-z0-9-]{0,63}`).
pub const MAX_AGENT_ID_BYTES: usize = 64;

/// Validated ACP agent identifier (`[a-z][a-z0-9-]{0,63}`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct AgentId(String);

impl AgentId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for AgentId {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let bytes = value.as_bytes();
        let valid = (1..=MAX_AGENT_ID_BYTES).contains(&bytes.len())
            && bytes[0].is_ascii_lowercase()
            && bytes[1..]
                .iter()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-');
        if valid {
            Ok(Self(value.to_owned()))
        } else {
            Err(format!(
                "Invalid ACP agent id {value:?}: expected 1–{MAX_AGENT_ID_BYTES} lowercase ASCII letters, digits or hyphens starting with a letter."
            ))
        }
    }
}

impl<'de> Deserialize<'de> for AgentId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for AgentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl AsRef<str> for AgentId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Canonical ACP `configOptions` category normalized at the edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigCategory {
    Model,
    Mode,
    ThoughtLevel,
    Other,
}

impl ConfigCategory {
    /// Dependency order for applying config options before `session/prompt`:
    /// `model` -> `mode` -> `thought_level` -> rest.
    pub(crate) fn apply_rank(self) -> u8 {
        match self {
            Self::Model => 0,
            Self::Mode => 1,
            Self::ThoughtLevel => 2,
            Self::Other => 3,
        }
    }
}

/// One selectable value of an ACP `configOption`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigChoice {
    pub value: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// True when the choice represents an agent-native auto-approval / bypass mode.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bypass_warning: bool,
}

/// One typed select `configOption` exposed to Chat and Settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigOption {
    pub id: String,
    pub name: String,
    pub category: ConfigCategory,
    pub current_value: String,
    pub choices: Vec<ConfigChoice>,
}

/// One user-selected `configOption` pair stored on `ChatTarget::Acp`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSelection {
    pub id: String,
    pub value: String,
    #[serde(default)]
    pub category: Option<ConfigCategory>,
}

/// Whether the ACP agent connected to Artifactize's one-turn Verify MCP bridge.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VerifyBridgeState {
    #[default]
    NotConnected,
    Connected,
}

/// A file location reported by an ACP tool call (UI hint only, never a security boundary).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolLocation {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

/// A bounded inline diff reported by an ACP tool call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDiff {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_text: Option<String>,
    pub new_text: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
}

/// Lifecycle state of a Chat tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// Status of one step in an ACP agent plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlanStepStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

/// One step in an ACP agent plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanStep {
    pub content: String,
    pub status: PlanStepStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
}

/// Kind of permission option advertised by an ACP agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionOptionKind {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
}

impl PermissionOptionKind {
    pub fn may_persist_in_agent_settings(self) -> bool {
        matches!(self, Self::AllowAlways | Self::RejectAlways)
    }
}

/// One validated option on an ACP permission card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionOption {
    pub option_id: String,
    pub name: String,
    pub kind: PermissionOptionKind,
    /// Disclosed in UI when the agent may persist this choice in its own settings files.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub persists_in_agent_settings: bool,
}

/// SQLite CAS status for `chat_pending_permissions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PermissionStatus {
    Pending,
    Selected,
    Delivering,
    Delivered,
    Cancelled,
    Expired,
}

impl PermissionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "PENDING",
            Self::Selected => "SELECTED",
            Self::Delivering => "DELIVERING",
            Self::Delivered => "DELIVERED",
            Self::Cancelled => "CANCELLED",
            Self::Expired => "EXPIRED",
        }
    }

    pub fn from_db(value: &str) -> Result<Self, String> {
        match value {
            "PENDING" => Ok(Self::Pending),
            "SELECTED" => Ok(Self::Selected),
            "DELIVERING" => Ok(Self::Delivering),
            "DELIVERED" => Ok(Self::Delivered),
            "CANCELLED" => Ok(Self::Cancelled),
            "EXPIRED" => Ok(Self::Expired),
            other => Err(format!("Unknown chat permission status {other:?}.")),
        }
    }
}

/// One permission prompt persisted in SQLite (`chat_pending_permissions`) and surfaced to UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRecord {
    pub session_id: crate::types::SessionId,
    pub owner_token: String,
    pub permission_id: String,
    pub rpc_request_id: String,
    pub title: String,
    pub tool_kind: crate::chat::ToolKind,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub options: Vec<PermissionOption>,
    pub status: PermissionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_option_id: Option<String>,
    pub created_at: crate::types::Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<crate::types::Timestamp>,
}

/// Process identity of the Artifactize host currently holding a warm ACP child for a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WarmOwner {
    pub pid: u32,
    pub start_time: u64,
}

impl WarmOwner {
    pub(crate) fn current() -> Result<Self, String> {
        let identity = crate::process::child_identity(std::process::id())
            .map_err(|error| error.to_string())?;
        Ok(Self {
            pid: identity.pid.get(),
            start_time: identity.start_time,
        })
    }
}

/// Persisted ACP session binding stored inside `chat_sessions.data` and guarded by atomic
/// `generation` CAS across worktree moves, agent switches, warm pool reuse, and `session/load`.
/// Confined to `artifactize-core` so raw ACP `remote_session_id` never crosses the public Chat API
/// (`C-ACP-EDGE-ISOLATION`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AcpBinding {
    pub(crate) agent_id: AgentId,
    #[serde(with = "crate::platform::path_serde")]
    pub(crate) worktree: PathBuf,
    pub(crate) checkout: repository::Identity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) remote_session_id: Option<String>,
    pub(crate) generation: u64,
    /// Last send confirmed durably saved with `Kind::Answer(Completed(_))`.
    #[serde(default)]
    pub(crate) settled_send: usize,
    /// `(resolved_executable, executable_sha256, args_hash, auth_generation)`.
    #[serde(default)]
    pub(crate) runtime_fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) warm_owner: Option<WarmOwner>,
}

/// Structured ACP transport or setup failure mapped cleanly to `agent::error::Failure`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    Missing {
        agent: String,
        hint: String,
    },
    TooOld {
        agent: String,
        version: semver::Version,
        minimum: semver::Version,
    },
    SignedOut {
        agent: String,
        hint: String,
    },
    Quota {
        agent: String,
    },
    RateLimit {
        agent: String,
    },
    TrustRequired(Box<trust::TrustRequirement>),
    ModeMismatch {
        requested: String,
        actual: Option<String>,
    },
    Failed(String),
}

impl Problem {
    pub(crate) fn classify_text(agent: &str, hint: &str, text: &str) -> Option<Self> {
        let lower = text.to_ascii_lowercase();
        let contains = |markers: &[&str]| markers.iter().any(|marker| lower.contains(marker));
        if contains(&[
            "authentication required",
            "auth_required",
            "unauthenticated",
            "not authenticated",
            "not logged in",
            "not signed in",
            "login required",
            "select an authentication method",
            "no auth method",
            "credentials not found",
            "no browser is available here",
        ]) {
            Some(Self::SignedOut {
                agent: agent.to_owned(),
                hint: hint.to_owned(),
            })
        } else if contains(&[
            "quota_exhausted",
            "quota exhausted",
            "quota exceeded",
            "usage limit reached",
            "usage quota",
        ]) {
            Some(Self::Quota {
                agent: agent.to_owned(),
            })
        } else if contains(&[
            "resource_exhausted",
            "rate_limit_exceeded",
            "rate limit",
            "error 429",
            "code 429",
        ]) || lower.starts_with("429 ")
        {
            Some(Self::RateLimit {
                agent: agent.to_owned(),
            })
        } else {
            None
        }
    }
}

impl From<Problem> for Failure {
    fn from(problem: Problem) -> Self {
        match problem {
            Problem::Missing { agent, hint } => Self::new(
                Code::Authentication,
                format!("{agent} is not installed on PATH. {hint}"),
            ),
            Problem::TooOld {
                agent,
                version,
                minimum,
            } => Self::new(
                Code::AgentError,
                format!("{agent} {version} is too old; update to {minimum} or newer."),
            ),
            Problem::SignedOut { agent, hint } => Self::new(
                Code::Authentication,
                format!("{agent} requires sign-in. {hint}"),
            ),
            Problem::Quota { agent } => Self::new(
                Code::Quota,
                format!(
                    "Your {agent} account reached its usage quota. Try again when your quota resets."
                ),
            ),
            Problem::RateLimit { agent } => Self::new(
                Code::RateLimit,
                format!("{agent} is rate limiting this account. Try again later."),
            ),
            Problem::TrustRequired(req) => Self::new(
                Code::AgentError,
                format!(
                    "Workspace trust is required before starting {} in {}.",
                    req.agent_id,
                    req.scope.display_label()
                ),
            ),
            Problem::ModeMismatch { requested, actual } => Self::new(
                Code::AgentError,
                match actual {
                    Some(actual) => format!(
                        "ACP agent did not confirm requested mode {requested:?} (reported {actual:?}); refusing silent approval-mode downgrade."
                    ),
                    None => format!(
                        "Requested mode {requested:?} is no longer offered by the ACP agent; refusing silent approval-mode downgrade."
                    ),
                },
            ),
            Problem::Failed(message) => Self::new(Code::ProviderError, message),
        }
    }
}
