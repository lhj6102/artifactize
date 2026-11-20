//! Typed IPC over the library's worktree-bound Chat.
use artifactize_core::{chat, runs};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{
    App, Failure,
    browse::PathKey,
    ids::{RunKey, SessionKey},
};

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatModel {
    pub backend: String,
    pub model: String,
}

impl TryFrom<ChatModel> for chat::Model {
    type Error = Failure;

    fn try_from(model: ChatModel) -> Result<Self, Failure> {
        Ok(Self {
            backend: model.backend.parse().map_err(Failure::Invalid)?,
            model: model.model.parse().map_err(Failure::Invalid)?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatSession {
    pub id: SessionKey,
    pub title: String,
    pub worktree: PathKey,
    pub selection: ChatModel,
    pub created_at: String,
    pub updated_at: String,
}

impl From<chat::Session> for ChatSession {
    fn from(session: chat::Session) -> Self {
        Self {
            id: session.id.into(),
            title: session.title,
            worktree: PathKey::new(&session.worktree),
            selection: ChatModel {
                backend: session.selection.backend.to_string(),
                model: session.selection.model.to_string(),
            },
            created_at: session.created_at.to_string(),
            updated_at: session.updated_at.to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ChatRunState {
    Preparing,
    Running,
    Waiting,
    WaitingHuman,
    Cancelling,
    Green,
    Red,
    Incomplete,
    Yellow,
    Error,
    Cancelled,
}

impl From<runs::State> for ChatRunState {
    fn from(state: runs::State) -> Self {
        match state {
            runs::State::Preparing => Self::Preparing,
            runs::State::Running => Self::Running,
            runs::State::Waiting => Self::Waiting,
            runs::State::WaitingHuman => Self::WaitingHuman,
            runs::State::Cancelling => Self::Cancelling,
            runs::State::Green => Self::Green,
            runs::State::Red => Self::Red,
            runs::State::Incomplete => Self::Incomplete,
            runs::State::Yellow => Self::Yellow,
            runs::State::Error => Self::Error,
            runs::State::Cancelled => Self::Cancelled,
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatRun {
    pub id: RunKey,
    pub state: Option<ChatRunState>,
    pub failed: Vec<String>,
    pub waiting: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChatBlock {
    Text {
        text: String,
    },
    Tool {
        name: String,
        arguments: String,
        output: Option<String>,
        failed: Option<bool>,
    },
    Run {
        card: ChatRun,
    },
}

impl From<chat::Block> for ChatBlock {
    fn from(block: chat::Block) -> Self {
        match block {
            chat::Block::Text { text } => Self::Text { text },
            chat::Block::Tool {
                name,
                arguments,
                output,
                failed,
            } => Self::Tool {
                name,
                arguments,
                output,
                failed,
            },
            chat::Block::Run { card } => Self::Run {
                card: ChatRun {
                    id: card.id.into(),
                    state: card.state.map(Into::into),
                    failed: card.failed.into_iter().map(Into::into).collect(),
                    waiting: card.waiting.into_iter().map(Into::into).collect(),
                },
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChatTurnState {
    Running,
    Completed,
    Stopped,
    Interrupted,
    Failed { message: String },
}

impl From<chat::TurnState> for ChatTurnState {
    fn from(state: chat::TurnState) -> Self {
        match state {
            chat::TurnState::Running => Self::Running,
            chat::TurnState::Completed => Self::Completed,
            chat::TurnState::Stopped => Self::Stopped,
            chat::TurnState::Interrupted => Self::Interrupted,
            chat::TurnState::Failed { message } => Self::Failed { message },
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatTurn {
    pub user: String,
    pub blocks: Vec<ChatBlock>,
    pub state: ChatTurnState,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatView {
    pub session: ChatSession,
    pub running: bool,
    pub turns: Vec<ChatTurn>,
}

/// Create an empty Chat session for the chosen worktree and a model its provider lists.
#[tauri::command]
#[specta::specta]
pub async fn chat_create(
    app: tauri::State<'_, App>,
    worktree: PathKey,
    selection: ChatModel,
) -> Result<ChatSession, Failure> {
    app.chat
        .create(&worktree.to_path(), selection.try_into()?)
        .await
        .map(Into::into)
        .map_err(Failure::Action)
}

/// List saved sessions newest first. `null` lists all worktrees.
#[tauri::command]
#[specta::specta]
pub async fn chat_list(
    app: tauri::State<'_, App>,
    worktree: Option<PathKey>,
) -> Result<Vec<ChatSession>, Failure> {
    let worktree = worktree.map(|key| key.to_path());
    app.chat
        .list(worktree.as_deref())
        .await
        .map(|sessions| sessions.into_iter().map(Into::into).collect())
        .map_err(Failure::Read)
}

/// Read a session and its live transcript. `sessionChanged` names sessions to refresh; Run
/// cards can change separately on `stateChanged`, after their originating turn has ended.
#[tauri::command]
#[specta::specta]
pub async fn chat_open(app: tauri::State<'_, App>, id: SessionKey) -> Result<ChatView, Failure> {
    app.chat
        .open(&id.into_inner())
        .await
        .map(|view| ChatView {
            session: view.session.into(),
            running: view.running,
            turns: view
                .turns
                .into_iter()
                .map(|turn| ChatTurn {
                    user: turn.user,
                    blocks: turn.blocks.into_iter().map(Into::into).collect(),
                    state: turn.state.into(),
                })
                .collect(),
        })
        .map_err(Failure::Read)
}

/// Accept a user message and start its streaming turn. A supplied model selection is saved
/// for later turns; `null` keeps the current model.
#[tauri::command]
#[specta::specta]
pub async fn chat_send(
    app: tauri::State<'_, App>,
    id: SessionKey,
    text: String,
    selection: Option<ChatModel>,
) -> Result<(), Failure> {
    app.chat
        .send(
            &id.into_inner(),
            text,
            selection.map(TryInto::try_into).transpose()?,
        )
        .await
        .map_err(Failure::Action)
}

/// Stop this process's in-flight turn; Verify Runs already accepted keep running.
#[tauri::command]
#[specta::specta]
pub fn chat_stop(app: tauri::State<'_, App>, id: SessionKey) -> bool {
    app.chat.stop(&id.into_inner())
}
