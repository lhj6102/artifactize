//! Settings: the accounts, keys and stores the CLI configures, through the same library calls
//! (`login`/`logout codex`, `models`, `remote login|logout|status|push`). Nothing here stores,
//! validates or reaches a service on its own; secrets never come back to the frontend.
use crate::{App, Failure};
use artifactize::{
    auth::{codex, remote},
    config::Backend,
    llm,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::Mutex;
use time::OffsetDateTime;
use tokio::sync::oneshot;

/// The Codex sign-in in progress, if any.
#[derive(Default)]
pub struct Settings {
    sign_ins: SignIns,
}

type SignInTask = tokio::task::JoinHandle<Result<(), String>>;

struct SignIn {
    /// Which sign-in this is: its waiter names it, and a waiter of an older one never takes
    /// or clears a newer one.
    id: u32,
    /// Delivers a pasted redirect URL to the sign-in.
    paste: Option<oneshot::Sender<String>>,
    /// Stops the sign-in: its listener closes, the port is freed and nothing is saved.
    abort: tokio::task::AbortHandle,
    /// The running sign-in, taken by its one waiter.
    task: Option<SignInTask>,
}

/// The one sign-in in progress. It stays here, reachable for a paste or a cancel, until it
/// ends; only its own waiter clears it.
#[derive(Default)]
struct SignIns {
    current: Mutex<Option<SignIn>>,
    started: std::sync::atomic::AtomicU32,
}

const CANCELLED: &str = "Codex sign-in was cancelled.";

impl SignIns {
    fn current(&self) -> std::sync::MutexGuard<'_, Option<SignIn>> {
        self.current.lock().expect("sign-in lock")
    }

    /// Stop the sign-in in progress, before another starts and needs its port.
    fn stop(&self) {
        if let Some(previous) = self.current().take() {
            previous.abort.abort();
        }
    }

    /// Make `task` the sign-in in progress (stopping any other) and return its id.
    fn register(&self, task: SignInTask, paste: oneshot::Sender<String>) -> u32 {
        let id = self
            .started
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut current = self.current();
        if let Some(previous) = current.take() {
            previous.abort.abort();
        }
        *current = Some(SignIn {
            id,
            paste: Some(paste),
            abort: task.abort_handle(),
            task: Some(task),
        });
        id
    }

    /// The task of sign-in `id` for its waiter. A sign-in another one replaced, or that was
    /// cancelled, reads as cancelled.
    fn take(&self, id: u32) -> Result<SignInTask, Failure> {
        let mut current = self.current();
        let sign_in = current
            .as_mut()
            .filter(|sign_in| sign_in.id == id)
            .ok_or_else(|| Failure::Account(CANCELLED.into()))?;
        sign_in.task.take().ok_or_else(|| {
            Failure::Missing("The Codex sign-in is already being waited for.".into())
        })
    }

    /// Sign-in `id` ended: forget it, unless another one replaced it meanwhile.
    fn finish(&self, id: u32) {
        let mut current = self.current();
        if current.as_ref().is_some_and(|sign_in| sign_in.id == id) {
            *current = None;
        }
    }

    fn paste(&self, redirect: String) -> Result<(), Failure> {
        let paste = self
            .current()
            .as_mut()
            .and_then(|sign_in| sign_in.paste.take())
            .ok_or_else(|| Failure::Missing("No Codex sign-in is waiting for a URL.".into()))?;
        paste
            .send(redirect)
            .map_err(|_| Failure::Missing("The Codex sign-in already ended.".into()))
    }

    fn cancel(&self) {
        let mut current = self.current();
        if let Some(sign_in) = current.as_mut() {
            sign_in.abort.abort();
            // No paste can reach a cancelled sign-in.
            sign_in.paste = None;
            // Nobody waits for it: forget it now.
            if sign_in.task.is_some() {
                *current = None;
            }
        }
    }
}

/// A backend, as the CLI names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Provider {
    Openai,
    Anthropic,
    Codex,
}

impl From<Provider> for Backend {
    fn from(provider: Provider) -> Self {
        match provider {
            Provider::Openai => Backend::Openai,
            Provider::Anthropic => Backend::Anthropic,
            Provider::Codex => Backend::Codex,
        }
    }
}

/// Whether an API-key backend's key is in the environment; never its value.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct KeyStatus {
    pub provider: Provider,
    /// The environment variable the CLI reads.
    pub variable: String,
    pub present: bool,
}

/// The OpenAI and Anthropic keys, as `doctor` reports them.
#[tauri::command]
#[specta::specta]
pub fn settings_keys() -> Vec<KeyStatus> {
    [Provider::Openai, Provider::Anthropic]
        .into_iter()
        .map(|provider| {
            let backend = provider.into();
            KeyStatus {
                provider,
                variable: llm::key_variable(backend)
                    .expect("an API-key backend")
                    .into(),
                present: llm::key_present(backend),
            }
        })
        .collect()
}

/// How Codex is signed in, read offline as the CLI does.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum CodexStatus {
    /// Not signed in.
    Absent,
    /// artifactize's own sign-in; `expired` once its access token ran out (it refreshes on
    /// use unless the refresh token was revoked).
    #[serde(rename_all = "camelCase")]
    Stored { expires_at: String, expired: bool },
    /// A read-only Codex auth file from ARTIFACTIZE_CODEX_AUTH_FILE; never written here.
    #[serde(rename_all = "camelCase")]
    File {
        path: String,
        expires_at: Option<String>,
        expired: bool,
    },
    /// Tokens cannot be stored where this state lives, and why.
    Refused { reason: String },
}

fn timestamp(time: codex::Timestamp) -> String {
    let seconds = i64::try_from(time.seconds()).unwrap_or(i64::MAX);
    OffsetDateTime::from_unix_timestamp(seconds)
        .ok()
        .and_then(|time| {
            time.format(&time::format_description::well_known::Rfc3339)
                .ok()
        })
        .unwrap_or_default()
}

fn read_codex(app: &App) -> Result<CodexStatus, Failure> {
    let status = codex::status(Some(&app.launch.state), app.launch.repo.as_deref())
        .map_err(Failure::Account)?;
    Ok(match status {
        codex::Status::Absent => CodexStatus::Absent,
        codex::Status::Refused { reason } => CodexStatus::Refused { reason },
        codex::Status::Stored { expiry } => match expiry {
            codex::StoredExpiry::Usable { expires_at } => CodexStatus::Stored {
                expires_at: timestamp(expires_at),
                expired: false,
            },
            codex::StoredExpiry::Expired { expires_at } => CodexStatus::Stored {
                expires_at: timestamp(expires_at),
                expired: true,
            },
        },
        codex::Status::File { path, expiry } => CodexStatus::File {
            path: artifactize_tools::files::path_text(&path),
            expires_at: match expiry {
                codex::FileExpiry::Usable { expires_at } => expires_at.map(timestamp),
                codex::FileExpiry::Expired => None,
            },
            expired: matches!(expiry, codex::FileExpiry::Expired),
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
    pub sign_in: u32,
}

/// Start `login codex`: the sign-in URL to show (the system browser gets it too when
/// `browser`), and whether the browser can return here. Finish with [`codex_login_wait`].
#[tauri::command]
#[specta::specta]
pub async fn codex_login_start(
    app: tauri::State<'_, App>,
    browser: bool,
) -> Result<CodexPrompt, Failure> {
    app.settings.sign_ins.stop();
    let (paste, pasted) = oneshot::channel();
    let (shown, prompt) = oneshot::channel();
    let state = app.launch.state.clone();
    let repo = app.launch.repo.clone();
    let mut task = tokio::spawn(async move {
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
    });
    let prompt = match prompt.await {
        Ok(prompt) => prompt,
        // It ended before showing anything: why.
        Err(_) => {
            let ended = (&mut task)
                .await
                .map_err(|error| Failure::Account(error.to_string()))?;
            return Err(Failure::Account(ended.err().unwrap_or_else(|| {
                "Codex sign-in ended before it started.".into()
            })));
        }
    };
    let sign_in = app.settings.sign_ins.register(task, paste);
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
    sign_in: u32,
) -> Result<CodexStatus, Failure> {
    let task = app.settings.sign_ins.take(sign_in)?;
    let ended = task.await;
    app.settings.sign_ins.finish(sign_in);
    match ended {
        Ok(Ok(())) => read_codex(&app),
        Ok(Err(error)) => Err(Failure::Account(error)),
        Err(error) if error.is_cancelled() => Err(Failure::Account(CANCELLED.into())),
        Err(error) => Err(Failure::Account(error.to_string())),
    }
}

/// Finish the sign-in with the URL the browser was sent to, as the CLI accepts on stdin.
#[tauri::command]
#[specta::specta]
pub fn codex_login_paste(app: tauri::State<'_, App>, redirect: String) -> Result<(), Failure> {
    app.settings.sign_ins.paste(redirect)
}

/// Stop the sign-in in progress; nothing is saved, even if the browser returns later.
#[tauri::command]
#[specta::specta]
pub fn codex_login_cancel(app: tauri::State<'_, App>) {
    app.settings.sign_ins.cancel();
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

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub slug: String,
    pub display_name: String,
}

/// `models <provider>`: what the backend offers to this key or account.
#[tauri::command]
#[specta::specta]
pub async fn models_list(
    app: tauri::State<'_, App>,
    provider: Provider,
) -> Result<Vec<Model>, Failure> {
    let listing = llm::models::list(
        provider.into(),
        Some(&app.launch.state),
        app.launch.repo.as_deref(),
    )
    .await
    .map_err(Failure::Account)?;
    Ok(listing
        .models
        .into_iter()
        .map(|model| Model {
            slug: model.slug.to_string(),
            display_name: model.display_name,
        })
        .collect())
}

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
#[serde(rename_all = "camelCase")]
pub struct RemoteStatus {
    pub configured: bool,
    pub url: Option<String>,
    pub share: Option<Share>,
    pub token_source: Option<TokenSource>,
    pub reachable: bool,
    pub principal: Option<String>,
    pub scopes: Vec<String>,
    pub error: Option<String>,
}

impl From<Option<remote::Status>> for RemoteStatus {
    fn from(status: Option<remote::Status>) -> Self {
        let Some(status) = status else {
            return Self {
                configured: false,
                url: None,
                share: None,
                token_source: None,
                reachable: false,
                principal: None,
                scopes: Vec::new(),
                error: None,
            };
        };
        Self {
            configured: true,
            url: Some(status.url),
            share: Some(status.share.into()),
            token_source: Some(match status.token_source {
                remote::TokenSource::Env => TokenSource::Env,
                remote::TokenSource::File => TokenSource::File,
                remote::TokenSource::None => TokenSource::None,
            }),
            reachable: status.reachable,
            principal: status.principal.map(|principal| principal.to_string()),
            scopes: status
                .scopes
                .into_iter()
                .flatten()
                .map(|scope| scope.to_string())
                .collect(),
            error: status.error,
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
    remote::parse_url(&url).map_err(Failure::Invalid)?;
    remote::login(
        Some(&app.launch.state),
        app.launch.repo.as_deref(),
        &url,
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
    let report =
        artifactize::remote::push(Some(&app.launch.state), app.launch.repo.as_deref(), dry_run)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A sign-in that never ends by itself.
    fn pending() -> (SignInTask, oneshot::Sender<String>) {
        let (paste, pasted) = oneshot::channel::<String>();
        let task = tokio::spawn(async move {
            let _ = pasted.await;
            std::future::pending::<()>().await;
            Ok(())
        });
        (task, paste)
    }

    /// The older waiter holds its task when a newer sign-in registers, and ends only then:
    /// it reads as cancelled, and the newer sign-in stays for its own waiter and paste.
    #[tokio::test]
    async fn an_older_waiter_ending_after_a_newer_start_leaves_it_alone() {
        let sign_ins = SignIns::default();
        let (task, paste) = pending();
        let older = sign_ins.register(task, paste);
        let held = sign_ins.take(older).unwrap();
        // A newer start: the older one is stopped, the newer registered, in that order.
        sign_ins.stop();
        let (task, paste) = pending();
        let newer = sign_ins.register(task, paste);
        // Only now does the older sign-in end, and its waiter finish.
        let ended = held.await;
        assert!(ended.unwrap_err().is_cancelled());
        sign_ins.finish(older);
        // The newer one is untouched: a paste reaches it and its waiter takes it.
        sign_ins
            .paste("http://localhost:1455/auth/callback?code=c&state=s".into())
            .unwrap();
        let task = sign_ins.take(newer).unwrap();
        task.abort();
        // A late waiter of the older one reads it as cancelled and takes nothing.
        assert_eq!(
            sign_ins.take(older).unwrap_err(),
            Failure::Account(CANCELLED.into())
        );
        sign_ins.finish(newer);
        assert!(sign_ins.current().is_none());
    }
}
