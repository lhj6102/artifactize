//! Artifactize's worktree-bound coding conversation. Provider selection changes the model,
//! never the tools or the workflow.
pub(crate) mod tools;
mod transcript;

use std::{
    collections::{BTreeMap, btree_map::Entry as Slot},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
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
    llm, repository, runs,
    store::{self, Receipts},
    types::{SessionId, Timestamp},
};

pub use transcript::{Block, RunCard, Turn, TurnState};

/// A description comes from the first nonempty prompt line, not another provider call.
const TITLE_CHARS: usize = 72;
/// Limit one prompt so a caller cannot overwhelm the transcript and model request at once.
const PROMPT_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub backend: Backend,
    pub model: ModelId,
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
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    /// Live ownership of a turn, read separately from the saved session.
    #[serde(skip)]
    pub running: bool,
    #[serde(skip)]
    pub unavailable: Option<String>,
}

impl Session {
    pub fn label(&self) -> crate::monitor::live::Label {
        crate::monitor::live::Label::for_identity(
            &self.worktree,
            &self.repository.clone().unwrap_or_default(),
        )
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
    active: Arc<Mutex<BTreeMap<SessionId, CancellationToken>>>,
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

    pub fn stop_all(&self) {
        for cancel in self.active.lock().expect("Chat active turns lock").values() {
            cancel.cancel();
        }
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
        if let Some(selection) = store::chat::latest_model(&self.state, worktree, identity).await? {
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
                })
            }))
    }

    /// Persist a session even before it has a message. An explicit selection must be listed.
    pub async fn create(
        &self,
        worktree: &Path,
        selection: Option<Model>,
    ) -> Result<Session, String> {
        let context = self.context(worktree)?;
        let worktree = context.repo();
        crate::config::read_workspace_config(worktree).map_err(|error| error.to_string())?;
        let repository = crate::task::blocking({
            let worktree = worktree.to_owned();
            move || repository::identify(&worktree)
        })
        .await?;
        let selection = match selection {
            Some(selection) => {
                self.check_model(worktree, &selection).await?;
                selection
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
            created_at: now,
            updated_at: now,
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

    pub async fn list(&self, worktree: Option<&Path>) -> Result<Vec<Session>, String> {
        let sessions = store::chat::list(&self.state, worktree).await?;
        crate::task::blocking(move || {
            let mut identities = BTreeMap::new();
            sessions
                .into_iter()
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
            match chat.prepare(&id, &text, selection).await {
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
    ) -> Result<Prepared, String> {
        let mut session = store::chat::load(&self.state, id).await?;
        session = crate::task::blocking(move || session.with_checkout()).await?;
        if let Some(reason) = session.unavailable.take() {
            return Err(reason);
        }
        let context = self.context(&session.worktree)?;
        if let Some(selection) = selection.filter(|selection| *selection != session.selection) {
            self.check_model(context.repo(), &selection).await?;
            session.selection = selection;
        }
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
                receipts.save_chat(&session).await?;
                Ok::<_, String>((recorder, history))
            }
            .await;
            if prepared.is_err() {
                let _ = receipts.finish_chat(id, &owner).await;
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
            session,
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
                agent::chat_turn(
                    &session.selection,
                    &context,
                    &tools,
                    history,
                    text,
                    &mut recorder,
                    &active.cancel,
                )
                .await
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
        recorder.event(Kind::Answer(answer));
        // Clear active ownership before the last invalidation so readers see the settled state.
        let _ = receipts.finish_chat(&session.id, &owner).await;
        let _ = receipts.close().await;
        drop(active);
        recorder.notify();
    }

    /// Cancel a turn owned by this Chat process, without cancelling its already-admitted Runs.
    pub fn stop(&self, id: &SessionId) -> bool {
        let active = self.active.lock().expect("Chat active turns lock");
        active.get(id).is_some_and(|cancel| {
            cancel.cancel();
            true
        })
    }

    async fn check_model(&self, worktree: &Path, selection: &Model) -> Result<(), String> {
        let listing =
            llm::models::list(selection.backend.clone(), Some(&self.state), Some(worktree)).await?;
        if listing
            .models
            .iter()
            .any(|model| model.slug == selection.model)
        {
            Ok(())
        } else {
            Err(format!(
                "{} does not list model {}.",
                selection.backend, selection.model
            ))
        }
    }

    fn claim(&self, id: &SessionId) -> Result<Active, String> {
        let mut entries = self.active.lock().expect("Chat active turns lock");
        match entries.entry(id.clone()) {
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
            .lock()
            .expect("Chat active turns lock")
            .remove(&self.id);
    }
}

fn path(state: &Path, id: &SessionId) -> PathBuf {
    state.join("chat-sessions").join(format!("{id}.jsonl"))
}
