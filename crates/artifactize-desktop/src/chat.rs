//! Typed IPC over the library's worktree-bound Chat (Direct HTTP + unified ACP).
use artifactize_core::{acp, chat, config, runs};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{
    App, Failure,
    browse::{BranchKey, PathKey},
    ids::{RunKey, SessionKey},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ChatConfigCategory {
    Model,
    Mode,
    ThoughtLevel,
    Other,
}

impl From<acp::ConfigCategory> for ChatConfigCategory {
    fn from(category: acp::ConfigCategory) -> Self {
        match category {
            acp::ConfigCategory::Model => Self::Model,
            acp::ConfigCategory::Mode => Self::Mode,
            acp::ConfigCategory::ThoughtLevel => Self::ThoughtLevel,
            acp::ConfigCategory::Other => Self::Other,
        }
    }
}

impl From<ChatConfigCategory> for acp::ConfigCategory {
    fn from(category: ChatConfigCategory) -> Self {
        match category {
            ChatConfigCategory::Model => Self::Model,
            ChatConfigCategory::Mode => Self::Mode,
            ChatConfigCategory::ThoughtLevel => Self::ThoughtLevel,
            ChatConfigCategory::Other => Self::Other,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatConfigChoice {
    pub value: String,
    pub name: String,
    pub description: Option<String>,
    pub bypass_warning: bool,
}

impl From<acp::ConfigChoice> for ChatConfigChoice {
    fn from(choice: acp::ConfigChoice) -> Self {
        Self {
            value: choice.value,
            name: choice.name,
            description: choice.description,
            bypass_warning: choice.bypass_warning,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatConfigOption {
    pub id: String,
    pub name: String,
    pub category: ChatConfigCategory,
    pub current_value: String,
    pub choices: Vec<ChatConfigChoice>,
}

impl From<acp::ConfigOption> for ChatConfigOption {
    fn from(option: acp::ConfigOption) -> Self {
        Self {
            id: option.id,
            name: option.name,
            category: option.category.into(),
            current_value: option.current_value,
            choices: option.choices.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatConfigSelection {
    pub id: String,
    pub value: String,
    #[serde(default)]
    pub category: Option<ChatConfigCategory>,
}

impl From<acp::ConfigSelection> for ChatConfigSelection {
    fn from(selection: acp::ConfigSelection) -> Self {
        Self {
            id: selection.id,
            value: selection.value,
            category: selection.category.map(Into::into),
        }
    }
}

impl From<ChatConfigSelection> for acp::ConfigSelection {
    fn from(selection: ChatConfigSelection) -> Self {
        Self {
            id: selection.id,
            value: selection.value,
            category: selection.category.map(Into::into),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChatDirectBackend {
    Builtin { provider: String },
    Custom { name: String },
}

impl TryFrom<ChatDirectBackend> for config::DirectBackend {
    type Error = Failure;

    fn try_from(backend: ChatDirectBackend) -> Result<Self, Failure> {
        match backend {
            ChatDirectBackend::Builtin { provider } => {
                let provider = config::HttpProvider::named(&provider).ok_or_else(|| {
                    Failure::Invalid(format!(
                        "{provider:?} is not a built-in direct HTTP provider."
                    ))
                })?;
                Ok(Self::Builtin { provider })
            }
            ChatDirectBackend::Custom { name } => Ok(Self::Custom {
                name: name.parse().map_err(Failure::Invalid)?,
            }),
        }
    }
}

impl From<config::DirectBackend> for ChatDirectBackend {
    fn from(backend: config::DirectBackend) -> Self {
        match backend {
            config::DirectBackend::Builtin { provider } => Self::Builtin {
                provider: provider.as_str().to_owned(),
            },
            config::DirectBackend::Custom { name } => Self::Custom {
                name: name.as_str().to_owned(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "engine", rename_all = "camelCase")]
pub enum ChatTarget {
    Direct {
        backend: ChatDirectBackend,
        model: String,
        #[serde(default)]
        effort: Option<String>,
    },
    Acp {
        agent: String,
        #[serde(default)]
        config: Vec<ChatConfigSelection>,
    },
    LegacyClaudeAgent {
        model: String,
        #[serde(default)]
        effort: Option<String>,
    },
}

impl TryFrom<ChatTarget> for chat::ChatTarget {
    type Error = Failure;

    fn try_from(target: ChatTarget) -> Result<Self, Failure> {
        match target {
            ChatTarget::Direct {
                backend,
                model,
                effort,
            } => Ok(Self::Direct {
                backend: backend.try_into()?,
                model: model.parse().map_err(Failure::Invalid)?,
                effort,
            }),
            ChatTarget::Acp { agent, config } => Ok(Self::Acp {
                agent: agent.parse().map_err(Failure::Invalid)?,
                config: config.into_iter().map(Into::into).collect(),
            }),
            ChatTarget::LegacyClaudeAgent { model, effort } => Ok(Self::LegacyClaudeAgent {
                model: model.parse().map_err(Failure::Invalid)?,
                effort,
            }),
        }
    }
}

impl From<chat::ChatTarget> for ChatTarget {
    fn from(target: chat::ChatTarget) -> Self {
        match target {
            chat::ChatTarget::Direct {
                backend,
                model,
                effort,
            } => Self::Direct {
                backend: backend.into(),
                model: model.to_string(),
                effort,
            },
            chat::ChatTarget::Acp { agent, config } => Self::Acp {
                agent: agent.to_string(),
                config: config.into_iter().map(Into::into).collect(),
            },
            chat::ChatTarget::LegacyClaudeAgent { model, effort } => Self::LegacyClaudeAgent {
                model: model.to_string(),
                effort,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ChatVerifyBridgeState {
    NotConnected,
    Connected,
}

impl From<acp::VerifyBridgeState> for ChatVerifyBridgeState {
    fn from(state: acp::VerifyBridgeState) -> Self {
        match state {
            acp::VerifyBridgeState::NotConnected => Self::NotConnected,
            acp::VerifyBridgeState::Connected => Self::Connected,
        }
    }
}

/// What a chat's turn is doing now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ChatActivity {
    /// No turn runs.
    Idle,
    /// A turn runs.
    Running,
    /// A turn runs and waits for an answer to a permission request.
    WaitingPermission,
}

impl ChatActivity {
    /// From the library's live flags; a permission waits only while its turn runs.
    fn new(running: bool, waiting_permission: bool) -> Self {
        match (running, waiting_permission) {
            (false, _) => Self::Idle,
            (true, false) => Self::Running,
            (true, true) => Self::WaitingPermission,
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
    pub target: Option<ChatTarget>,
    pub created_at: String,
    pub updated_at: String,
    pub activity: ChatActivity,
    pub legacy_migration_notice: bool,
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
            target: session.target.map(Into::into),
            created_at: session.created_at.to_string(),
            updated_at: session.updated_at.to_string(),
            activity: ChatActivity::new(session.running, session.waiting_permission),
            legacy_migration_notice: session.legacy_migration_notice,
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

#[derive(Debug, Clone, Copy, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ChatToolStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl From<acp::ToolStatus> for ChatToolStatus {
    fn from(status: acp::ToolStatus) -> Self {
        match status {
            acp::ToolStatus::Pending => Self::Pending,
            acp::ToolStatus::Running => Self::Running,
            acp::ToolStatus::Completed => Self::Completed,
            acp::ToolStatus::Failed => Self::Failed,
            acp::ToolStatus::Cancelled => Self::Cancelled,
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatToolLocation {
    pub path: String,
    pub line: Option<u32>,
}

impl From<acp::ToolLocation> for ChatToolLocation {
    fn from(location: acp::ToolLocation) -> Self {
        Self {
            path: location.path,
            line: location.line,
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatToolDiff {
    pub path: String,
    pub old_text: Option<String>,
    pub new_text: String,
    pub truncated: bool,
}

impl From<acp::ToolDiff> for ChatToolDiff {
    fn from(diff: acp::ToolDiff) -> Self {
        Self {
            path: diff.path,
            old_text: diff.old_text,
            new_text: diff.new_text,
            truncated: diff.truncated,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ChatPlanStepStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

impl From<acp::PlanStepStatus> for ChatPlanStepStatus {
    fn from(status: acp::PlanStepStatus) -> Self {
        match status {
            acp::PlanStepStatus::Pending => Self::Pending,
            acp::PlanStepStatus::InProgress => Self::InProgress,
            acp::PlanStepStatus::Completed => Self::Completed,
            acp::PlanStepStatus::Failed => Self::Failed,
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatPlanStep {
    pub content: String,
    pub status: ChatPlanStepStatus,
    pub priority: Option<String>,
}

impl From<acp::PlanStep> for ChatPlanStep {
    fn from(step: acp::PlanStep) -> Self {
        Self {
            content: step.content,
            status: step.status.into(),
            priority: step.priority,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ChatPermissionOptionKind {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
}

impl From<acp::PermissionOptionKind> for ChatPermissionOptionKind {
    fn from(kind: acp::PermissionOptionKind) -> Self {
        match kind {
            acp::PermissionOptionKind::AllowOnce => Self::AllowOnce,
            acp::PermissionOptionKind::AllowAlways => Self::AllowAlways,
            acp::PermissionOptionKind::RejectOnce => Self::RejectOnce,
            acp::PermissionOptionKind::RejectAlways => Self::RejectAlways,
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatPermissionOption {
    pub option_id: String,
    pub name: String,
    pub kind: ChatPermissionOptionKind,
    pub persists_in_agent_settings: bool,
}

impl From<acp::PermissionOption> for ChatPermissionOption {
    fn from(option: acp::PermissionOption) -> Self {
        Self {
            option_id: option.option_id,
            name: option.name,
            kind: option.kind.into(),
            persists_in_agent_settings: option.persists_in_agent_settings,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ChatPermissionStatus {
    Pending,
    Selected,
    Delivering,
    Delivered,
    Cancelled,
    Expired,
}

impl From<acp::PermissionStatus> for ChatPermissionStatus {
    fn from(status: acp::PermissionStatus) -> Self {
        match status {
            acp::PermissionStatus::Pending => Self::Pending,
            acp::PermissionStatus::Selected => Self::Selected,
            acp::PermissionStatus::Delivering => Self::Delivering,
            acp::PermissionStatus::Delivered => Self::Delivered,
            acp::PermissionStatus::Cancelled => Self::Cancelled,
            acp::PermissionStatus::Expired => Self::Expired,
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
    Thought {
        text: String,
    },
    Tool {
        call_id: Option<String>,
        name: String,
        title: Option<String>,
        arguments: String,
        output: Option<String>,
        failed: Option<bool>,
        tool_kind: ChatToolKind,
        status: ChatToolStatus,
        summary: String,
        exit_code: Option<i32>,
        omitted_lines: u32,
        output_truncated: bool,
        locations: Vec<ChatToolLocation>,
        diff: Option<ChatToolDiff>,
    },
    Plan {
        steps: Vec<ChatPlanStep>,
    },
    Permission {
        permission_id: String,
        title: String,
        tool_kind: ChatToolKind,
        summary: String,
        detail: Option<String>,
        options: Vec<ChatPermissionOption>,
        status: ChatPermissionStatus,
        selected_option_id: Option<String>,
    },
    Notice {
        text: String,
    },
    Handoff {
        from: String,
        to: String,
        summary: String,
    },
    Run {
        card: ChatRun,
    },
}

impl From<chat::Block> for ChatBlock {
    fn from(block: chat::Block) -> Self {
        match block {
            chat::Block::Text { text } => Self::Text { text },
            chat::Block::Thought { text } => Self::Thought { text },
            chat::Block::Tool {
                call_id,
                name,
                title,
                arguments,
                result,
                tool_kind,
                status,
                summary,
                locations,
                diff,
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
                    call_id,
                    name,
                    title,
                    arguments,
                    output,
                    failed,
                    tool_kind: tool_kind.into(),
                    status: status.into(),
                    summary,
                    exit_code,
                    omitted_lines,
                    output_truncated,
                    locations: locations.into_iter().map(Into::into).collect(),
                    diff: diff.map(Into::into),
                }
            }
            chat::Block::Plan { steps } => Self::Plan {
                steps: steps.into_iter().map(Into::into).collect(),
            },
            chat::Block::Permission {
                permission_id,
                title,
                tool_kind,
                summary,
                detail,
                options,
                status,
                selected_option_id,
            } => Self::Permission {
                permission_id,
                title,
                tool_kind: tool_kind.into(),
                summary,
                detail,
                options: options.into_iter().map(Into::into).collect(),
                status: status.into(),
                selected_option_id,
            },
            chat::Block::Notice { text } => Self::Notice { text },
            chat::Block::Handoff { from, to, summary } => Self::Handoff { from, to, summary },
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
    WaitingPermission,
    Completed,
    Stopped,
    Interrupted,
    Failed { message: String },
}

impl From<chat::TurnState> for ChatTurnState {
    fn from(state: chat::TurnState) -> Self {
        match state {
            chat::TurnState::Running => Self::Running,
            chat::TurnState::WaitingPermission => Self::WaitingPermission,
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
    pub activity: ChatActivity,
    pub turns: Vec<ChatTurn>,
    pub live_config_options: Vec<ChatConfigOption>,
    pub verify_bridge_state: ChatVerifyBridgeState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ChatTrustScope {
    GitRepo { common_dir: PathKey },
    Directory { canonical_path: PathKey },
}

impl From<acp::trust::TrustScope> for ChatTrustScope {
    fn from(scope: acp::trust::TrustScope) -> Self {
        match scope {
            acp::trust::TrustScope::GitRepo { common_dir } => Self::GitRepo {
                common_dir: PathKey::new(&common_dir),
            },
            acp::trust::TrustScope::Directory { canonical_path } => Self::Directory {
                canonical_path: PathKey::new(&canonical_path),
            },
        }
    }
}

impl From<ChatTrustScope> for acp::trust::TrustScope {
    fn from(scope: ChatTrustScope) -> Self {
        match scope {
            ChatTrustScope::GitRepo { common_dir } => Self::GitRepo {
                common_dir: common_dir.to_path(),
            },
            ChatTrustScope::Directory { canonical_path } => Self::Directory {
                canonical_path: canonical_path.to_path(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatLauncherFingerprint {
    pub resolved_executable: PathKey,
    pub executable_sha256: String,
    pub args_hash: String,
}

impl From<ChatLauncherFingerprint> for acp::trust::LauncherFingerprint {
    fn from(fp: ChatLauncherFingerprint) -> Self {
        Self {
            resolved_executable: fp.resolved_executable.to_path(),
            executable_sha256: fp.executable_sha256,
            args_hash: fp.args_hash,
        }
    }
}

impl From<acp::trust::LauncherFingerprint> for ChatLauncherFingerprint {
    fn from(fp: acp::trust::LauncherFingerprint) -> Self {
        Self {
            resolved_executable: PathKey::new(&fp.resolved_executable),
            executable_sha256: fp.executable_sha256,
            args_hash: fp.args_hash,
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatTrustRequirement {
    pub agent_id: String,
    pub agent_name: String,
    pub scope: ChatTrustScope,
    pub scope_label: String,
    pub fingerprint: ChatLauncherFingerprint,
    pub disclosure: String,
    pub reason: String,
    pub covers_all_worktrees: bool,
}

impl From<acp::trust::TrustRequirement> for ChatTrustRequirement {
    fn from(req: acp::trust::TrustRequirement) -> Self {
        let covers_all_worktrees = matches!(req.scope, acp::trust::TrustScope::GitRepo { .. });
        let scope_label = req.scope.display_label();
        Self {
            agent_id: req.agent_id.to_string(),
            agent_name: req.agent_name,
            scope: req.scope.into(),
            scope_label,
            fingerprint: req.fingerprint.into(),
            disclosure: req.disclosure,
            reason: req.reason,
            covers_all_worktrees,
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatTrustRecord {
    pub agent_id: String,
    pub scope: ChatTrustScope,
    pub scope_label: String,
    pub fingerprint: ChatLauncherFingerprint,
    pub trusted_at: String,
}

impl From<acp::trust::TrustRecord> for ChatTrustRecord {
    fn from(record: acp::trust::TrustRecord) -> Self {
        let scope_label = record.scope.display_label();
        Self {
            agent_id: record.agent_id.to_string(),
            scope: record.scope.into(),
            scope_label,
            fingerprint: record.fingerprint.into(),
            trusted_at: record.granted_at.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChatSendResult {
    Accepted,
    TrustRequired {
        requirement: Box<ChatTrustRequirement>,
    },
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
    let listing = app
        .chat
        .branches(app.launch.repo.as_deref())
        .await
        .map_err(Failure::Read)?;
    app.chat_worktrees.watch(&listing.sessions);
    Ok(listing
        .branches
        .into_iter()
        .map(|branch| ChatBranch {
            key: BranchKey::new(&branch.place),
            project: branch.project,
            name: branch.name,
            workspace: PathKey::new(&branch.workspace),
        })
        .collect())
}

/// Read the starting target for a new session. `null` means no agent or provider is ready and
/// the worktree has no previous usable session.
#[tauri::command]
#[specta::specta]
pub async fn chat_default_target(
    app: tauri::State<'_, App>,
    worktree: PathKey,
    branch: Option<BranchKey>,
) -> Result<Option<ChatTarget>, Failure> {
    app.chat
        .default_target_for(
            &worktree.to_path(),
            branch.as_ref().map(BranchKey::to_place).as_ref(),
        )
        .await
        .map(|target| target.map(Into::into))
        .map_err(Failure::Read)
}

/// Create an empty Chat session for the chosen worktree; `null` leaves its target unset.
#[tauri::command]
#[specta::specta]
pub async fn chat_create(
    app: tauri::State<'_, App>,
    worktree: PathKey,
    branch: Option<BranchKey>,
    selection: Option<ChatTarget>,
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

/// Move a saved Chat to one of the live choices without adding a message.
#[tauri::command]
#[specta::specta]
pub async fn chat_set_worktree(
    app: tauri::State<'_, App>,
    id: SessionKey,
    worktree: String,
) -> Result<ChatSession, Failure> {
    let path = artifactize_tools::files::path_from_text(&worktree)
        .map_err(|_| Failure::Invalid(chat::MoveError::Invalid(worktree).to_string()))?;
    let session = app
        .chat
        .set_worktree(&id.into_inner(), &path, app.launch.repo.as_deref())
        .await
        .map_err(|error| match error {
            chat::MoveError::Invalid(_) => Failure::Invalid(error.to_string()),
            chat::MoveError::Busy => Failure::Busy(error.to_string()),
            chat::MoveError::Failed(message) => Failure::Action(message),
        })?;
    app.chat_worktrees.watch(std::slice::from_ref(&session));
    Ok(session.into())
}

/// Save the selected Chat target (ACP agent + config, or direct HTTP model + effort) for the
/// next turn without sending a message.
#[tauri::command]
#[specta::specta]
pub async fn chat_set_target(
    app: tauri::State<'_, App>,
    id: SessionKey,
    selection: ChatTarget,
) -> Result<ChatSession, Failure> {
    app.chat
        .set_target(&id.into_inner(), selection.try_into()?)
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

/// Read a session, its live transcript, permissions, and live ACP configOptions.
#[tauri::command]
#[specta::specta]
pub async fn chat_open(app: tauri::State<'_, App>, id: SessionKey) -> Result<ChatView, Failure> {
    let id = id.into_inner();
    app.chat_stream.opened(&app.chat, &id).await;
    app.chat
        .open(&id)
        .await
        .map(|view| {
            app.chat_worktrees
                .watch(std::slice::from_ref(&view.session));
            ChatView {
                session: view.session.into(),
                activity: ChatActivity::new(view.running, view.waiting_permission),
                turns: view
                    .turns
                    .into_iter()
                    .map(|turn| ChatTurn {
                        user: turn.user,
                        blocks: turn.blocks.into_iter().map(Into::into).collect(),
                        state: turn.state.into(),
                    })
                    .collect(),
                live_config_options: view
                    .live_config_options
                    .into_iter()
                    .map(Into::into)
                    .collect(),
                verify_bridge_state: view.verify_bridge_state.into(),
            }
        })
        .map_err(Failure::Read)
}

/// Accept a user message and start its streaming turn, or return `TrustRequired` before
/// recording anything if the ACP agent has not been trusted for this workspace yet.
#[tauri::command]
#[specta::specta]
pub async fn chat_send(
    app: tauri::State<'_, App>,
    id: SessionKey,
    text: String,
    selection: Option<ChatTarget>,
) -> Result<ChatSendResult, Failure> {
    match app
        .chat
        .send(
            &id.into_inner(),
            text,
            selection.map(TryInto::try_into).transpose()?,
        )
        .await
    {
        Ok(()) => Ok(ChatSendResult::Accepted),
        Err(chat::SendError::TrustRequired(requirement)) => Ok(ChatSendResult::TrustRequired {
            requirement: Box::new((*requirement).into()),
        }),
        Err(error @ chat::SendError::NoModel) => Err(Failure::NoModel(error.to_string())),
        Err(chat::SendError::LegacyRetired(message)) | Err(chat::SendError::Failed(message)) => {
            Err(Failure::Action(message))
        }
    }
}

/// Submit a user choice for a pending ACP permission request (`PENDING -> SELECTED`).
#[tauri::command]
#[specta::specta]
pub async fn chat_permission_respond(
    app: tauri::State<'_, App>,
    id: SessionKey,
    permission_id: String,
    option_id: String,
) -> Result<bool, Failure> {
    app.chat
        .respond_permission(&id.into_inner(), &permission_id, &option_id)
        .await
        .map_err(Failure::Action)
}

/// Record explicit machine-local Workspace Trust for an ACP agent on a worktree/repo scope, for
/// the launcher `fingerprint` the trust prompt showed.
#[tauri::command]
#[specta::specta]
pub async fn chat_trust_grant(
    app: tauri::State<'_, App>,
    worktree: PathKey,
    agent: String,
    fingerprint: ChatLauncherFingerprint,
) -> Result<ChatTrustRecord, Failure> {
    let agent_id: acp::AgentId = agent.parse().map_err(Failure::Invalid)?;
    app.chat
        .trust_workspace(&worktree.to_path(), &agent_id, &fingerprint.into())
        .await
        .map(Into::into)
        .map_err(Failure::Action)
}

/// List all saved machine-local ACP Workspace Trust records.
#[tauri::command]
#[specta::specta]
pub fn chat_trust_list(app: tauri::State<'_, App>) -> Result<Vec<ChatTrustRecord>, Failure> {
    app.chat
        .list_trust()
        .map(|rows| rows.into_iter().map(Into::into).collect())
        .map_err(Failure::Read)
}

/// Revoke a saved Workspace Trust record and terminate any matching warm ACP children.
#[tauri::command]
#[specta::specta]
pub async fn chat_trust_revoke(
    app: tauri::State<'_, App>,
    agent: String,
    scope: ChatTrustScope,
) -> Result<bool, Failure> {
    let agent_id: acp::AgentId = agent.parse().map_err(Failure::Invalid)?;
    app.chat
        .revoke_trust(&agent_id, &scope.into())
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
