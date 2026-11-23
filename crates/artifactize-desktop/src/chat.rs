//! Typed IPC over the library's worktree-bound Chat.
use artifactize_core::{chat, runs};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{
    App, Failure,
    browse::{BranchKey, PathKey},
    ids::{RunKey, SessionKey},
};

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatModel {
    pub backend: String,
    pub model: String,
    pub effort: Option<String>,
}

impl TryFrom<ChatModel> for chat::Model {
    type Error = Failure;

    fn try_from(model: ChatModel) -> Result<Self, Failure> {
        Ok(Self {
            backend: model.backend.parse().map_err(Failure::Invalid)?,
            model: model.model.parse().map_err(Failure::Invalid)?,
            effort: model.effort,
        })
    }
}

impl From<chat::Model> for ChatModel {
    fn from(model: chat::Model) -> Self {
        Self {
            backend: model.backend.to_string(),
            model: model.model.to_string(),
            effort: model.effort,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ChatSessionView {
    Conversation,
    Graph,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatSession {
    pub id: SessionKey,
    pub title: String,
    pub worktree: PathKey,
    pub project: String,
    pub branch: String,
    pub branch_key: BranchKey,
    pub view: ChatSessionView,
    pub unavailable: Option<String>,
    pub selection: ChatModel,
    pub created_at: String,
    pub updated_at: String,
    pub running: bool,
    pub outcome: Option<ChatOutcome>,
    pub unseen: bool,
}

impl From<chat::Session> for ChatSession {
    fn from(session: chat::Session) -> Self {
        let label = session.label();
        let branch_key = BranchKey::new(&session.branch());
        let outcome = session.outcome().map(Into::into);
        let unseen = session.unseen();
        Self {
            id: session.id.into(),
            title: session.title,
            worktree: PathKey::new(&session.worktree),
            project: label.project,
            branch: label.branch,
            branch_key,
            view: match session.view {
                chat::SessionView::Conversation => ChatSessionView::Conversation,
                chat::SessionView::Graph => ChatSessionView::Graph,
            },
            unavailable: session.unavailable,
            selection: session.selection.into(),
            created_at: session.created_at.to_string(),
            updated_at: session.updated_at.to_string(),
            running: session.running,
            outcome,
            unseen,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ChatOutcome {
    Running,
    Completed,
    Failed,
    Stopped,
    Interrupted,
}

impl From<chat::Outcome> for ChatOutcome {
    fn from(outcome: chat::Outcome) -> Self {
        match outcome {
            chat::Outcome::Running => Self::Running,
            chat::Outcome::Completed => Self::Completed,
            chat::Outcome::Failed => Self::Failed,
            chat::Outcome::Stopped => Self::Stopped,
            chat::Outcome::Interrupted => Self::Interrupted,
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

#[derive(Debug, Clone, Copy, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ChatToolKind {
    Read,
    Edit,
    Command,
    Verify,
    Other,
}

impl From<chat::ToolKind> for ChatToolKind {
    fn from(kind: chat::ToolKind) -> Self {
        match kind {
            chat::ToolKind::Read => Self::Read,
            chat::ToolKind::Edit => Self::Edit,
            chat::ToolKind::Command => Self::Command,
            chat::ToolKind::Verify => Self::Verify,
            chat::ToolKind::Other => Self::Other,
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ChatBlock {
    Text {
        text: String,
    },
    Tool {
        name: String,
        arguments: String,
        output: Option<String>,
        failed: Option<bool>,
        tool_kind: ChatToolKind,
        summary: String,
        exit_code: Option<i32>,
        omitted_lines: u32,
        output_truncated: bool,
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
                result,
                tool_kind,
                summary,
            } => {
                let (output, failed, exit_code, omitted_lines, output_truncated) = result
                    .map(|result| {
                        (
                            Some(result.output),
                            result.failed,
                            result.exit_code,
                            u32::try_from(result.omitted_lines).unwrap_or(u32::MAX),
                            result.output_truncated,
                        )
                    })
                    .unwrap_or_default();
                Self::Tool {
                    name,
                    arguments,
                    output,
                    failed,
                    tool_kind: tool_kind.into(),
                    summary,
                    exit_code,
                    omitted_lines,
                    output_truncated,
                }
            }
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

/// A live branch and the checkout a new Chat on it can use.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatBranch {
    pub key: BranchKey,
    pub project: String,
    pub name: String,
    pub workspace: PathKey,
}

/// Live branches known through Runs, launch, or saved Chat sessions; ignores Verify search.
#[tauri::command]
#[specta::specta]
pub async fn chat_branches(app: tauri::State<'_, App>) -> Result<Vec<ChatBranch>, Failure> {
    let runs = artifactize_core::store::read_catalog(&app.launch.state)
        .await
        .map_err(Failure::Read)?;
    let launch = app.launch.repo.clone();
    let mut branches = crate::failure::joined(
        tokio::task::spawn_blocking(move || {
            use artifactize_core::monitor::live;
            live::branches(&runs, launch.as_deref(), &mut live::Discovery::default())
                .branches
                .into_iter()
                .map(|branch| ChatBranch {
                    key: BranchKey::new(&branch.place),
                    project: branch.project,
                    name: branch.name,
                    workspace: PathKey::new(&branch.workspace),
                })
                .collect::<Vec<_>>()
        })
        .await,
    )?;
    let sessions = app.chat.list(None, None).await.map_err(Failure::Read)?;
    app.chat_worktrees.watch(&sessions);
    for session in sessions
        .into_iter()
        .filter(|session| session.unavailable.is_none())
    {
        let key = BranchKey::new(&session.branch());
        if !branches.iter().any(|branch| branch.key == key) {
            let label = session.label();
            branches.push(ChatBranch {
                key,
                project: label.project,
                name: label.branch,
                workspace: PathKey::new(&session.worktree),
            });
        }
    }
    Ok(branches)
}

/// Read the starting model for a new session. The pending call is the loading state;
/// `null` means no provider lists a model and the worktree has no previous session.
#[tauri::command]
#[specta::specta]
pub async fn chat_default_model(
    app: tauri::State<'_, App>,
    worktree: PathKey,
    branch: Option<BranchKey>,
) -> Result<Option<ChatModel>, Failure> {
    app.chat
        .default_model_for(
            &worktree.to_path(),
            branch.as_ref().map(BranchKey::to_place).as_ref(),
        )
        .await
        .map(|model| model.map(Into::into))
        .map_err(Failure::Read)
}

/// Create an empty Chat session for the chosen worktree; `null` uses its default model.
#[tauri::command]
#[specta::specta]
pub async fn chat_create(
    app: tauri::State<'_, App>,
    worktree: PathKey,
    branch: Option<BranchKey>,
    selection: Option<ChatModel>,
) -> Result<ChatSession, Failure> {
    app.chat
        .create_for_branch(
            &worktree.to_path(),
            branch.as_ref().map(BranchKey::to_place).as_ref(),
            selection.map(TryInto::try_into).transpose()?,
        )
        .await
        .map(Into::into)
        .map_err(Failure::Action)
}

/// Save the selected model and effort for the next turn without sending a message.
#[tauri::command]
#[specta::specta]
pub async fn chat_set_model(
    app: tauri::State<'_, App>,
    id: SessionKey,
    selection: ChatModel,
) -> Result<ChatSession, Failure> {
    app.chat
        .set_model(&id.into_inner(), selection.try_into()?)
        .await
        .map(Into::into)
        .map_err(Failure::Action)
}

/// List saved sessions newest first. `null` applies no worktree or query filter.
#[tauri::command]
#[specta::specta]
pub async fn chat_list(
    app: tauri::State<'_, App>,
    worktree: Option<PathKey>,
    query: Option<String>,
) -> Result<Vec<ChatSession>, Failure> {
    let worktree = worktree.map(|key| key.to_path());
    app.chat
        .list(worktree.as_deref(), query.as_deref())
        .await
        .map(|sessions| {
            app.chat_worktrees.watch(&sessions);
            sessions.into_iter().map(Into::into).collect()
        })
        .map_err(Failure::Read)
}

/// Navigate to the newest usable Chat of a branch, or its newest saved Chat if none can work.
#[tauri::command]
#[specta::specta]
pub async fn chat_latest_for_branch(
    app: tauri::State<'_, App>,
    branch: BranchKey,
) -> Result<Option<ChatSession>, Failure> {
    app.chat
        .latest_for_navigation(&branch.to_place())
        .await
        .map(|session| session.map(Into::into))
        .map_err(Failure::Read)
}

/// Keep the chosen view for this session without sending a message or reordering Chat.
#[tauri::command]
#[specta::specta]
pub async fn chat_set_view(
    app: tauri::State<'_, App>,
    id: SessionKey,
    view: ChatSessionView,
) -> Result<ChatSession, Failure> {
    let view = match view {
        ChatSessionView::Conversation => chat::SessionView::Conversation,
        ChatSessionView::Graph => chat::SessionView::Graph,
    };
    app.chat
        .set_view(&id.into_inner(), view)
        .await
        .map(Into::into)
        .map_err(Failure::Action)
}

/// Acknowledge the latest settled outcome for this machine user.
#[tauri::command]
#[specta::specta]
pub async fn chat_mark_seen(app: tauri::State<'_, App>, id: SessionKey) -> Result<(), Failure> {
    app.chat
        .mark_seen(&id.into_inner())
        .await
        .map_err(Failure::Action)
}

/// Read a session and its live transcript. `sessionChanged` names sessions to refresh; Run
/// cards can change separately on `stateChanged`, after their originating turn has ended.
#[tauri::command]
#[specta::specta]
pub async fn chat_open(app: tauri::State<'_, App>, id: SessionKey) -> Result<ChatView, Failure> {
    let id = id.into_inner();
    app.chat_stream.opened(&app.chat, &id);
    app.chat
        .open(&id)
        .await
        .map(|view| {
            app.chat_worktrees
                .watch(std::slice::from_ref(&view.session));
            ChatView {
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
            }
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

/// Ask the library to stop the turn, regardless of which process owns it.
#[tauri::command]
#[specta::specta]
pub async fn chat_stop(app: tauri::State<'_, App>, id: SessionKey) -> Result<bool, Failure> {
    app.chat
        .stop(&id.into_inner())
        .await
        .map_err(Failure::Action)
}

/// Open an HTTP(S) link from a conversation in the system browser.
#[tauri::command]
#[specta::specta]
pub async fn chat_open_link(url: String) -> Result<(), Failure> {
    open_web_link(&url, |url| async move {
        artifactize_tools::opener::open(std::ffi::OsStr::new(url.as_str())).await
    })
    .await
}

async fn open_web_link<O: Future<Output = std::io::Result<()>>>(
    input: &str,
    open: impl FnOnce(tauri::Url) -> O,
) -> Result<(), Failure> {
    let url = tauri::Url::parse(input).map_err(|error| Failure::Invalid(error.to_string()))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(Failure::Invalid(
            "Chat links must be HTTP or HTTPS URLs.".into(),
        ));
    }
    open(url)
        .await
        .map_err(|error| Failure::Action(format!("Cannot open link: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn chat_opens_only_a_web_url_in_the_system_browser() {
        let url = "https://example.test/path?topic=a%20b#section";
        open_web_link(url, |target| async move {
            assert_eq!(target.as_str(), url);
            Ok(())
        })
        .await
        .unwrap();
        for invalid in [
            "file:///private/report",
            "javascript:void(0)",
            "../relative",
        ] {
            assert!(matches!(
                open_web_link(invalid, |_| async { panic!("not a web link") }).await,
                Err(Failure::Invalid(_))
            ));
        }
    }
}
