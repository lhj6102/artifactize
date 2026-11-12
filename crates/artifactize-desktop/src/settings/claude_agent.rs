//! Claude Code's own sign-in for the `claude-agent` backend: its state as Claude Code reports
//! it, and `claude auth login`/`logout` run as `artifactize login|logout claude-agent` run
//! them. Claude Code keeps its login; nothing of it reaches artifactize.
use super::sign_in::Flow;
use crate::ids::SignInId;
use crate::{App, Failure};
use artifactize_core::auth::claude_agent;
use serde::Serialize;
use specta::Type;

/// Claude Code's sign-in, which shows the URL Claude Code printed and takes a pasted code.
pub(super) struct ClaudeCode;

impl Flow for ClaudeCode {
    type Prompt = claude_agent::Prompt;
    const CANCELLED: &'static str = "Claude Code sign-in was cancelled.";
    const NOT_WAITING: &'static str = "No Claude Code sign-in is waiting for a code.";
    const ENDED: &'static str = "The Claude Code sign-in already ended.";
    const WAITED: &'static str = "The Claude Code sign-in is already being waited for.";
}

/// Claude Code's sign-in, as Claude Code reports it.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ClaudeAgentStatus {
    /// `claude` is not on `PATH`.
    Missing,
    SignedOut,
    /// Signed in with `method` (for example `claude.ai`), on `plan` (for example `max`).
    SignedIn {
        method: String,
        plan: Option<String>,
    },
    /// Claude Code could not tell, and why.
    Failed {
        error: String,
    },
}

impl From<claude_agent::Status> for ClaudeAgentStatus {
    fn from(status: claude_agent::Status) -> Self {
        match status {
            claude_agent::Status::Missing => Self::Missing,
            claude_agent::Status::SignedOut => Self::SignedOut,
            claude_agent::Status::SignedIn { method, plan } => Self::SignedIn { method, plan },
            claude_agent::Status::Failed { error } => Self::Failed { error },
        }
    }
}

async fn read_status() -> ClaudeAgentStatus {
    claude_agent::status().await.into()
}

/// Ask Claude Code whether it is signed in (`claude auth status`).
#[tauri::command]
#[specta::specta]
pub async fn claude_agent_status() -> Result<ClaudeAgentStatus, Failure> {
    Ok(read_status().await)
}

/// What a Claude Code sign-in shows: the URL to open when the browser Claude Code opened did
/// not, whose page then gives a code to paste.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeAgentPrompt {
    pub url: String,
    /// The sign-in to wait for with [`claude_agent_login_wait`].
    pub sign_in: SignInId,
}

/// Start `login claude-agent`: `claude auth login` runs and opens the browser. Returns the URL
/// it printed; finish with [`claude_agent_login_wait`].
#[tauri::command]
#[specta::specta]
pub async fn claude_agent_login_start(
    app: tauri::State<'_, App>,
) -> Result<ClaudeAgentPrompt, Failure> {
    let (sign_in, prompt) = app
        .settings
        .claude_sign_ins
        .start(|pasted, shown| {
            tokio::spawn(async move {
                claude_agent::login_with(Some(pasted), |prompt| {
                    let _ = shown.send(prompt.clone());
                })
                .await
            })
        })
        .await?;
    Ok(ClaudeAgentPrompt {
        url: prompt.url,
        sign_in,
    })
}

/// Wait for sign-in `sign_in` (from [`claude_agent_login_start`]) to end; returns Claude Code's
/// sign-in as it then reports it.
#[tauri::command]
#[specta::specta]
pub async fn claude_agent_login_wait(
    app: tauri::State<'_, App>,
    sign_in: SignInId,
) -> Result<ClaudeAgentStatus, Failure> {
    app.settings.claude_sign_ins.wait(sign_in).await?;
    Ok(read_status().await)
}

/// Give the sign-in the code Claude Code's sign-in page shows, as the CLI accepts on stdin.
#[tauri::command]
#[specta::specta]
pub fn claude_agent_login_paste(app: tauri::State<'_, App>, code: String) -> Result<(), Failure> {
    app.settings.claude_sign_ins.paste(code)
}

/// Stop the sign-in in progress: Claude Code is stopped before it saves a login.
#[tauri::command]
#[specta::specta]
pub fn claude_agent_login_cancel(app: tauri::State<'_, App>) {
    app.settings.claude_sign_ins.cancel();
}

/// `logout claude-agent`: run `claude auth logout`; returns the sign-in as Claude Code then
/// reports it.
#[tauri::command]
#[specta::specta]
pub async fn claude_agent_logout() -> Result<ClaudeAgentStatus, Failure> {
    claude_agent::logout().await.map_err(Failure::Account)?;
    Ok(read_status().await)
}
