//! The shared remote review store: sign in and out, its status, and pushing results.
use crate::{App, Failure};
use artifactize_core::auth::remote;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Share {
    /// Reduced records without argv, captured output, tool calls or paths.
    Summary,
    /// Also the saved execution as is.
    Full,
}

impl From<Share> for remote::Share {
    fn from(share: Share) -> Self {
        match share {
            Share::Summary => remote::Share::Summary,
            Share::Full => remote::Share::Full,
        }
    }
}

impl From<remote::Share> for Share {
    fn from(share: remote::Share) -> Self {
        match share {
            remote::Share::Summary => Share::Summary,
            remote::Share::Full => Share::Full,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum TokenSource {
    /// ARTIFACTIZE_REMOTE_TOKEN.
    Env,
    /// Saved by `remote login`.
    File,
    None,
}

/// `remote status`: never the token.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RemoteStatus {
    /// No remote review store is configured.
    Unconfigured,
    Configured {
        url: String,
        share: Share,
        token_source: TokenSource,
        connection: Connection,
    },
}

/// What asking the store answered.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Connection {
    /// The token was accepted.
    SignedIn {
        principal: String,
        scopes: Vec<String>,
    },
    /// The store answered, but not with an accepted token.
    Refused { error: String },
    /// The store sent no HTTP response.
    Unreachable { error: String },
}

impl From<Option<remote::Status>> for RemoteStatus {
    fn from(status: Option<remote::Status>) -> Self {
        let Some(status) = status else {
            return Self::Unconfigured;
        };
        let connection = match status.connection {
            remote::Connection::SignedIn { principal, scopes } => Connection::SignedIn {
                principal: principal.to_string(),
                scopes: scopes.into_iter().map(|scope| scope.to_string()).collect(),
            },
            remote::Connection::Refused { error } => Connection::Refused { error },
            remote::Connection::Unreachable { error } => Connection::Unreachable { error },
        };
        Self::Configured {
            url: status.url,
            share: status.share.into(),
            token_source: match status.token_source {
                remote::TokenSource::Env => TokenSource::Env,
                remote::TokenSource::File => TokenSource::File,
                remote::TokenSource::None => TokenSource::None,
            },
            connection,
        }
    }
}

async fn remote_status_of(app: &App) -> Result<RemoteStatus, Failure> {
    remote::status(Some(&app.launch.state), app.launch.repo.as_deref())
        .await
        .map(Into::into)
        .map_err(Failure::Account)
}

/// `remote status`: URL, share level, token source, reachability, principal and scopes.
#[tauri::command]
#[specta::specta]
pub async fn remote_status(app: tauri::State<'_, App>) -> Result<RemoteStatus, Failure> {
    remote_status_of(&app).await
}

/// `remote login`: verify `token` with the store, then save it with remote.json. The token
/// arrives in the IPC body only; it is never logged, echoed or returned.
#[tauri::command]
#[specta::specta]
pub async fn remote_login(
    app: tauri::State<'_, App>,
    url: String,
    share: Share,
    token: String,
) -> Result<RemoteStatus, Failure> {
    let url = remote::parse_url(&url).map_err(Failure::Invalid)?;
    remote::login(
        Some(&app.launch.state),
        app.launch.repo.as_deref(),
        url,
        share.into(),
        token.trim().to_owned(),
    )
    .await
    .map_err(Failure::Account)?;
    remote_status_of(&app).await
}

/// `remote logout`: forget the stored token and remote.json; the server keeps the token
/// valid until it is revoked there.
#[tauri::command]
#[specta::specta]
pub async fn remote_logout(app: tauri::State<'_, App>) -> Result<RemoteStatus, Failure> {
    remote::logout(Some(&app.launch.state), app.launch.repo.as_deref())
        .map_err(Failure::Account)?;
    remote_status_of(&app).await
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Pushed {
    pub dry_run: bool,
    pub pushed: u32,
    pub existing: u32,
    pub skipped: u32,
}

/// `remote push`: re-send local GREEN/RED results the store does not have yet.
#[tauri::command]
#[specta::specta]
pub async fn remote_push(app: tauri::State<'_, App>, dry_run: bool) -> Result<Pushed, Failure> {
    let report = artifactize_core::remote::push(
        Some(&app.launch.state),
        app.launch.repo.as_deref(),
        dry_run,
    )
    .await
    .map_err(Failure::Account)?;
    let count = |value: usize| u32::try_from(value).unwrap_or(u32::MAX);
    Ok(Pushed {
        dry_run: report.dry_run,
        pushed: count(report.pushed),
        existing: count(report.existing),
        skipped: count(report.skipped),
    })
}
