//! Antigravity IPC forwards native account operations to the library.
use super::sign_in::{Flow, NativeStart};
use crate::ids::SignInId;
use crate::{App, Failure};
use artifactize_core::auth::antigravity;
use serde::Serialize;
use specta::Type;

/// Antigravity's sign-in, which shows the URL Antigravity printed and takes a pasted code.
pub(super) struct GoogleAccount;

impl Flow for GoogleAccount {
    type Prompt = antigravity::Prompt;
    const CANCELLED: &'static str = "Antigravity sign-in was cancelled.";
    const NOT_WAITING: &'static str = "No Antigravity sign-in is waiting for a code.";
    const ENDED: &'static str = "The Antigravity sign-in already ended.";
    const WAITED: &'static str = "The Antigravity sign-in is already being waited for.";
}

/// Antigravity's sign-in, as Antigravity reports it.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AntigravityStatus {
    /// `agy` is not on `PATH`.
    Missing,
    TooOld {
        version: String,
        error: String,
    },
    SignedOut,
    SignedIn,
    Quota {
        error: String,
    },
    RateLimited {
        error: String,
    },
    Failed {
        error: String,
    },
}

impl From<antigravity::Status> for AntigravityStatus {
    fn from(status: antigravity::Status) -> Self {
        match status {
            antigravity::Status::Missing => Self::Missing,
            antigravity::Status::SignedOut => Self::SignedOut,
            antigravity::Status::SignedIn => Self::SignedIn,
            antigravity::Status::TooOld { version, error } => Self::TooOld { version, error },
            antigravity::Status::Quota { error } => Self::Quota { error },
            antigravity::Status::RateLimited { error } => Self::RateLimited { error },
            antigravity::Status::Failed { error } => Self::Failed { error },
        }
    }
}

async fn read_status() -> AntigravityStatus {
    antigravity::status().await.into()
}

/// Ask Antigravity whether it is signed in (an ACP probe without a prompt).
#[tauri::command]
#[specta::specta]
pub async fn antigravity_status() -> Result<AntigravityStatus, Failure> {
    Ok(read_status().await)
}

/// The Google authorization URL. Its Antigravity callback page gives a code to paste.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AntigravityPrompt {
    pub url: String,
    /// The sign-in to wait for with [`antigravity_login_wait`].
    pub sign_in: SignInId,
}

/// Start `login antigravity`: `agy acp --login` prints a Google sign-in URL. Returns the URL
/// it printed; finish with [`antigravity_login_wait`].
#[tauri::command]
#[specta::specta]
pub async fn antigravity_login_start(
    app: tauri::State<'_, App>,
) -> Result<NativeStart<AntigravityPrompt, AntigravityStatus>, Failure> {
    let started = app
        .settings
        .antigravity_sign_ins
        .start(|pasted, shown| {
            tokio::spawn(async move {
                antigravity::login_with(pasted, |prompt| {
                    let _ = shown.send(prompt.clone());
                })
                .await
            })
        })
        .await?;
    Ok(match started {
        Some((sign_in, prompt)) => NativeStart::Prompt(AntigravityPrompt {
            url: prompt.url.to_string(),
            sign_in,
        }),
        None => NativeStart::Complete(read_status().await),
    })
}

/// Wait for sign-in `sign_in` (from [`antigravity_login_start`]) to end; returns Antigravity's
/// sign-in as it then reports it.
#[tauri::command]
#[specta::specta]
pub async fn antigravity_login_wait(
    app: tauri::State<'_, App>,
    sign_in: SignInId,
) -> Result<AntigravityStatus, Failure> {
    app.settings.antigravity_sign_ins.wait(sign_in).await?;
    Ok(read_status().await)
}

/// Give the sign-in the code Antigravity's sign-in page shows, as the CLI accepts on stdin.
#[tauri::command]
#[specta::specta]
pub fn antigravity_login_paste(
    app: tauri::State<'_, App>,
    sign_in: SignInId,
    code: String,
) -> Result<(), Failure> {
    app.settings.antigravity_sign_ins.paste(sign_in, code)
}

/// Stop the sign-in in progress: Antigravity is stopped before it saves a login.
#[tauri::command]
#[specta::specta]
pub fn antigravity_login_cancel(app: tauri::State<'_, App>, sign_in: SignInId) {
    app.settings.antigravity_sign_ins.cancel(sign_in);
}

/// `logout antigravity`: run ACP `logout`; returns the sign-in as Antigravity then
/// reports it.
#[tauri::command]
#[specta::specta]
pub async fn antigravity_logout() -> Result<AntigravityStatus, Failure> {
    antigravity::logout().await.map_err(Failure::Account)?;
    Ok(read_status().await)
}
