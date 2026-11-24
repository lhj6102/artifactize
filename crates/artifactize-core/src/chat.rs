//! Artifactize's worktree-bound coding conversation. Provider selection changes the model,
//! never the tools or the workflow.
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
use tokio_util::sync::CancellationToken;

use crate::{
    agent::{
        self,
        session::{Answer, Conversation, Kind, Recorder, Send},
    },
    auth::providers,
    config::{Backend, ModelId, Provider},
    llm,
    monitor::live::{self, Branch, Place},
    repository, runs,
    store::{self, Receipts},
    types::{SessionId, Timestamp},
};

pub use details::ToolKind;
pub(crate) fn recorded_outcome(state: &Path, id: &SessionId) -> Option<Outcome> {
    Conversation::load(&path(state, id))
        .ok()
        .flatten()
        .as_ref()
        .and_then(transcript::last_outcome)
}

pub use transcript::{Block, RunCard, ToolResult, Turn, TurnState};

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub backend: Backend,
    pub model: ModelId,
    #[serde(default)]
    pub effort: Option<String>,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum SendError {
    #[error("Choose a model before sending a Chat message.")]
    NoModel,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: SessionId,
    pub title: String,
    #[serde(with = "crate::platform::path_serde")]
    pub worktree: PathBuf,
    #[serde(default)]
    pub repository: Option<repository::Identity>,
    #[serde(default, alias = "selection")]
    pub model: Option<Model>,
    #[serde(default)]
    pub view: SessionView,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    #[serde(default)]
    activity: Option<Activity>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::platform::path_serde::option"
    )]
    pub(crate) moved_from: Option<PathBuf>,
    /// Live ownership of a turn, read separately from the saved session.
    #[serde(skip)]
    pub running: bool,
    #[serde(skip)]
    pub unavailable: Option<String>,
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
    pub turns: Vec<Turn>,
}

#[derive(Clone)]
pub struct Chat {
    state: PathBuf,
    executable: Option<PathBuf>,
    active: Arc<ActiveTurns>,
}

#[derive(Default)]
struct ActiveTurns {
    state: Mutex<Turns>,
    settled: tokio::sync::Notify,
}

#[derive(Default)]
struct Turns {
    closed: bool,
    turns: BTreeMap<SessionId, CancellationToken>,
}

impl Chat {
    pub fn new(state: PathBuf, executable: PathBuf) -> Self {
        Self {
            state,
            executable: Some(executable),
            active: Arc::default(),
        }
    }

    /// Desktop discovers the packaged CLI when a session needs it, not at app startup.
    pub fn discover(state: PathBuf) -> Self {
        Self {
            state,
            executable: None,
            active: Arc::default(),
        }
    }

    fn context(&self, worktree: &Path) -> Result<runs::Context, String> {
        match &self.executable {
            Some(executable) => runs::Context::new(worktree, Some(&self.state), executable),
            None => runs::Context::discover(worktree, Some(&self.state)),
        }
    }

    /// Stop accepting work, cancel owned Chat turns and wait for their saved outcomes and
    /// ownership release. Verify Runs already admitted remain independent of Chat.
    pub fn close(&self) -> impl std::future::Future<Output = ()> + use<> {
        {
            let mut active = self.active.state.lock().expect("Chat active turns lock");
            active.closed = true;
            for cancel in active.turns.values() {
                cancel.cancel();
            }
        }
        let active = self.active.clone();
        async move {
            loop {
                let settled = active.settled.notified();
                tokio::pin!(settled);
                settled.as_mut().enable();
                if active
                    .state
                    .lock()
                    .expect("Chat active turns lock")
                    .turns
                    .is_empty()
                {
                    return;
                }
                settled.await;
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
                .finish_chat(&session.id, &owner, Some(&session))
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

    /// The starting model for a new session: the worktree's last available choice, or the first
    /// listed model in Settings order after every provider has answered.
    pub async fn default_model(&self, worktree: &Path) -> Result<Option<Model>, String> {
        self.default_model_for(worktree, None).await
    }

    pub async fn default_model_for(
        &self,
        worktree: &Path,
        branch: Option<&Place>,
    ) -> Result<Option<Model>, String> {
        let (worktree, identity) = checkout_for(worktree, branch).await?;
        self.starting_model(&worktree, &identity).await
    }

    async fn starting_model(
        &self,
        worktree: &Path,
        identity: &repository::Identity,
    ) -> Result<Option<Model>, String> {
        if let Some(model) = self.last_model(worktree, identity).await?
            && let Ok(available) = self.resolve_model(worktree, model, None).await
        {
            return Ok(Some(available));
        }
        let providers = providers::list(Some(&self.state), Some(worktree))?;
        let backends = Provider::ALL
            .into_iter()
            .map(Backend::from)
            .chain(providers.custom.into_keys());
        let listings = futures_util::future::join_all(
            backends.map(|backend| llm::models::list(backend, Some(&self.state), Some(worktree))),
        )
        .await;
        Ok(listings
            .into_iter()
            .filter_map(Result::ok)
            .find_map(|listing| {
                listing.models.into_iter().next().map(|model| Model {
                    backend: listing.backend,
                    model: model.slug,
                    effort: model.default_effort,
                })
            }))
    }

    async fn last_model(
        &self,
        workspace: &Path,
        checkout: &repository::Identity,
    ) -> Result<Option<Model>, String> {
        let branch = Place::for_checkout(workspace, checkout);
        Ok(store::chat::for_branch(&self.state, &branch)
            .await?
            .into_iter()
            .find_map(|session| session.model))
    }

    /// Persist an empty session, with no model unless one is explicitly selected.
    pub async fn create(
        &self,
        worktree: &Path,
        selection: Option<Model>,
    ) -> Result<Session, String> {
        self.create_for_branch(worktree, None, selection).await
    }

    /// Create only on the checkout the person selected, not a branch switched in meanwhile.
    pub async fn create_for_branch(
        &self,
        workspace: &Path,
        branch: Option<&Place>,
        selection: Option<Model>,
    ) -> Result<Session, String> {
        let (workspace, repository) = checkout_for(workspace, branch).await?;
        let worktree = workspace.as_path();
        crate::config::read_workspace_config(worktree).map_err(|error| error.to_string())?;
        let model = if let Some(selection) = selection {
            let previous = if selection.effort.is_none() {
                self.last_model(worktree, &repository).await?
            } else {
                None
            };
            Some(
                self.resolve_model(worktree, selection, previous.as_ref())
                    .await?,
            )
        } else {
            None
        };
        let now = crate::broker::now();
        let session = Session {
            id: format!("chat-{}", agent::uuid()?).parse()?,
            title: "New chat".into(),
            worktree: worktree.into(),
            repository: Some(repository),
            model,
            view: SessionView::default(),
            created_at: now,
            updated_at: now,
            activity: None,
            moved_from: None,
            running: false,
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

    /// The model's streamed text, tools, and current state of each referenced Run.
    pub async fn open(&self, id: &SessionId) -> Result<View, String> {
        let session = store::chat::load(&self.state, id).await?;
        let session = crate::task::blocking(move || session.with_checkout()).await?;
        let running = session.running;
        let conversation = Conversation::load(&path(&self.state, id))?;
        let mut turns = transcript::turns(conversation.as_ref(), running);
        transcript::update_runs(&self.state, id, &mut turns).await;
        Ok(View {
            session,
            running,
            turns,
        })
    }

    /// Save a selection for the next turn without adding a message. An accepted turn keeps
    /// the selection it started with, even if the user changes the model while it streams.
    pub async fn set_model(&self, id: &SessionId, selection: Model) -> Result<Session, String> {
        let session = self.available(id).await?;
        let selection = self
            .resolve_model(&session.worktree, selection, session.model.as_ref())
            .await?;
        let receipts = Receipts::open(&self.state, &session.worktree).await?;
        let saved = receipts.set_chat_model(&self.state, id, selection).await;
        let closed = receipts.close().await;
        let saved = saved.and_then(|session| closed.map(|()| session))?;
        crate::changes::Publisher::new(&self.state)
            .notify(crate::changes::Change::SessionInvalidated(id.clone()));
        Ok(saved)
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
    /// notifies `SessionInvalidated(id)` as text, tool outcomes and Run references arrive.
    pub async fn send(
        &self,
        id: &SessionId,
        text: String,
        selection: Option<Model>,
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
        selection: Option<Model>,
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
            let prepared = async {
                let mut session = crate::task::blocking(move || session.with_checkout()).await?;
                if let Some(reason) = session.unavailable.take() {
                    return Err(SendError::Failed(reason));
                }
                let current = session.model.as_ref().ok_or(SendError::NoModel)?;
                let context = self.context(&session.worktree)?;
                let selection = if let Some(selection) = selection {
                    Some(tokio::select! {
                        _ = cancel.cancelled() => return Err(SendError::Failed("Chat stopped before accepting the turn.".into())),
                        resolved = self.resolve_model(context.repo(), selection, Some(current)) => resolved?,
                    })
                } else {
                    None
                };
                let conversation = Conversation::load(&file)?;
                let send = conversation.as_ref().map(Conversation::sends).unwrap_or(0) + 1;
                let history = conversation
                    .as_ref()
                    .map(Conversation::history)
                    .transpose()?
                    .unwrap_or_default();
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
                    framing: None,
                    files_changed: None,
                    tools: None,
                }));
                let prompt = rig_core::message::Message::user(text.to_owned());
                recorder.message(1, &prompt, false);
                recorder.check_saved()?;
                session.updated_at = crate::broker::now();
                session.activity = Some(Activity {
                    outcome: Outcome::Running,
                    unseen: false,
                });
                let model = receipts.accept_chat(&session, selection).await?;
                Ok::<_, SendError>((session, recorder, history, model, context))
            }
            .await;
            if prepared.is_err() {
                let _ = receipts.finish_chat(id, &owner, None).await;
            }
            prepared
        }
        .await;
        let (session, recorder, history, model, context) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                let _ = receipts.close().await;
                return Err(error);
            }
        };
        Ok(Prepared {
            session,
            model,
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
            model,
            context,
            receipts,
            owner,
            mut recorder,
            history,
        } = prepared;
        let context = context.with_origin(runs::Origin::Chat {
            session_id: session.id.clone(),
            title: session.title.clone(),
        });
        let directory = self.state.join("chat-output");
        let output = crate::workspace::prepare_directory(&directory, &session.worktree)
            .map_err(|error| agent::error::Failure::from(error.to_string()))
            .and_then(|directory| {
                crate::platform::private_tempdir_in("turn-", &directory)
                    .map_err(|error| agent::error::Failure::from(error.to_string()))
            });
        let result = match output {
            Ok(output) => {
                let tools = tools::Tools::new(&context, output.path())
                    .guard_checkout(session.repository.as_ref());
                let turn = agent::chat_turn(
                    &model,
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
                        if receipts.chat_stop_requested(&session.id, &owner).await == Ok(true) {
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
        // Publish the settled outcome and release ownership together.
        let _ = receipts
            .finish_chat(&session.id, &owner, Some(&session))
            .await;
        let _ = receipts.close().await;
        drop(active);
        recorder.notify();
    }

    /// Stop the active turn, even if another app owns it. Accepted Verify Runs are independent.
    pub async fn stop(&self, id: &SessionId) -> Result<bool, String> {
        let local = self
            .active
            .state
            .lock()
            .expect("Chat active turns lock")
            .turns
            .get(id)
            .cloned();
        if let Some(cancel) = local {
            cancel.cancel();
            return Ok(true);
        }
        let Some(owner) = store::chat::turn_owner(&self.state, id).await? else {
            return Ok(false);
        };
        let session = store::chat::load(&self.state, id).await?;
        self.stop_exact(&session, owner).await
    }

    /// The desktop's quit prompt names every running Chat. Deliver the same Stop to those
    /// turns even when another process owns them, before the desktop window closes.
    pub async fn stop_running(&self) -> Result<(), StopRunningError> {
        let local: std::collections::BTreeSet<_> = {
            let active = self.active.state.lock().expect("Chat active turns lock");
            active
                .turns
                .iter()
                .map(|(id, token)| {
                    token.cancel();
                    id.clone()
                })
                .collect()
        };
        let mut failed = Vec::new();
        let turns = store::chat::active_turns(&self.state, true)
            .await
            .map_err(StopRunningError::Unreadable)?;
        for (session, owner) in turns {
            if owner.in_current_process() && local.contains(&session.id) {
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

    async fn resolve_model(
        &self,
        worktree: &Path,
        mut selection: Model,
        previous: Option<&Model>,
    ) -> Result<Model, String> {
        let listing =
            llm::models::list(selection.backend.clone(), Some(&self.state), Some(worktree)).await?;
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
                let cancel = entry.insert(CancellationToken::new()).clone();
                Ok(Active {
                    chat: self.clone(),
                    id: id.clone(),
                    cancel,
                })
            }
        }
    }
}

struct Prepared {
    session: Session,
    model: Model,
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
