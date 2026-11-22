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
    monitor::live::Place,
    repository, runs,
    store::{self, Receipts},
    types::{SessionId, Timestamp},
};

pub use details::ToolKind;
pub use transcript::{Block, RunCard, Turn, TurnState};

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
    pub selection: Model,
    #[serde(default)]
    pub view: SessionView,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    #[serde(default)]
    activity: Option<Activity>,
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

    pub(crate) fn refresh_activity(&mut self, state: &Path) {
        let owner_gone = self.outcome() == Some(Outcome::Running) && !self.running;
        if !owner_gone && self.activity.is_some() {
            return;
        }
        // Do not present legacy history as new. If an owner died after flushing its
        // answer and before updating the catalog, prefer that answer over Interrupted.
        let outcome = Conversation::load(&path(state, &self.id))
            .ok()
            .flatten()
            .as_ref()
            .and_then(transcript::last_outcome);
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

    /// The starting model for a new session: the worktree's last choice, or the first
    /// listed model in Settings order after every provider has answered.
    pub async fn default_model(&self, worktree: &Path) -> Result<Option<Model>, String> {
        let worktree =
            crate::workspace::canonical_target(worktree).map_err(|error| error.to_string())?;
        let identity = crate::task::blocking({
            let worktree = worktree.clone();
            move || repository::identify(&worktree)
        })
        .await?;
        self.starting_model(&worktree, &identity).await
    }

    async fn starting_model(
        &self,
        worktree: &Path,
        identity: &repository::Identity,
    ) -> Result<Option<Model>, String> {
        if let Some(selection) = self
            .latest_for_branch(&Place::for_checkout(worktree, identity))
            .await?
            .map(|session| session.selection)
        {
            return Ok(Some(selection));
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

    /// Persist a session even before it has a message. An explicit selection must be listed.
    pub async fn create(
        &self,
        worktree: &Path,
        selection: Option<Model>,
    ) -> Result<Session, String> {
        let workspace =
            crate::workspace::canonical_target(worktree).map_err(|error| error.to_string())?;
        let worktree = workspace.as_path();
        crate::config::read_workspace_config(worktree).map_err(|error| error.to_string())?;
        let repository = crate::task::blocking({
            let worktree = worktree.to_owned();
            move || repository::identify(&worktree)
        })
        .await?;
        let selection = match selection {
            Some(selection) => {
                let previous = if selection.effort.is_none() {
                    self.latest_for_branch(&Place::for_checkout(worktree, &repository))
                        .await?
                        .map(|session| session.selection)
                } else {
                    None
                };
                self.resolve_model(worktree, selection, previous.as_ref())
                    .await?
            }
            None => self
                .starting_model(worktree, &repository)
                .await?
                .ok_or("No provider lists a model. Connect a provider first.")?,
        };
        let now = crate::broker::now();
        let session = Session {
            id: format!("chat-{}", agent::uuid()?).parse()?,
            title: "New chat".into(),
            worktree: worktree.into(),
            repository: Some(repository),
            selection,
            view: SessionView::default(),
            created_at: now,
            updated_at: now,
            activity: None,
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

    /// The most recently active saved session on this branch, even if its checkout is gone.
    pub async fn latest_for_branch(&self, branch: &Place) -> Result<Option<Session>, String> {
        let mut sessions = store::chat::for_branch(&self.state, branch)
            .await?
            .into_iter();
        crate::task::blocking(move || sessions.next().map(Session::with_checkout)).await
    }

    /// The branch's Current can use any saved session whose exact checkout is still usable.
    pub async fn current_worktree(&self, branch: &Place) -> Result<Option<PathBuf>, String> {
        let sessions = store::chat::for_branch(&self.state, branch).await?;
        crate::task::blocking(move || {
            sessions
                .into_iter()
                .find(|session| session.check_checkout().is_ok())
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

    /// The model's streamed text, tools, and current state of each referenced Run.
    pub async fn open(&self, id: &SessionId) -> Result<View, String> {
        let session = store::chat::load(&self.state, id).await?;
        let session = crate::task::blocking(move || session.with_checkout()).await?;
        let running = session.running;
        let conversation = Conversation::load(&path(&self.state, id))?;
        let mut turns = transcript::turns(conversation.as_ref(), running);
        if let Ok(context) = self.context(&session.worktree) {
            transcript::update_runs(&context, &mut turns).await;
        }
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
            .resolve_model(&session.worktree, selection, Some(&session.selection))
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
    ) -> Result<(), String> {
        if text.trim().is_empty() || text.len() > PROMPT_BYTES {
            return Err(format!(
                "Chat messages must have text and be at most {PROMPT_BYTES} bytes."
            ));
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
            .map_err(|_| "The queued Chat turn did not start.")?
    }

    async fn prepare(
        &self,
        id: &SessionId,
        text: &str,
        selection: Option<Model>,
        cancel: &CancellationToken,
    ) -> Result<Prepared, String> {
        let mut session = self.available(id).await?;
        let context = self.context(&session.worktree)?;
        let selection = if let Some(selection) = selection {
            Some(tokio::select! {
                _ = cancel.cancelled() => return Err("Chat stopped before accepting the turn.".into()),
                resolved = self.resolve_model(context.repo(), selection, Some(&session.selection)) => resolved?,
            })
        } else {
            None
        };
        let file = path(&self.state, id);
        let owner = store::chat::Owner::current()?;
        let receipts = Receipts::open(&self.state, context.repo()).await?;
        let prepared = async {
            receipts.claim_chat(id, &owner).await?;
            let prepared = async {
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
                session.selection = receipts.accept_chat(&session, selection).await?;
                Ok::<_, String>((recorder, history))
            }
            .await;
            if prepared.is_err() {
                let _ = receipts.finish_chat(id, &owner, None).await;
            }
            prepared
        }
        .await;
        let (recorder, history) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                let _ = receipts.close().await;
                return Err(error);
            }
        };
        Ok(Prepared {
            session,
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
                    &session.selection,
                    &context,
                    &tools,
                    history,
                    text,
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
    pub async fn stop_running(&self) -> Result<(), String> {
        for (session, owner) in store::chat::active_turns(&self.state, true).await? {
            self.stop_exact(&session, owner).await?;
        }
        Ok(())
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
