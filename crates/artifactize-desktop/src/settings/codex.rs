//! The Codex sign-in: its state, the sign-in in progress and its commands.
use super::sign_in::Flow;
use crate::ids::SignInId;
use crate::{App, Failure};
use artifactize_core::auth::codex;
use serde::Serialize;
use specta::Type;

/// The Codex sign-in, which shows its URL and takes a pasted redirect URL.
pub(super) struct Codex;

impl Flow for Codex {
    type Prompt = codex::Prompt;
    const CANCELLED: &'static str = "Codex sign-in was cancelled.";
    const NOT_WAITING: &'static str = "No Codex sign-in is waiting for a URL.";
    const ENDED: &'static str = "The Codex sign-in already ended.";
    const WAITED: &'static str = "The Codex sign-in is already being waited for.";
}

/// A moment as the library counts it: whole seconds since 1970, as decimal text so no value of
/// the library's range is rounded on the way to the frontend.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(transparent)]
pub struct UnixSeconds(String);

impl From<codex::Timestamp> for UnixSeconds {
    fn from(time: codex::Timestamp) -> Self {
        Self(time.seconds().to_string())
    }
}

/// How long a stored sign-in's access token lasts.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Expiry {
    /// It still works.
    Usable { expires_at: UnixSeconds },
    /// It ran out; it refreshes on use unless the refresh token was revoked.
    Expired { expires_at: UnixSeconds },
}

/// What a read-only Codex auth file says of its token.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum FileExpiry {
    Usable { expires_at: Option<UnixSeconds> },
    Expired,
}

/// How Codex is signed in, read offline as the CLI does.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum CodexStatus {
    /// Not signed in.
    Absent,
    /// artifactize's own sign-in.
    Stored { expiry: Expiry },
    /// A read-only Codex auth file from ARTIFACTIZE_CODEX_AUTH_FILE; never written here.
    File { path: String, expiry: FileExpiry },
    /// Tokens cannot be stored where this state lives, and why.
    Refused { reason: String },
}

fn read_codex(app: &App) -> Result<CodexStatus, Failure> {
    let status = codex::status(Some(&app.launch.state), app.launch.repo.as_deref())
        .map_err(Failure::Account)?;
    Ok(match status {
        codex::Status::Absent => CodexStatus::Absent,
        codex::Status::Refused { reason } => CodexStatus::Refused { reason },
        codex::Status::Stored { expiry } => CodexStatus::Stored {
            expiry: match expiry {
                codex::StoredExpiry::Usable { expires_at } => Expiry::Usable {
                    expires_at: expires_at.into(),
                },
                codex::StoredExpiry::Expired { expires_at } => Expiry::Expired {
                    expires_at: expires_at.into(),
                },
            },
        },
        codex::Status::File { path, expiry } => CodexStatus::File {
            path: artifactize_tools::files::path_text(&path),
            expiry: match expiry {
                codex::FileExpiry::Usable { expires_at } => FileExpiry::Usable {
                    expires_at: expires_at.map(Into::into),
                },
                codex::FileExpiry::Expired => FileExpiry::Expired,
            },
        },
    })
}

/// The Codex sign-in state, offline.
#[tauri::command]
#[specta::specta]
pub fn codex_status(app: tauri::State<'_, App>) -> Result<CodexStatus, Failure> {
    read_codex(&app)
}

/// What the sign-in asks for: open `url`; `callback` says whether the browser can return
/// here (when the port is taken, only pasting the redirect URL finishes it).
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CodexPrompt {
    pub url: String,
    pub callback: bool,
    /// The sign-in to wait for with [`codex_login_wait`].
    pub sign_in: SignInId,
}

/// Start `login codex`: the sign-in URL to show (the system browser gets it too when
/// `browser`), and whether the browser can return here. Finish with [`codex_login_wait`].
#[tauri::command]
#[specta::specta]
pub async fn codex_login_start(
    app: tauri::State<'_, App>,
    browser: bool,
) -> Result<CodexPrompt, Failure> {
    let state = app.launch.state.clone();
    let repo = app.launch.repo.clone();
    let (sign_in, prompt) = app
        .settings
        .codex_sign_ins
        .start(|pasted, shown| {
            tokio::spawn(async move {
                codex::login_with(
                    Some(&state),
                    repo.as_deref(),
                    Some(pasted),
                    browser,
                    |prompt| {
                        let _ = shown.send(prompt.clone());
                    },
                )
                .await
            })
        })
        .await?;
    Ok(CodexPrompt {
        url: prompt.url,
        callback: prompt.callback,
        sign_in,
    })
}

/// Wait for sign-in `sign_in` (from [`codex_login_start`]) to finish; returns the new state.
/// The sign-in stays reachable meanwhile, so a paste or a cancel still reaches it; one that a
/// newer start replaced reads as cancelled and never touches the newer one.
#[tauri::command]
#[specta::specta]
pub async fn codex_login_wait(
    app: tauri::State<'_, App>,
    sign_in: SignInId,
) -> Result<CodexStatus, Failure> {
    app.settings.codex_sign_ins.wait(sign_in).await?;
    read_codex(&app)
}

/// Finish the sign-in with the URL the browser was sent to, as the CLI accepts on stdin.
#[tauri::command]
#[specta::specta]
pub fn codex_login_paste(app: tauri::State<'_, App>, redirect: String) -> Result<(), Failure> {
    app.settings.codex_sign_ins.paste(redirect)
}

/// Stop the sign-in in progress; nothing is saved, even if the browser returns later.
#[tauri::command]
#[specta::specta]
pub fn codex_login_cancel(app: tauri::State<'_, App>) {
    app.settings.codex_sign_ins.cancel();
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SignedOut {
    /// The store confirmed the revocation; otherwise only the local tokens were removed.
    pub revoked: bool,
    pub status: CodexStatus,
}

/// `logout codex`: revoke and remove artifactize's own tokens.
#[tauri::command]
#[specta::specta]
pub async fn codex_logout(app: tauri::State<'_, App>) -> Result<SignedOut, Failure> {
    let revoked = codex::logout(Some(&app.launch.state), app.launch.repo.as_deref())
        .await
        .map_err(Failure::Account)?;
    Ok(SignedOut {
        revoked,
        status: read_codex(&app)?,
    })
}
