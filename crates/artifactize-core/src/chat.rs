//! Artifactize's worktree-bound coding conversation.
//!
//! Unifies agentic CLI backends on ACP (`ChatTarget::Acp`) alongside direct HTTP API providers
//! (`ChatTarget::Direct`), while preserving read-only historical access for retired
//! `LegacyClaudeAgent` sessions (`C-CHAT-ACP-UNIFIED`, `C-TRUST-BEFORE-WORKTREE`,
//! `C-ONE-TURN-OWNERSHIP`, `C-ACP-EDGE-ISOLATION`).

mod details;
#[cfg(test)]
mod tests;
pub(crate) mod tools;
mod transcript;

use std::{
    collections::{BTreeMap, btree_map::Entry as Slot},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::{
    acp::{
        self, AcpBinding, AgentId, ConfigCategory, ConfigOption, ConfigSelection,
        VerifyBridgeState, WarmOwner,
        catalog::{self, AgentAvailability},
        engine::{AcpTurnInput, VerifiedAcpLaunch},
        pool::{IDLE_TTL, Pool},
        trust::{TrustRecord, TrustRequirement, TrustScope},
    },
    agent::{
        self,
        session::{Answer, Conversation, Kind, Recorder, Send},
    },
    auth::providers,
    config::{CustomBackendName, DirectBackend, HttpProvider, ModelId, Provider},
    llm,
    monitor::live::{self, Branch, Place},
    repository, runs,
    store::{self, Receipts},
    types::{SessionId, Timestamp},
};

pub use crate::store::chat::ChatWatermark;
pub use details::ToolKind;
pub use transcript::{Block, RunCard, ToolResult, Turn, TurnState};

pub(crate) fn recorded_outcome(state: &Path, id: &SessionId) -> Option<Outcome> {
    Conversation::load(&path(state, id))
        .ok()
        .flatten()
        .as_ref()
        .and_then(transcript::last_outcome)
}

/// The Chat branch choices and the sessions observed while discovering them.
pub struct BranchListing {
    pub branches: Vec<Branch>,
    pub sessions: Vec<Session>,
}

/// A description comes from the first nonempty prompt line, not another provider call.
const TITLE_CHARS: usize = 72;
/// Limit one prompt so a caller cannot overwhelm the transcript and model request at once.
const PROMPT_BYTES: usize = 256 * 1024;
/// A turn checks for a durable stop from another app even when the provider sends no data.
const STOP_CHECK: Duration = Duration::from_millis(250);
/// Extra slack after `IDLE_TTL` before the background warm-pool timer reaps an idle child.
const IDLE_REAP_MARGIN: Duration = Duration::from_secs(1);

/// Direct HTTP provider selection used by `ChatTarget::Direct` and `agent::chat::turn`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectSelection {
    pub backend: DirectBackend,
    pub model: ModelId,
    #[serde(default)]
    pub effort: Option<String>,
}

/// The execution target chosen for a Chat session (`T-ILLEGAL-STATES`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "engine", rename_all = "camelCase")]
pub enum ChatTarget {
    /// Direct HTTP API provider (Codex OAuth, OpenAI, Anthropic, Gemini, or custom HTTP).
    Direct {
        backend: DirectBackend,
        model: ModelId,
        #[serde(default)]
        effort: Option<String>,
    },
    /// Unified ACP agentic CLI backend (built-in preset, Registry-installed, or custom).
    Acp {
        agent: AgentId,
        #[serde(default)]
        config: Vec<ConfigSelection>,
    },
    /// Retired `claude-agent` Chat target kept readable for historical sessions; cannot run new turns.
    LegacyClaudeAgent {
        model: ModelId,
        #[serde(default)]
        effort: Option<String>,
    },
}

impl ChatTarget {
    pub fn direct(selection: DirectSelection) -> Self {
        Self::Direct {
            backend: selection.backend,
            model: selection.model,
            effort: selection.effort,
        }
    }

    pub fn acp(agent: AgentId, config: Vec<ConfigSelection>) -> Self {
        Self::Acp { agent, config }
    }

    pub fn is_legacy_claude_agent(&self) -> bool {
        matches!(self, Self::LegacyClaudeAgent { .. })
    }

    pub fn acp_agent(&self) -> Option<&AgentId> {
        match self {
            Self::Acp { agent, .. } => Some(agent),
            _ => None,
        }
    }

    pub fn engine_label(&self) -> String {
        match self {
            Self::Direct { backend, model, .. } => format!("Direct ({backend} / {model})"),
            Self::Acp { agent, .. } => format!("ACP ({agent})"),
            Self::LegacyClaudeAgent { model, .. } => format!("Legacy Claude Agent ({model})"),
        }
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum SendError {
    #[error("Choose a Chat agent or model before sending a Chat message.")]
    NoModel,
    #[error("Workspace trust is required before launching {}.", .0.agent_name)]
    TrustRequired(Box<TrustRequirement>),
    #[error("{0}")]
    LegacyRetired(String),
    #[error("{0}")]
    Failed(String),
}

impl From<String> for SendError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum MoveError {
    #[error("Not an available Chat worktree: {0}")]
    Invalid(String),
    #[error("Stop the running turn first")]
    Busy,
    #[error("{0}")]
    Failed(String),
}

/// Whether an accepted quit could read active ownership and deliver Stop to those turns.
#[derive(Debug)]
pub enum StopRunningError {
    Unreadable(String),
    Turns(Vec<String>),
}

/// The state of the latest accepted turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    Running,
    Completed,
    Failed,
    Stopped,
    Interrupted,
}

/// The workspace view last used in this session, independent of its turn activity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionView {
    #[default]
    Conversation,
    Graph,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Activity {
    outcome: Outcome,
    unseen: bool,
}

/// Edge deserialization wire representation for `Session` (`T-PARSE-AT-EDGE`).
///
/// Decodes legacy `model` / `selection` fields as raw strings bypassing `Backend::from_str`
/// so historical sessions pointing at `"antigravity"` or `"claude-agent"` always open cleanly.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionWire {
    id: SessionId,
    title: String,
    #[serde(with = "crate::platform::path_serde")]
    worktree: PathBuf,
    #[serde(default)]
    repository: Option<repository::Identity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    target: Option<ChatTarget>,
    #[serde(default, alias = "selection", skip_serializing)]
    model: Option<LegacyModelWire>,
    #[serde(default)]
    view: SessionView,
    created_at: Timestamp,
    updated_at: Timestamp,
    #[serde(default)]
    activity: Option<Activity>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::platform::path_serde::option"
    )]
    moved_from: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    acp_binding: Option<AcpBinding>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    legacy_migration_notice: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous_engine: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyModelWire {
    backend: String,
    model: String,
    #[serde(default)]
    effort: Option<String>,
}

impl LegacyModelWire {
    fn migrate(self) -> Option<ChatTarget> {
        let backend = self.backend.trim();
        let effort = self
            .effort
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        if backend == "antigravity" {
            let mut config = Vec::new();
            let model = self.model.trim();
            if !model.is_empty() {
                config.push(ConfigSelection {
                    id: "model".into(),
                    value: model.to_owned(),
                    category: Some(ConfigCategory::Model),
                });
            }
            if let Some(level) = effort {
                config.push(ConfigSelection {
                    id: "thought_level".into(),
                    value: level,
                    category: Some(ConfigCategory::ThoughtLevel),
                });
            }
            return Some(ChatTarget::Acp {
                agent: "antigravity"
                    .parse()
                    .expect("antigravity is a valid AgentId"),
                config,
            });
        }
        if backend == "claude-agent" {
            let model: ModelId = self.model.parse().ok()?;
            return Some(ChatTarget::LegacyClaudeAgent { model, effort });
        }
        if let Some(provider) = HttpProvider::named(backend) {
            let model: ModelId = self.model.parse().ok()?;
            return Some(ChatTarget::Direct {
                backend: DirectBackend::Builtin { provider },
                model,
                effort,
            });
        }
        if let Ok(name) = backend.parse::<CustomBackendName>() {
            let model: ModelId = self.model.parse().ok()?;
            return Some(ChatTarget::Direct {
                backend: DirectBackend::Custom { name },
                model,
                effort,
            });
        }
        None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "SessionWire", into = "SessionWire")]
pub struct Session {
    pub id: SessionId,
    pub title: String,
    pub worktree: PathBuf,
    pub repository: Option<repository::Identity>,
    pub target: Option<ChatTarget>,
    pub view: SessionView,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    activity: Option<Activity>,
    pub(crate) moved_from: Option<PathBuf>,
    pub acp_binding: Option<AcpBinding>,
    /// Live ownership of a turn, read separately from the saved session.
    pub running: bool,
    /// Derived live from `running && has_pending_permission`; never persisted in `Activity`.
    pub waiting_permission: bool,
    /// Live ACP `configOptions` snapshot while a turn is running.
    pub live_config_options: Vec<ConfigOption>,
    /// Live Verify MCP bridge state while an ACP turn is running.
    pub verify_bridge_state: VerifyBridgeState,
    /// True when this historical session was saved on a legacy engine (`claude-agent` or legacy
    /// `antigravity`) so the UI and handoff surface the transition explicitly (`C-NO-SILENT-REGRESSION`).
    pub legacy_migration_notice: bool,
    /// Engine label the conversation ran on before a picker switch, for legacy transcripts whose
    /// `Kind::Send` events predate `Send.engine` (`C-NO-SILENT-REGRESSION`).
    pub(crate) previous_engine: Option<String>,
    pub unavailable: Option<String>,
}

impl From<SessionWire> for Session {
    fn from(wire: SessionWire) -> Self {
        let migrated_from_legacy_antigravity = wire.target.is_none()
            && wire
                .model
                .as_ref()
                .is_some_and(|legacy| legacy.backend.trim() == "antigravity");
        let target = wire
            .target
            .or_else(|| wire.model.and_then(LegacyModelWire::migrate));
        let legacy_migration_notice = wire.legacy_migration_notice
            || target
                .as_ref()
                .is_some_and(ChatTarget::is_legacy_claude_agent)
            || migrated_from_legacy_antigravity;
        let previous_engine = wire
            .previous_engine
            .or_else(|| migrated_from_legacy_antigravity.then(|| "Legacy Antigravity".to_owned()));
        Self {
            id: wire.id,
            title: wire.title,
            worktree: wire.worktree,
            repository: wire.repository,
            target,
            view: wire.view,
            created_at: wire.created_at,
            updated_at: wire.updated_at,
            activity: wire.activity,
            moved_from: wire.moved_from,
            acp_binding: wire.acp_binding,
            running: false,
            waiting_permission: false,
            live_config_options: Vec::new(),
            verify_bridge_state: VerifyBridgeState::NotConnected,
            legacy_migration_notice,
            previous_engine,
            unavailable: None,
        }
    }
}

impl From<Session> for SessionWire {
    fn from(session: Session) -> Self {
        Self {
            id: session.id,
            title: session.title,
            worktree: session.worktree,
            repository: session.repository,
            target: session.target,
            model: None,
            view: session.view,
            created_at: session.created_at,
            updated_at: session.updated_at,
            activity: session.activity,
            moved_from: session.moved_from,
            acp_binding: session.acp_binding,
            legacy_migration_notice: session.legacy_migration_notice,
            previous_engine: session.previous_engine,
        }
    }
}

impl Session {
    /// Stable across branch switches or removal of the saved checkout.
    pub fn branch(&self) -> Place {
        Place::for_checkout(
            &self.worktree,
            self.repository
                .as_ref()
                .unwrap_or(&repository::Identity::default()),
        )
    }

    pub fn label(&self) -> crate::monitor::live::Label {
        crate::monitor::live::Label::for_identity(
            &self.worktree,
            &self.repository.clone().unwrap_or_default(),
        )
    }

    pub fn outcome(&self) -> Option<Outcome> {
        self.activity.map(|activity| activity.outcome)
    }

    pub fn unseen(&self) -> bool {
        self.activity.is_some_and(|activity| activity.unseen)
    }

    pub(crate) fn needs_activity(&self) -> bool {
        self.activity.is_none() || (self.outcome() == Some(Outcome::Running) && !self.running)
    }

    pub(crate) fn refresh_activity(&mut self, outcome: Option<Outcome>) {
        let owner_gone = self.outcome() == Some(Outcome::Running) && !self.running;
        if !self.needs_activity() {
            return;
        }
        // Do not present legacy history as new. If an owner died after flushing its
        // answer and before updating the catalog, prefer that answer over Interrupted.
        self.activity = if owner_gone {
            Some(Activity {
                outcome: outcome.unwrap_or(Outcome::Interrupted),
                unseen: true,
            })
        } else {
            outcome.map(|outcome| Activity {
                outcome: if self.running {
                    Outcome::Running
                } else {
                    outcome
                },
                unseen: false,
            })
        };
    }

    pub(crate) fn settle_from(&mut self, turn: &Self) {
        self.activity = turn.activity;
        self.updated_at = self.updated_at.max(turn.updated_at);
    }

    pub(crate) fn mark_seen(&mut self) -> bool {
        self.activity
            .as_mut()
            .is_some_and(|activity| std::mem::take(&mut activity.unseen))
    }

    fn matches(&self, words: &[String]) -> bool {
        if words.is_empty() {
            return true;
        }
        let label = self.label();
        let fields = [
            self.title.to_lowercase(),
            label.project.to_lowercase(),
            label.branch.to_lowercase(),
        ];
        words
            .iter()
            .all(|word| fields.iter().any(|field| field.contains(word)))
    }

    fn check_checkout(&self) -> Result<(), String> {
        if !self.worktree.is_dir() {
            return Err("This Chat's worktree no longer exists.".into());
        }
        verify_checkout(&self.worktree, self.repository.as_ref())
    }

    fn with_checkout(mut self) -> Self {
        self.unavailable = self.check_checkout().err();
        self.legacy_migration_notice = self.legacy_migration_notice
            || self
                .target
                .as_ref()
                .is_some_and(ChatTarget::is_legacy_claude_agent);
        self
    }
}

async fn checkout_for(
    worktree: &Path,
    expected: Option<&Place>,
) -> Result<(PathBuf, repository::Identity), String> {
    let worktree =
        crate::workspace::canonical_target(worktree).map_err(|error| error.to_string())?;
    let expected = expected.cloned();
    crate::task::blocking(move || {
        let identity = repository::identify(&worktree);
        if expected.is_some_and(|expected| expected != Place::for_checkout(&worktree, &identity)) {
            return Err("This branch is no longer checked out in the worktree.".into());
        }
        Ok((worktree, identity))
    })
    .await?
}

fn verify_checkout(worktree: &Path, saved: Option<&repository::Identity>) -> Result<(), String> {
    checkout_refusal(saved, &repository::identify(worktree)).map_or(Ok(()), Err)
}

fn checkout_refusal(
    saved: Option<&repository::Identity>,
    current: &repository::Identity,
) -> Option<String> {
    match saved {
        Some(saved) if saved == current => None,
        Some(saved) => Some(match &saved.branch {
            Some(branch) => format!("Check out {branch} in this worktree to continue this Chat."),
            None => "Restore this Chat's original checkout to continue.".into(),
        }),
        None if *current == repository::Identity::default() => None,
        None => {
            Some("This Chat was saved without a branch. Start a new Chat for this checkout.".into())
        }
    }
}

#[derive(Debug, Clone)]
pub struct View {
    pub session: Session,
    pub running: bool,
    pub waiting_permission: bool,
    pub turns: Vec<Turn>,
    pub live_config_options: Vec<ConfigOption>,
    pub verify_bridge_state: VerifyBridgeState,
    /// The engine or agent the conversation last ran on, when the session's target is another:
    /// the next message continues there with a Handoff.
    pub handoff_from: Option<String>,
}

#[derive(Clone)]
pub struct Chat {
    state: PathBuf,
    executable: Option<PathBuf>,
    active: Arc<ActiveTurns>,
    pool: Arc<Pool>,
    permission_notify: Arc<Notify>,
}

#[derive(Default)]
struct ActiveTurns {
    state: Mutex<Turns>,
    settled: Notify,
}

#[derive(Default)]
struct Turns {
    closed: bool,
    turns: BTreeMap<SessionId, ActiveEntry>,
}

#[derive(Clone)]
struct ActiveEntry {
    cancel: CancellationToken,
    owner_token: Option<String>,
    worktree: Option<PathBuf>,
    /// A Stop came before the turn published its ownership: the turn commits the durable Stop
    /// itself once it does (`record_owner`).
    stopped: bool,
}

/// Resolved execution engine for a single accepted Chat turn (`T-ILLEGAL-STATES`).
enum ChatEngine {
    Direct {
        selection: DirectSelection,
        engine_label: String,
        previous_engine_label: Option<String>,
        prior_turns_count: usize,
    },
    Acp {
        config: Vec<ConfigSelection>,
        launch: Box<VerifiedAcpLaunch>,
        previous_engine_label: Option<String>,
        prior_turns: Vec<Turn>,
        last_outcome: Option<Outcome>,
    },
}

impl Chat {
    pub fn new(state: PathBuf, executable: PathBuf) -> Self {
        Self {
            state,
            executable: Some(executable),
            active: Arc::default(),
            pool: Arc::new(Pool::default()),
            permission_notify: Arc::new(Notify::new()),
        }
    }

    /// Desktop discovers the packaged CLI when a session needs it, not at app startup.
    pub fn discover(state: PathBuf) -> Self {
        Self {
            state,
            executable: None,
            active: Arc::default(),
            pool: Arc::new(Pool::default()),
            permission_notify: Arc::new(Notify::new()),
        }
    }

    fn context(&self, worktree: &Path) -> Result<runs::Context, String> {
        match &self.executable {
            Some(executable) => runs::Context::new(worktree, Some(&self.state), executable),
            None => runs::Context::discover(worktree, Some(&self.state)),
        }
    }

    /// Stop accepting work, commit durable Stop in SQLite before cancelling owned turns, wait for
    /// their saved outcomes and ownership release, and drain all warm ACP children.
    /// Verify Runs already admitted remain independent of Chat.
    pub fn close(&self) -> impl std::future::Future<Output = ()> + use<> {
        let entries: Vec<(SessionId, ActiveEntry)> = {
            let mut active = self.active.state.lock().expect("Chat active turns lock");
            active.closed = true;
            active
                .turns
                .iter_mut()
                .map(|(id, entry)| {
                    entry.stopped |= entry.owner_token.is_none();
                    (id.clone(), entry.clone())
                })
                .collect()
        };
        let state = self.state.clone();
        let active = self.active.clone();
        let pool = self.pool.clone();
        let permission_notify = self.permission_notify.clone();
        async move {
            // Uniform Stop path (`C-ONE-TURN-OWNERSHIP`): commit SQLite stop_requested +
            // permission cancellation FIRST, and only fire the in-memory CancellationToken for
            // turns whose Stop transaction succeeded (or that have no SQLite active turn yet).
            let mut stopped_ids = std::collections::BTreeSet::new();
            for (id, entry) in &entries {
                if let (Some(token), Some(worktree)) = (&entry.owner_token, &entry.worktree) {
                    let mut committed = false;
                    for attempt in 0..5 {
                        if let Ok(receipts) = Receipts::open(&state, worktree).await {
                            let stopped = receipts.stop_chat_by_token(id, token).await;
                            let closed = receipts.close().await;
                            if stopped.and(closed).is_ok() {
                                committed = true;
                                break;
                            }
                        }
                        if attempt < 4 {
                            tokio::time::sleep(Duration::from_millis(20)).await;
                        }
                    }
                    if committed {
                        stopped_ids.insert(id.clone());
                        entry.cancel.cancel();
                    }
                } else {
                    stopped_ids.insert(id.clone());
                    entry.cancel.cancel();
                }
            }
            permission_notify.notify_waiters();
            loop {
                let settled = active.settled.notified();
                tokio::pin!(settled);
                settled.as_mut().enable();
                if active
                    .state
                    .lock()
                    .expect("Chat active turns lock")
                    .turns
                    .keys()
                    .all(|id| !stopped_ids.contains(id))
                {
                    break;
                }
                settled.await;
            }
            // Terminate all warm ACP children and clear their warm_owner references.
            let drained = pool.drain_all();
            let current_owner = WarmOwner::current().ok();
            for child in drained {
                let session_id = child.session_id.clone();
                let generation = child.generation;
                let _ = child.terminate().await;
                if let Some(warm_owner) = current_owner
                    && let Ok(session) = store::chat::load(&state, &session_id).await
                    && let Ok(receipts) = Receipts::open(&state, &session.worktree).await
                {
                    let _ = receipts
                        .clear_acp_warm_owner(&session_id, generation, warm_owner)
                        .await;
                    let _ = receipts.close().await;
                }
            }
        }
    }

    /// Reconcile exact turn ownership after a process dies without a final change hint.
    /// Returns only sessions this observer actually reconciled.
    pub async fn reconcile_interrupted(&self) -> Result<Vec<SessionId>, String> {
        let mut changed = Vec::new();
        for (session, owner) in store::chat::active_turns(&self.state, false).await? {
            let receipts = Receipts::open(&self.state, &session.worktree).await?;
            let settled = receipts
                .finish_chat(&session.id, &owner, Some(&session), None)
                .await;
            let closed = receipts.close().await;
            if settled.and_then(|settled| closed.map(|()| settled))? {
                changed.push(session.id);
            }
        }
        let publisher = crate::changes::Publisher::new(&self.state);
        for session in &changed {
            publisher.notify(crate::changes::Change::SessionInvalidated(session.clone()));
        }
        Ok(changed)
    }

    /// The starting target for a new session: the worktree's last available choice, or the first
    /// ready ACP Chat agent, or the first available direct HTTP provider model.
    pub async fn default_target(&self, worktree: &Path) -> Result<Option<ChatTarget>, String> {
        self.default_target_for(worktree, None).await
    }

    pub async fn default_target_for(
        &self,
        worktree: &Path,
        branch: Option<&Place>,
    ) -> Result<Option<ChatTarget>, String> {
        let (worktree, identity) = checkout_for(worktree, branch).await?;
        self.starting_target(&worktree, &identity).await
    }

    async fn starting_target(
        &self,
        worktree: &Path,
        identity: &repository::Identity,
    ) -> Result<Option<ChatTarget>, String> {
        if let Some(target) = self.last_target(worktree, identity).await?
            && !target.is_legacy_claude_agent()
            && let Ok(resolved) = self.resolve_target(worktree, target, None).await
        {
            return Ok(Some(resolved));
        }

        // Prefer ready ACP Chat agents in catalog order.
        if let Ok(entries) = catalog::list_catalog(Some(&self.state), Some(worktree)) {
            let probes = futures_util::future::join_all(entries.iter().map(|entry| {
                catalog::probe_agent(&entry.id, Some(&self.state), Some(worktree), false)
            }))
            .await;
            for (entry, availability) in entries.into_iter().zip(probes) {
                if let AgentAvailability::Ready { config_options, .. } = availability {
                    let config = default_config_selections(&config_options);
                    return Ok(Some(ChatTarget::Acp {
                        agent: entry.id,
                        config,
                    }));
                }
            }
        }

        // Fall back to direct HTTP providers (Codex, OpenAI, Anthropic, Gemini + HTTP custom).
        let providers = providers::list(Some(&self.state), Some(worktree))?;
        let mut direct_backends: Vec<DirectBackend> = HttpProvider::ALL
            .into_iter()
            .map(|provider| DirectBackend::Builtin { provider })
            .collect();
        for (name, view) in providers.custom {
            if matches!(
                view,
                providers::CustomView::Alias {
                    provider: Provider::ClaudeAgent
                }
            ) {
                continue;
            }
            if let Ok(custom) = name.as_str().parse::<CustomBackendName>() {
                direct_backends.push(DirectBackend::Custom { name: custom });
            }
        }
        let listings = futures_util::future::join_all(direct_backends.iter().map(|backend| {
            llm::models::list(backend.to_backend(), Some(&self.state), Some(worktree))
        }))
        .await;
        Ok(direct_backends
            .into_iter()
            .zip(listings)
            .find_map(|(backend, listing)| {
                let listing = listing.ok()?;
                listing
                    .models
                    .into_iter()
                    .next()
                    .map(|model| ChatTarget::Direct {
                        backend,
                        model: model.slug,
                        effort: model.default_effort,
                    })
            }))
    }

    async fn last_target(
        &self,
        workspace: &Path,
        checkout: &repository::Identity,
    ) -> Result<Option<ChatTarget>, String> {
        let branch = Place::for_checkout(workspace, checkout);
        Ok(store::chat::for_branch(&self.state, &branch)
            .await?
            .into_iter()
            .find_map(|session| {
                session
                    .target
                    .filter(|target| !target.is_legacy_claude_agent())
            }))
    }

    /// Persist an empty session, with no target unless one is explicitly selected.
    pub async fn create(
        &self,
        worktree: &Path,
        selection: Option<ChatTarget>,
    ) -> Result<Session, String> {
        self.create_for_branch(worktree, None, selection).await
    }

    /// Create only on the checkout the person selected, not a branch switched in meanwhile.
    pub async fn create_for_branch(
        &self,
        workspace: &Path,
        branch: Option<&Place>,
        selection: Option<ChatTarget>,
    ) -> Result<Session, String> {
        let (workspace, repository) = checkout_for(workspace, branch).await?;
        let worktree = workspace.as_path();
        crate::config::read_workspace_config(worktree).map_err(|error| error.to_string())?;
        let target = if let Some(selection) = selection {
            let previous = self.last_target(worktree, &repository).await?;
            Some(
                self.resolve_target(worktree, selection, previous.as_ref())
                    .await?,
            )
        } else {
            None
        };
        let now = crate::broker::now();
        let legacy_migration_notice = target
            .as_ref()
            .is_some_and(ChatTarget::is_legacy_claude_agent);
        let session = Session {
            id: format!("chat-{}", agent::uuid()?).parse()?,
            title: "New chat".into(),
            worktree: worktree.into(),
            repository: Some(repository),
            target,
            view: SessionView::default(),
            created_at: now,
            updated_at: now,
            activity: None,
            moved_from: None,
            acp_binding: None,
            running: false,
            waiting_permission: false,
            live_config_options: Vec::new(),
            verify_bridge_state: VerifyBridgeState::NotConnected,
            legacy_migration_notice,
            previous_engine: None,
            unavailable: None,
        };
        let session = crate::task::blocking(move || session.with_checkout()).await?;
        if let Some(reason) = &session.unavailable {
            return Err(reason.clone());
        }
        let receipts = Receipts::open(&self.state, worktree).await?;
        let result = receipts.create_chat(&session).await;
        let closed = receipts.close().await;
        result.and(closed)?;
        Ok(session)
    }

    /// Discover the live choices used by the Chat branch picker, independently of Verify search.
    pub async fn branches(&self, launch: Option<&Path>) -> Result<BranchListing, String> {
        let runs = store::read_catalog(&self.state).await?;
        let sessions = self.list(None, None).await?;
        let launch = launch.map(Path::to_owned);
        crate::task::blocking(move || {
            let mut branches =
                live::branches(&runs, launch.as_deref(), &mut live::Discovery::default()).branches;
            for session in sessions
                .iter()
                .filter(|session| session.unavailable.is_none())
            {
                let place = session.branch();
                if !branches.iter().any(|branch| branch.place == place) {
                    let label = session.label();
                    branches.push(Branch {
                        place,
                        project: label.project,
                        name: label.branch,
                        workspace: session.worktree.clone(),
                        runs: Vec::new(),
                    });
                }
            }
            BranchListing { branches, sessions }
        })
        .await
    }

    /// Rebind the saved conversation to a live choice, excluding active turns in any process.
    /// Evicts any warm ACP child owned by this process for the moved session.
    pub async fn set_worktree(
        &self,
        id: &SessionId,
        worktree: &Path,
        launch: Option<&Path>,
    ) -> Result<Session, MoveError> {
        let path = crate::platform::path_text(worktree);
        let invalid = || MoveError::Invalid(path.clone());
        let branch = self
            .branches(launch)
            .await
            .map_err(MoveError::Failed)?
            .branches
            .into_iter()
            .find(|branch| crate::platform::path_text(&branch.workspace) == path)
            .ok_or_else(invalid)?;
        let (workspace, repository) = checkout_for(&branch.workspace, Some(&branch.place))
            .await
            .map_err(|_| invalid())?;
        let receipts = Receipts::open(&self.state, &workspace)
            .await
            .map_err(MoveError::Failed)?;
        let result = receipts
            .set_chat_worktree(&self.state, id, workspace, repository)
            .await;
        let closed = receipts.close().await;
        let (saved, changed) = result
            .and_then(|saved| closed.map(|()| saved))
            .map_err(MoveError::Failed)?
            .ok_or(MoveError::Busy)?;
        if changed {
            if let Some(child) = self.pool.take_any(id) {
                let _ = child.terminate().await;
            }
            crate::changes::Publisher::new(&self.state)
                .notify(crate::changes::Change::SessionInvalidated(id.clone()));
        }
        crate::task::blocking(move || saved.with_checkout())
            .await
            .map_err(MoveError::Failed)
    }

    /// Keep the catalog's activity order while matching every word across its displayed labels.
    pub async fn list(
        &self,
        worktree: Option<&Path>,
        query: Option<&str>,
    ) -> Result<Vec<Session>, String> {
        let words = query
            .unwrap_or_default()
            .split_whitespace()
            .map(str::to_lowercase)
            .collect::<Vec<_>>();
        let sessions = store::chat::list(&self.state, worktree).await?;
        crate::task::blocking(move || {
            let mut identities = BTreeMap::new();
            sessions
                .into_iter()
                .filter(|session| session.matches(&words))
                .map(|mut session| {
                    let checkout =
                        identities
                            .entry(session.worktree.clone())
                            .or_insert_with(|| {
                                session
                                    .worktree
                                    .is_dir()
                                    .then(|| repository::identify(&session.worktree))
                            });
                    session.unavailable = match checkout {
                        Some(current) => checkout_refusal(session.repository.as_ref(), current),
                        None => Some("This Chat's worktree no longer exists.".into()),
                    };
                    session.legacy_migration_notice = session.legacy_migration_notice
                        || session
                            .target
                            .as_ref()
                            .is_some_and(ChatTarget::is_legacy_claude_agent);
                    session
                })
                .collect()
        })
        .await
    }

    /// Find a conversation by its former checkout, including a workspace within the worktree.
    pub async fn latest_for_unavailable_checkout(
        &self,
        workspace: &Path,
    ) -> Result<Option<Session>, String> {
        let workspace = workspace.to_path_buf();
        let workspace = crate::task::blocking(move || {
            let workspace = crate::workspace::canonical_target(&workspace)
                .map_err(|error| error.to_string())?;
            Ok::<_, String>(
                crate::repository::identify(&workspace)
                    .worktree_path
                    .is_none()
                    .then_some(workspace),
            )
        })
        .await??;
        let Some(workspace) = workspace else {
            return Ok(None);
        };
        let sessions = store::chat::list(&self.state, None).await?;
        crate::task::blocking(move || {
            sessions
                .into_iter()
                .filter(|session| {
                    let recorded = session
                        .repository
                        .as_ref()
                        .and_then(|repo| repo.worktree_path.as_deref())
                        .unwrap_or(&session.worktree);
                    crate::platform::paths::relative_beneath(&workspace, recorded).is_some()
                        || crate::platform::paths_equal(&workspace, recorded)
                })
                .map(Session::with_checkout)
                .find(|session| session.unavailable.is_some())
        })
        .await
    }

    /// The most recently active saved session on this branch, even if its checkout is gone.
    pub async fn latest_for_branch(&self, branch: &Place) -> Result<Option<Session>, String> {
        let mut sessions = store::chat::for_branch(&self.state, branch)
            .await?
            .into_iter();
        crate::task::blocking(move || sessions.next().map(Session::with_checkout)).await
    }

    /// Prefer the newest usable session for branch navigation. If the checkout moved and none
    /// can work on it, keep the newest saved conversation reachable with its diagnostic.
    pub async fn latest_for_navigation(&self, branch: &Place) -> Result<Option<Session>, String> {
        let sessions = store::chat::for_branch(&self.state, branch).await?;
        crate::task::blocking(move || {
            let mut fallback = None;
            for session in sessions {
                let session = session.with_checkout();
                if session.unavailable.is_none() {
                    return Some(session);
                }
                if fallback.is_none() {
                    fallback = Some(session);
                }
            }
            fallback
        })
        .await
    }

    /// The latest session whose exact checkout can be worked on now.
    pub async fn latest_available_for_branch(
        &self,
        branch: &Place,
    ) -> Result<Option<Session>, String> {
        let sessions = store::chat::for_branch(&self.state, branch).await?;
        crate::task::blocking(move || {
            sessions
                .into_iter()
                .find(|session| session.check_checkout().is_ok())
        })
        .await
    }

    /// The oldest usable checkout anchors Current when there is no Verify workspace.
    /// Activity in a newer session must not redirect scans, edits or tools.
    pub async fn current_worktree(&self, branch: &Place) -> Result<Option<PathBuf>, String> {
        let sessions = store::chat::for_branch(&self.state, branch).await?;
        crate::task::blocking(move || {
            sessions
                .into_iter()
                .filter(|session| session.check_checkout().is_ok())
                .min_by(|a, b| (a.created_at, &a.id).cmp(&(b.created_at, &b.id)))
                .map(|session| session.worktree)
        })
        .await
    }

    /// Save a view without changing the session's activity order or requiring a live checkout.
    pub async fn set_view(&self, id: &SessionId, view: SessionView) -> Result<Session, String> {
        let session = store::chat::load(&self.state, id).await?;
        let receipts = Receipts::open(&self.state, &session.worktree).await?;
        let result = receipts.set_chat_view(&self.state, id, view).await;
        let closed = receipts.close().await;
        let (saved, changed) = result.and_then(|saved| closed.map(|()| saved))?;
        if changed {
            crate::changes::Publisher::new(&self.state)
                .notify(crate::changes::Change::SessionInvalidated(id.clone()));
        }
        crate::task::blocking(move || saved.with_checkout()).await
    }

    /// Acknowledge the latest settled outcome without changing the session's activity order.
    /// A turn still running will become unseen when it settles, even if this is called now.
    pub async fn mark_seen(&self, id: &SessionId) -> Result<(), String> {
        let session = store::chat::load(&self.state, id).await?;
        if !session.unseen() {
            return Ok(());
        }
        let receipts = Receipts::open(&self.state, &session.worktree).await?;
        let result = receipts.mark_chat_seen(&self.state, id).await;
        let closed = receipts.close().await;
        let changed = result.and_then(|changed| closed.map(|()| changed))?;
        if changed {
            crate::changes::Publisher::new(&self.state)
                .notify(crate::changes::Change::SessionInvalidated(id.clone()));
        }
        Ok(())
    }

    /// Chat records are append-only. Readers can detect a missed hint without parsing a
    /// transcript, with `None` covering a session that has not yet accepted a turn.
    pub fn transcript_size(&self, id: &SessionId) -> Option<u64> {
        crate::platform::open_regular(&path(&self.state, id))
            .and_then(|file| file.metadata())
            .ok()
            .map(|file| file.len())
    }

    /// Lightweight SQLite watermark across `(chat_sessions, chat_active_turns,
    /// chat_pending_permissions)` tracked by Desktop `watch.rs` alongside `transcript_size`.
    pub async fn watermark(&self, id: &SessionId) -> Result<Option<ChatWatermark>, String> {
        store::chat::watermark(&self.state, id).await
    }

    /// Evict any local warm ACP child whose committed `acp_binding.generation` has advanced
    /// (called on `Change::SessionInvalidated` so another process's move/switch terminates stale
    /// local children promptly).
    pub async fn evict_warm_if_stale(&self, id: &SessionId) {
        let committed_gen = store::chat::load(&self.state, id)
            .await
            .ok()
            .and_then(|session| session.acp_binding)
            .map(|binding| binding.generation);
        if let Some(child) = self.pool.take_if_generation_stale(id, committed_gen) {
            let _ = child.terminate().await;
        }
    }

    /// The model/agent's streamed text, rich ACP blocks, live permissions, and Run cards.
    pub async fn open(&self, id: &SessionId) -> Result<View, String> {
        let session = store::chat::load(&self.state, id).await?;
        let session = crate::task::blocking(move || session.with_checkout()).await?;
        let running = session.running;
        let waiting_permission = session.waiting_permission;
        let live_config_options = session.live_config_options.clone();
        let verify_bridge_state = session.verify_bridge_state;
        let live_permissions = store::chat::list_permissions(&self.state, id).await?;
        let conversation = Conversation::load(&path(&self.state, id))?;
        let handoff_from = session.target.as_ref().and_then(|target| {
            let next = target.engine_label();
            handoff_from(conversation.as_ref(), &next, || {
                session
                    .previous_engine
                    .as_ref()
                    .filter(|prev| *prev != &next)
                    .cloned()
            })
        });
        let mut turns = transcript::turns(conversation.as_ref(), running, &live_permissions);
        transcript::update_runs(&self.state, &session.worktree, id, &mut turns).await;
        Ok(View {
            session,
            running,
            waiting_permission,
            turns,
            live_config_options,
            verify_bridge_state,
            handoff_from,
        })
    }

    /// Save a Chat target for the next turn without adding a message. An accepted turn keeps
    /// the target it started with, even if the user changes the picker while it streams.
    pub async fn set_target(
        &self,
        id: &SessionId,
        selection: ChatTarget,
    ) -> Result<Session, String> {
        let session = self.available(id).await?;
        let selection = self
            .resolve_target(&session.worktree, selection, session.target.as_ref())
            .await?;
        let receipts = Receipts::open(&self.state, &session.worktree).await?;
        let saved = receipts.set_chat_target(&self.state, id, selection).await;
        let closed = receipts.close().await;
        let (saved, agent_changed) = saved.and_then(|result| closed.map(|()| result))?;
        if agent_changed && let Some(child) = self.pool.take_any(id) {
            let _ = child.terminate().await;
        }
        crate::changes::Publisher::new(&self.state)
            .notify(crate::changes::Change::SessionInvalidated(id.clone()));
        crate::task::blocking(move || saved.with_checkout()).await
    }

    /// Respond to a pending ACP permission card (`PENDING -> SELECTED` in one `IMMEDIATE`
    /// SQLite transaction) and wake any local owning turn immediately.
    pub async fn respond_permission(
        &self,
        id: &SessionId,
        permission_id: &str,
        option_id: &str,
    ) -> Result<bool, String> {
        let session = store::chat::load(&self.state, id).await?;
        let receipts = Receipts::open(&self.state, &session.worktree).await?;
        let updated = receipts
            .respond_chat_permission(id, permission_id, option_id)
            .await;
        let closed = receipts.close().await;
        let updated = updated.and_then(|changed| closed.map(|()| changed))?;
        if updated {
            self.permission_notify.notify_waiters();
            crate::changes::Publisher::new(&self.state)
                .notify(crate::changes::Change::SessionInvalidated(id.clone()));
        }
        Ok(updated)
    }

    /// Record explicit machine-local Workspace Trust for an ACP agent on the `shown_scope` and
    /// `shown` launcher fingerprint presented to the user. If either the checkout scope or the
    /// executable changed since the prompt was shown, consent is refused (`C-TRUST-BEFORE-WORKTREE`).
    pub async fn trust_workspace(
        &self,
        worktree: &Path,
        agent_id: &AgentId,
        shown_scope: &TrustScope,
        shown: &acp::trust::LauncherFingerprint,
    ) -> Result<TrustRecord, String> {
        let worktree =
            crate::workspace::canonical_target(worktree).map_err(|error| error.to_string())?;
        let state = self.state.clone();
        let agent_id = agent_id.clone();
        let (spec, scope, fingerprint) = crate::task::blocking(move || {
            let identity = repository::identify(&worktree);
            let spec = catalog::resolve_spec(&agent_id, Some(&state), Some(&worktree))?;
            let resolved = acp::trust::resolve_executable(
                &spec.program,
                Some(&worktree),
                spec.reject_worktree_executable,
            )?;
            let fingerprint = acp::trust::LauncherFingerprint::compute(&resolved, &spec.args)?;
            let scope = TrustScope::for_worktree(&worktree, &identity)?;
            Ok::<_, String>((spec, scope, fingerprint))
        })
        .await??;
        if scope != *shown_scope {
            return Err(format!(
                "The workspace checkout changed after you were asked to trust {}; send the message again to review the new scope.",
                spec.name
            ));
        }
        if fingerprint != *shown {
            return Err(format!(
                "{}'s launcher changed after you were asked to trust it; send the message again to review the new one.",
                spec.name
            ));
        }
        acp::trust::grant_trust(Some(&self.state), None, spec.id, scope, fingerprint).await
    }

    /// List all saved machine-local ACP Workspace Trust records.
    pub fn list_trust(&self) -> Result<Vec<TrustRecord>, String> {
        acp::trust::list_trust(Some(&self.state), None)
    }

    /// Revoke a saved Workspace Trust record and terminate any matching warm ACP children.
    pub async fn revoke_trust(
        &self,
        agent_id: &AgentId,
        scope: &TrustScope,
    ) -> Result<bool, String> {
        let removed = acp::trust::revoke_trust(Some(&self.state), None, agent_id, scope).await?;
        if removed {
            let drained = self.pool.drain_all();
            let current_owner = WarmOwner::current().ok();
            for child in drained {
                if &child.agent_id == agent_id {
                    let session_id = child.session_id.clone();
                    let generation = child.generation;
                    let _ = child.terminate().await;
                    if let Some(warm_owner) = current_owner
                        && let Ok(session) = store::chat::load(&self.state, &session_id).await
                        && let Ok(receipts) = Receipts::open(&self.state, &session.worktree).await
                    {
                        let _ = receipts
                            .clear_acp_warm_owner(&session_id, generation, warm_owner)
                            .await;
                        let _ = receipts.close().await;
                    }
                } else {
                    self.pool.park(child);
                }
            }
        }
        Ok(removed)
    }

    async fn available(&self, id: &SessionId) -> Result<Session, String> {
        let session = store::chat::load(&self.state, id).await?;
        let mut session = crate::task::blocking(move || session.with_checkout()).await?;
        match session.unavailable.take() {
            Some(reason) => Err(reason),
            None => Ok(session),
        }
    }

    /// Start a durable turn and return as soon as it has an accepted user message. The recorder
    /// notifies `SessionInvalidated(id)` as text, tool outcomes, permissions and Run references arrive.
    pub async fn send(
        &self,
        id: &SessionId,
        text: String,
        selection: Option<ChatTarget>,
    ) -> Result<(), SendError> {
        if text.trim().is_empty() || text.len() > PROMPT_BYTES {
            return Err(SendError::Failed(format!(
                "Chat messages must have text and be at most {PROMPT_BYTES} bytes."
            )));
        }
        let active = self.claim(id)?;
        let chat = self.clone();
        let id = id.clone();
        let (accepted, acknowledgement) = tokio::sync::oneshot::channel();
        // Once queued, the library owns preparation and cleanup independently of the IPC caller.
        tokio::spawn(async move {
            match chat.prepare(&id, &text, selection, &active.cancel).await {
                Ok(prepared) => {
                    let _ = accepted.send(Ok(()));
                    chat.finish(prepared, text, active).await;
                }
                Err(error) => {
                    drop(active);
                    let _ = accepted.send(Err(error));
                }
            }
        });
        acknowledgement
            .await
            .map_err(|_| SendError::Failed("The queued Chat turn did not start.".into()))?
    }

    async fn prepare(
        &self,
        id: &SessionId,
        text: &str,
        selection: Option<ChatTarget>,
        cancel: &CancellationToken,
    ) -> Result<Prepared, SendError> {
        let saved = store::chat::load(&self.state, id).await?;
        let file = path(&self.state, id);
        let owner = store::chat::Owner::current()?;
        let receipts = Receipts::open(&self.state, &saved.worktree).await?;
        let prepared = async {
            // Claim and read the checkout in one transaction. If a move won the race, this
            // turn starts on the newly saved workspace, never the stale path read above.
            let session = receipts.claim_chat(&self.state, id, &owner).await?;
            if self.record_owner(id, owner.token.clone(), session.worktree.clone()) {
                receipts
                    .stop_chat_by_token(id, &owner.token)
                    .await
                    .map_err(SendError::Failed)?;
            }
            let prepared = async {
                let mut session = crate::task::blocking(move || session.with_checkout()).await?;
                if let Some(reason) = session.unavailable.take() {
                    return Err(SendError::Failed(reason));
                }
                let previous_target = session.target.clone();
                let context = self.context(&session.worktree)?;
                let explicit_selection = if let Some(selection) = selection {
                    Some(tokio::select! {
                        _ = cancel.cancelled() => return Err(SendError::Failed("Chat stopped before accepting the turn.".into())),
                        resolved = self.resolve_target(context.repo(), selection, previous_target.as_ref()) => resolved?,
                    })
                } else {
                    None
                };
                let effective_target = explicit_selection
                    .clone()
                    .or(previous_target.clone())
                    .ok_or(SendError::NoModel)?;

                // Validate target and enforce Workspace Trust BEFORE recording Kind::Send into JSONL.
                let conversation = Conversation::load(&file)?;
                let send = conversation.as_ref().map(Conversation::sends).unwrap_or(0) + 1;
                let history = conversation
                    .as_ref()
                    .map(Conversation::history)
                    .transpose()?
                    .unwrap_or_default();

                let engine = match &effective_target {
                    ChatTarget::LegacyClaudeAgent { .. } => {
                        return Err(SendError::LegacyRetired(
                            "Legacy claude-agent Chat execution has been retired. Switch this session to Claude ACP or a direct API provider to continue."
                                .into(),
                        ));
                    }
                    ChatTarget::Direct {
                        backend,
                        model,
                        effort,
                    } => {
                        let engine_label = effective_target.engine_label();
                        let previous_label =
                            handoff_from(conversation.as_ref(), &engine_label, || {
                                session
                                    .previous_engine
                                    .as_ref()
                                    .filter(|prev| *prev != &engine_label)
                                    .cloned()
                                    .or_else(|| {
                                        previous_target
                                            .as_ref()
                                            .map(ChatTarget::engine_label)
                                            .filter(|prev| prev != &engine_label)
                                    })
                                    .or_else(|| {
                                        session
                                            .acp_binding
                                            .as_ref()
                                            .map(|b| format!("ACP ({})", b.agent_id))
                                    })
                            });
                        let prior_turns_count =
                            conversation.as_ref().map(Conversation::sends).unwrap_or(0);
                        ChatEngine::Direct {
                            selection: DirectSelection {
                                backend: backend.clone(),
                                model: model.clone(),
                                effort: effort.clone(),
                            },
                            engine_label,
                            previous_engine_label: previous_label,
                            prior_turns_count,
                        }
                    }
                    ChatTarget::Acp { agent, config } => {
                        let state = self.state.clone();
                        let worktree = session.worktree.clone();
                        let identity = session.repository.clone().unwrap_or_default();
                        let agent_id = agent.clone();
                        let trust_result = crate::task::blocking(move || {
                            acp::engine::verify_launch(&state, &worktree, &identity, &agent_id)
                        })
                        .await
                        .map_err(SendError::Failed)?
                        .map_err(SendError::Failed)?;
                        let launch = match trust_result {
                            Ok(launch) => launch,
                            Err(requirement) => {
                                return Err(SendError::TrustRequired(Box::new(requirement)));
                            }
                        };
                        let prior_turns = transcript::turns(conversation.as_ref(), false, &[]);
                        let last_outcome = conversation.as_ref().and_then(transcript::last_outcome);
                        let next_engine_label = effective_target.engine_label();
                        let previous_engine_label = handoff_from(
                            conversation.as_ref(),
                            &next_engine_label,
                            || {
                                session
                                    .previous_engine
                                    .as_ref()
                                    .filter(|prev| *prev != &next_engine_label)
                                    .cloned()
                                    .or_else(|| {
                                        previous_engine_label(
                                            previous_target.as_ref(),
                                            session.acp_binding.as_ref(),
                                            session.legacy_migration_notice,
                                            agent,
                                        )
                                    })
                            },
                        );
                        ChatEngine::Acp {
                            config: config.clone(),
                            launch: Box::new(launch),
                            previous_engine_label,
                            prior_turns,
                            last_outcome,
                        }
                    }
                };

                if cancel.is_cancelled() {
                    return Err(SendError::Failed(
                        "Chat stopped before accepting the turn.".into(),
                    ));
                }

                if send == 1 {
                    session.title = text
                        .lines()
                        .find_map(|line| {
                            let line = line.trim();
                            (!line.is_empty()).then(|| line.chars().take(TITLE_CHARS).collect())
                        })
                        .expect("nonblank prompt contains a title");
                }
                let mut recorder = Recorder::chat(&self.state, &file, id, send)?;
                recorder.event(Kind::Send(Send {
                    text: Some(text.to_owned()),
                    engine: Some(effective_target.engine_label()),
                    ..Send::default()
                }));
                let prompt = rig_core::message::Message::user(text.to_owned());
                recorder.message(1, &prompt, false);
                recorder.check_saved()?;
                session.updated_at = crate::broker::now();
                session.activity = Some(Activity {
                    outcome: Outcome::Running,
                    unseen: false,
                });
                let (_target, acp_binding) = receipts
                    .accept_chat(&session, Some(effective_target))
                    .await?;
                Ok::<_, SendError>((session, recorder, send, history, engine, acp_binding, context))
            }
            .await;
            if prepared.is_err() {
                let _ = receipts.finish_chat(id, &owner, None, None).await;
            }
            prepared
        }
        .await;
        let (session, recorder, send, history, engine, acp_binding, context) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                let _ = receipts.close().await;
                return Err(error);
            }
        };
        Ok(Prepared {
            session,
            send,
            engine,
            acp_binding,
            context,
            receipts,
            owner,
            recorder,
            history,
        })
    }

    async fn finish(&self, prepared: Prepared, text: String, active: Active) {
        let Prepared {
            mut session,
            send,
            engine,
            acp_binding,
            context,
            receipts,
            owner,
            mut recorder,
            history,
        } = prepared;
        let context = context.with_origin(runs::Origin::Chat {
            session_id: session.id.clone(),
            title: session.title.clone(),
            send: Some(send),
        });
        let directory = self.state.join("chat-output");
        let output = crate::workspace::prepare_directory(&directory, &session.worktree)
            .map_err(|error| agent::error::Failure::from(error.to_string()))
            .and_then(|directory| {
                crate::platform::private_tempdir_in("turn-", &directory)
                    .map_err(|error| agent::error::Failure::from(error.to_string()))
            });
        let mut settled_acp: Option<(u64, Option<usize>, Option<WarmOwner>)> = None;
        let mut park_reaper = false;
        let result = match output {
            Ok(output) => {
                let tools = tools::Tools::new(&context, output.path())
                    .guard_checkout(session.repository.as_ref());
                match engine {
                    ChatEngine::Direct {
                        selection,
                        engine_label,
                        previous_engine_label,
                        prior_turns_count,
                    } => {
                        if let Some(prev_label) = previous_engine_label
                            && prior_turns_count > 0
                        {
                            recorder.event(Kind::Handoff(crate::agent::session::HandoffEvent {
                                from: prev_label,
                                to: engine_label.clone(),
                                summary: format!(
                                    "Continued conversation in {engine_label} carrying {prior_turns_count} prior turn(s)."
                                ),
                            }));
                        }
                        let turn = agent::chat_turn(
                            &selection,
                            &context,
                            &tools,
                            agent::ChatPrompt::new(
                                history,
                                text,
                                session.moved_from.as_deref(),
                                &session.worktree,
                            ),
                            &mut recorder,
                            &active.cancel,
                        );
                        let requested = async {
                            let mut check = tokio::time::interval(STOP_CHECK);
                            check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                            loop {
                                check.tick().await;
                                if receipts.chat_stop_requested(&session.id, &owner).await
                                    == Ok(true)
                                {
                                    active.cancel.cancel();
                                    break;
                                }
                            }
                        };
                        tokio::pin!(turn);
                        tokio::select! {
                            answer = &mut turn => answer,
                            () = requested => turn.await,
                        }
                    }
                    ChatEngine::Acp {
                        config,
                        launch,
                        previous_engine_label,
                        prior_turns,
                        last_outcome,
                    } => {
                        let identity = session.repository.clone().unwrap_or_default();
                        let outcome = acp::engine::run_turn(
                            AcpTurnInput {
                                session_id: &session.id,
                                worktree: &session.worktree,
                                identity: &identity,
                                moved_from: session.moved_from.as_deref(),
                                send,
                                prior_turns: &prior_turns,
                                last_outcome,
                                previous_engine_label: previous_engine_label.as_deref(),
                                user_text: &text,
                                requested_config: &config,
                                binding: acp_binding,
                                launch: *launch,
                                context: &context,
                                tools: &tools,
                                receipts: &receipts,
                                owner: &owner,
                                pool: &self.pool,
                                permission_notify: &self.permission_notify,
                                turn_cancel: &active.cancel,
                            },
                            &mut recorder,
                        )
                        .await;
                        // Only a quiescent session counts as settled for `session/load`.
                        let completed_send =
                            (outcome.result.is_ok() && outcome.resumable).then_some(send);
                        if outcome.warm_owner.is_some() {
                            park_reaper = true;
                        }
                        settled_acp =
                            Some((outcome.generation, completed_send, outcome.warm_owner));
                        outcome.result
                    }
                }
            }
            Err(error) => Err(error),
        };
        let answer = match result {
            Ok(text) => Answer::Completed(text),
            Err(failure) => Answer::Failed {
                code: Some(failure.code),
                message: Some(failure.message),
            },
        };
        session.activity = Some(Activity {
            outcome: transcript::outcome(&answer),
            unseen: true,
        });
        session.updated_at = crate::broker::now().max(session.updated_at);
        recorder.event(Kind::Answer(answer));
        let flushed = recorder.check_saved().is_ok();
        let settled_acp = if flushed {
            settled_acp
        } else {
            settled_acp.map(|(generation, _, warm_owner)| (generation, None, warm_owner))
        };
        // Publish the settled outcome, release ownership, and advance ACP settled_send together.
        let _ = receipts
            .finish_chat(&session.id, &owner, Some(&session), settled_acp)
            .await;
        let _ = receipts.close().await;
        if park_reaper {
            self.schedule_idle_reaper();
        }
        drop(active);
        recorder.notify();
    }

    fn schedule_idle_reaper(&self) {
        let pool = self.pool.clone();
        let state = self.state.clone();
        tokio::spawn(async move {
            tokio::time::sleep(IDLE_TTL + IDLE_REAP_MARGIN).await;
            let expired = pool.take_expired();
            let current_owner = WarmOwner::current().ok();
            for child in expired {
                let session_id = child.session_id.clone();
                let generation = child.generation;
                let _ = child.terminate().await;
                if let Some(warm_owner) = current_owner
                    && let Ok(session) = store::chat::load(&state, &session_id).await
                    && let Ok(receipts) = Receipts::open(&state, &session.worktree).await
                {
                    let _ = receipts
                        .clear_acp_warm_owner(&session_id, generation, warm_owner)
                        .await;
                    let _ = receipts.close().await;
                }
            }
        });
    }

    /// Stop the active turn, even if another app owns it. Accepted Verify Runs are independent.
    /// Always commits the SQLite Stop transaction (`stop_requested = true` + permission
    /// cancellation) BEFORE firing the in-memory CancellationToken.
    pub async fn stop(&self, id: &SessionId) -> Result<bool, String> {
        // Under the lock that publishes ownership: a turn not yet owned in SQLite is marked, and
        // commits this Stop itself as it takes ownership.
        let local = self
            .active
            .state
            .lock()
            .expect("Chat active turns lock")
            .turns
            .get_mut(id)
            .map(|entry| {
                entry.stopped |= entry.owner_token.is_none();
                entry.clone()
            });
        if let Some(entry) = local {
            if let (Some(token), Some(worktree)) = (&entry.owner_token, &entry.worktree) {
                let receipts = Receipts::open(&self.state, worktree).await?;
                let stopped = receipts.stop_chat_by_token(id, token).await;
                let closed = receipts.close().await;
                let _ = stopped.and_then(|value| closed.map(|()| value))?;
            }
            self.permission_notify.notify_waiters();
            entry.cancel.cancel();
            crate::changes::Publisher::new(&self.state)
                .notify(crate::changes::Change::SessionInvalidated(id.clone()));
            return Ok(true);
        }
        let Some(owner) = store::chat::turn_owner(&self.state, id).await? else {
            return Ok(false);
        };
        let session = store::chat::load(&self.state, id).await?;
        let stopped = self.stop_exact(&session, owner).await?;
        if stopped {
            crate::changes::Publisher::new(&self.state)
                .notify(crate::changes::Change::SessionInvalidated(id.clone()));
        }
        Ok(stopped)
    }

    /// The desktop's quit prompt names every running Chat. Deliver the same Stop to those
    /// turns even when another process owns them, before the desktop window closes.
    pub async fn stop_running(&self) -> Result<(), StopRunningError> {
        // Under the lock that publishes ownership: a turn not yet owned in SQLite is marked, and
        // commits this Stop itself as it takes ownership (`record_owner`).
        let local: Vec<(SessionId, ActiveEntry)> = {
            let mut active = self.active.state.lock().expect("Chat active turns lock");
            active
                .turns
                .iter_mut()
                .map(|(id, entry)| {
                    entry.stopped |= entry.owner_token.is_none();
                    (id.clone(), entry.clone())
                })
                .collect()
        };
        let mut failed = Vec::new();
        // Turns stopped here through their owner token; the sweep below skips only these. A
        // turn snapshotted before it published its token may claim SQLite ownership meanwhile,
        // so the sweep still stops it there, setting `stop_requested` and its permissions.
        let mut stopped_by_token = std::collections::BTreeSet::new();
        for (id, entry) in local {
            if let (Some(token), Some(worktree)) = (&entry.owner_token, &entry.worktree) {
                match Receipts::open(&self.state, worktree).await {
                    Ok(receipts) => {
                        let stopped = receipts.stop_chat_by_token(&id, token).await;
                        let closed = receipts.close().await;
                        match stopped.and(closed) {
                            Ok(()) => {
                                entry.cancel.cancel();
                                stopped_by_token.insert(id.clone());
                            }
                            Err(error) => failed.push(format!("{id}: {error}")),
                        }
                    }
                    Err(error) => failed.push(format!("{id}: {error}")),
                }
            } else {
                entry.cancel.cancel();
            }
        }
        self.permission_notify.notify_waiters();
        let turns = store::chat::active_turns(&self.state, true)
            .await
            .map_err(StopRunningError::Unreadable)?;
        for (session, owner) in turns {
            if owner.in_current_process() && stopped_by_token.contains(&session.id) {
                continue;
            }
            if let Err(error) = self.stop_exact(&session, owner).await {
                failed.push(format!("{}: {error}", session.id));
            }
        }
        if failed.is_empty() {
            Ok(())
        } else {
            Err(StopRunningError::Turns(failed))
        }
    }

    async fn stop_exact(
        &self,
        session: &Session,
        owner: store::chat::Owner,
    ) -> Result<bool, String> {
        let receipts = Receipts::open(&self.state, &session.worktree).await?;
        let stopped = receipts.stop_chat(&session.id, owner).await;
        let closed = receipts.close().await;
        stopped.and_then(|stopped| closed.map(|()| stopped))
    }

    async fn resolve_target(
        &self,
        worktree: &Path,
        selection: ChatTarget,
        previous: Option<&ChatTarget>,
    ) -> Result<ChatTarget, String> {
        match selection {
            ChatTarget::LegacyClaudeAgent { .. } => Err(
                "Legacy claude-agent Chat execution has been retired. Choose an ACP Chat agent or a direct API provider."
                    .into(),
            ),
            ChatTarget::Direct {
                backend,
                model,
                effort,
            } => {
                let prev = match previous {
                    Some(ChatTarget::Direct {
                        backend,
                        model,
                        effort,
                    }) => Some(DirectSelection {
                        backend: backend.clone(),
                        model: model.clone(),
                        effort: effort.clone(),
                    }),
                    _ => None,
                };
                let resolved = self
                    .resolve_direct(
                        worktree,
                        DirectSelection {
                            backend,
                            model,
                            effort,
                        },
                        prev.as_ref(),
                    )
                    .await?;
                Ok(ChatTarget::direct(resolved))
            }
            ChatTarget::Acp { agent, config } => {
                let _spec = catalog::resolve_spec(&agent, Some(&self.state), Some(worktree))?;
                Ok(ChatTarget::Acp { agent, config })
            }
        }
    }

    async fn resolve_direct(
        &self,
        worktree: &Path,
        mut selection: DirectSelection,
        previous: Option<&DirectSelection>,
    ) -> Result<DirectSelection, String> {
        let backend = selection.backend.to_backend();
        let route = llm::route(
            &backend,
            Some(&self.state),
            Some(worktree),
            "for this Chat model",
        )
        .map_err(|failure| failure.message)?;
        if !matches!(route, llm::Route::Http(_)) {
            return Err(format!(
                "{} is not a direct HTTP provider for Chat.",
                selection.backend
            ));
        }
        let listing = llm::models::list(backend, Some(&self.state), Some(worktree)).await?;
        let model = listing
            .models
            .into_iter()
            .find(|model| model.slug == selection.model)
            .ok_or_else(|| {
                format!(
                    "{} does not list model {}.",
                    selection.backend, selection.model
                )
            })?;
        if let Some(effort) = &selection.effort {
            if !model.efforts.contains(effort) {
                let allowed = if model.efforts.is_empty() {
                    "none".to_owned()
                } else {
                    model.efforts.join(", ")
                };
                return Err(format!(
                    "{} model {} does not offer effort {effort:?}. Available efforts: {allowed}.",
                    selection.backend, selection.model
                ));
            }
        } else {
            selection.effort = previous
                .and_then(|previous| previous.effort.as_ref())
                .filter(|effort| model.efforts.contains(effort))
                .cloned()
                .or(model.default_effort);
        }
        Ok(selection)
    }

    fn claim(&self, id: &SessionId) -> Result<Active, String> {
        let mut active = self.active.state.lock().expect("Chat active turns lock");
        if active.closed {
            return Err("Chat is closing.".into());
        }
        match active.turns.entry(id.clone()) {
            Slot::Occupied(_) => Err("This Chat session already has an active turn.".into()),
            Slot::Vacant(entry) => {
                let cancel = CancellationToken::new();
                entry.insert(ActiveEntry {
                    cancel: cancel.clone(),
                    owner_token: None,
                    worktree: None,
                    stopped: false,
                });
                Ok(Active {
                    chat: self.clone(),
                    id: id.clone(),
                    cancel,
                })
            }
        }
    }

    /// Publish the turn's SQLite ownership; true when a Stop came before it, which the turn then
    /// commits durably itself.
    fn record_owner(&self, id: &SessionId, token: String, worktree: PathBuf) -> bool {
        self.active
            .state
            .lock()
            .expect("Chat active turns lock")
            .turns
            .get_mut(id)
            .is_some_and(|entry| {
                entry.owner_token = Some(token);
                entry.worktree = Some(worktree);
                entry.stopped
            })
    }
}

fn default_config_selections(options: &[ConfigOption]) -> Vec<ConfigSelection> {
    options
        .iter()
        .filter(|opt| !opt.current_value.is_empty())
        .map(|opt| ConfigSelection {
            id: opt.id.clone(),
            value: opt.current_value.clone(),
            category: Some(opt.category),
        })
        .collect()
}

/// The engine a turn on `next` hands off from: the one the conversation's last turn ran on, as
/// its send recorded it, when that differs. The picker saves its choice before the next send,
/// so the saved target names the new engine, not the last one. A conversation whose sends
/// predate that record falls back to `unrecorded`.
fn handoff_from(
    conversation: Option<&Conversation>,
    next: &str,
    unrecorded: impl FnOnce() -> Option<String>,
) -> Option<String> {
    let last = conversation.and_then(|conversation| {
        conversation
            .events()
            .iter()
            .rev()
            .find_map(|event| match &event.kind {
                Kind::Send(send) => Some(send.engine.clone()),
                _ => None,
            })
    });
    match last {
        Some(Some(last)) => (last != next).then_some(last),
        Some(None) | None => unrecorded(),
    }
}

fn previous_engine_label(
    previous_target: Option<&ChatTarget>,
    binding: Option<&AcpBinding>,
    legacy_migration_notice: bool,
    next_agent: &AgentId,
) -> Option<String> {
    if let Some(prev) = previous_target {
        match prev {
            ChatTarget::Acp { agent, .. } if agent == next_agent => {
                if legacy_migration_notice && binding.is_none() {
                    return Some("Legacy Antigravity".into());
                }
            }
            other => return Some(other.engine_label()),
        }
    }
    if let Some(binding) = binding
        && &binding.agent_id != next_agent
    {
        return Some(format!("ACP ({})", binding.agent_id));
    }
    None
}

struct Prepared {
    session: Session,
    send: usize,
    engine: ChatEngine,
    acp_binding: Option<AcpBinding>,
    context: runs::Context,
    receipts: Receipts,
    owner: store::chat::Owner,
    recorder: Recorder,
    history: Vec<rig_core::message::Message>,
}

struct Active {
    chat: Chat,
    id: SessionId,
    cancel: CancellationToken,
}

impl Drop for Active {
    fn drop(&mut self) {
        self.chat
            .active
            .state
            .lock()
            .expect("Chat active turns lock")
            .turns
            .remove(&self.id);
        self.chat.active.settled.notify_waiters();
    }
}

fn path(state: &Path, id: &SessionId) -> PathBuf {
    state.join("chat-sessions").join(format!("{id}.jsonl"))
}
