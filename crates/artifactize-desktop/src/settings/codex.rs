//! The Codex sign-in: its state, the sign-in in progress and its commands.
use crate::ids::SignInId;
use crate::{App, Failure};
use artifactize_core::auth::codex;
use serde::Serialize;
use specta::Type;
use std::sync::Mutex;
use tokio::sync::oneshot;

type SignInTask = tokio::task::JoinHandle<Result<(), String>>;

struct SignIn {
    /// Which sign-in this is: its waiter names it, and a waiter of an older one never takes
    /// or clears a newer one.
    id: SignInId,
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
pub(super) struct SignIns {
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
    fn register(&self, task: SignInTask, paste: oneshot::Sender<String>) -> SignInId {
        let id = SignInId::allocate(&self.started);
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

    /// Start a sign-in with `launch` (given where pastes arrive and where it shows its
    /// prompt) and return its id and prompt. It is the sign-in in progress from the moment it
    /// is launched, before its prompt: a cancel or a newer start meanwhile stops it, and it
    /// then reads as cancelled and is never registered afterwards.
    async fn start(
        &self,
        launch: impl FnOnce(oneshot::Receiver<String>, oneshot::Sender<codex::Prompt>) -> SignInTask,
    ) -> Result<(SignInId, codex::Prompt), Failure> {
        // The previous sign-in frees its port before this one listens.
        self.stop();
        let (paste, pasted) = oneshot::channel();
        let (shown, prompt) = oneshot::channel();
        let id = self.register(launch(pasted, shown), paste);
        match prompt.await {
            Ok(prompt) if self.is_current(id) => Ok((id, prompt)),
            // Cancelled or replaced while it started: stopped already.
            Ok(_) => Err(Failure::Account(CANCELLED.into())),
            // It ended before showing anything: cancelled, or why it failed.
            Err(_) => {
                let task = self.take(id)?;
                let ended = task.await;
                self.finish(id);
                match ended {
                    Ok(Err(error)) => Err(Failure::Account(error)),
                    // The library shows the prompt before it can succeed: a success without
                    // one is a bug in this sign-in's own wiring.
                    Ok(Ok(())) => unreachable!("a sign-in succeeded without showing its prompt"),
                    Err(error) => Err(ended_badly(error)),
                }
            }
        }
    }

    /// Sign-in `id` is still the one in progress.
    fn is_current(&self, id: SignInId) -> bool {
        self.current()
            .as_ref()
            .is_some_and(|sign_in| sign_in.id == id)
    }

    /// The task of sign-in `id` for its waiter. A sign-in another one replaced, or that was
    /// cancelled, reads as cancelled.
    fn take(&self, id: SignInId) -> Result<SignInTask, Failure> {
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
    fn finish(&self, id: SignInId) {
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
        .sign_ins
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
    let task = app.settings.sign_ins.take(sign_in)?;
    let ended = task.await;
    app.settings.sign_ins.finish(sign_in);
    match ended {
        Ok(Ok(())) => read_codex(&app),
        Ok(Err(error)) => Err(Failure::Account(error)),
        Err(error) => Err(ended_badly(error)),
    }
}

/// A sign-in task that did not return: cancelled is reported, a panic is a bug and goes on.
fn ended_badly(error: tokio::task::JoinError) -> Failure {
    match artifactize_core::task::joined::<()>(Err(error)) {
        Ok(()) => unreachable!("an error stays an error"),
        Err(_) => Failure::Account(CANCELLED.into()),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A sign-in whose prompt the test releases: `go` shows it.
    fn held() -> (
        oneshot::Sender<()>,
        impl FnOnce(oneshot::Receiver<String>, oneshot::Sender<codex::Prompt>) -> SignInTask,
    ) {
        let (go, gate) = oneshot::channel::<()>();
        let launch = move |pasted: oneshot::Receiver<String>,
                           shown: oneshot::Sender<codex::Prompt>| {
            tokio::spawn(async move {
                if gate.await.is_ok() {
                    let _ = shown.send(codex::Prompt {
                        url: "http://127.0.0.1:9/oauth/authorize".into(),
                        callback: true,
                    });
                }
                // The fixture completes when its public paste boundary receives a value.
                pasted.await.map(|_| ()).map_err(|_| "paste closed".into())
            })
        };
        (go, launch)
    }

    /// Poll `future` once: a start runs up to its first wait, its prompt.
    async fn poll_once<F: std::future::Future + Unpin>(future: &mut F) -> Option<F::Output> {
        std::future::poll_fn(|context| {
            std::task::Poll::Ready(match std::pin::Pin::new(&mut *future).poll(context) {
                std::task::Poll::Ready(output) => Some(output),
                std::task::Poll::Pending => None,
            })
        })
        .await
    }

    /// A cancel while a start waits for its prompt stops it: the start reads as cancelled,
    /// nothing is registered, and a paste finds no sign-in.
    #[tokio::test]
    async fn a_cancel_during_a_start_leaves_no_sign_in() {
        let sign_ins = SignIns::default();
        let (go, launch) = held();
        let mut start = Box::pin(sign_ins.start(launch));
        assert!(
            poll_once(&mut start).await.is_none(),
            "waiting for its prompt"
        );
        sign_ins.cancel();
        // The prompt would come now; the stopped sign-in never shows it.
        let _ = go.send(());
        assert_eq!(
            start.await.unwrap_err(),
            Failure::Account("Codex sign-in was cancelled.".into())
        );
        assert_eq!(
            sign_ins.paste("http://localhost:1455/".into()).unwrap_err(),
            Failure::Missing("No Codex sign-in is waiting for a URL.".into())
        );
    }

    /// A newer start while an older one waits for its prompt stops the older: it reads as
    /// cancelled and never replaces the newer one.
    #[tokio::test]
    async fn a_newer_start_during_a_start_supersedes_it() {
        let sign_ins = SignIns::default();
        let (older_go, older) = held();
        let mut first = Box::pin(sign_ins.start(older));
        assert!(poll_once(&mut first).await.is_none());
        let (newer_go, newer) = held();
        let mut second = Box::pin(sign_ins.start(newer));
        assert!(poll_once(&mut second).await.is_none());
        let _ = older_go.send(());
        assert_eq!(
            first.await.unwrap_err(),
            Failure::Account("Codex sign-in was cancelled.".into())
        );
        let _ = newer_go.send(());
        let (id, prompt) = second.await.unwrap();
        assert_eq!(prompt.url, "http://127.0.0.1:9/oauth/authorize");
        assert!(prompt.callback);
        sign_ins
            .paste("http://localhost:1455/auth/callback?code=c&state=s".into())
            .unwrap();
        assert_eq!(sign_ins.take(id).unwrap().await.unwrap(), Ok(()));
        sign_ins.finish(id);
    }

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
        assert!(task.await.unwrap_err().is_cancelled());
        // A late waiter of the older one reads it as cancelled and takes nothing.
        assert_eq!(
            sign_ins.take(older).unwrap_err(),
            Failure::Account("Codex sign-in was cancelled.".into())
        );
        sign_ins.finish(newer);
        assert_eq!(
            sign_ins.take(newer).unwrap_err(),
            Failure::Account("Codex sign-in was cancelled.".into())
        );
    }
}
