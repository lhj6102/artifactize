//! A sign-in in progress, of Codex or of Claude Code: started, waited for, given a paste or
//! cancelled from separate commands. The sign-in itself is the library's; this only keeps
//! the running one reachable until it ends.
use crate::Failure;
use crate::ids::SignInId;
use std::marker::PhantomData;
use std::sync::Mutex;
use tokio::sync::oneshot;

pub(super) type SignInTask = tokio::task::JoinHandle<Result<(), String>>;

/// What differs between the sign-ins: the prompt they show and how they are named.
pub(super) trait Flow {
    /// What the sign-in asks the person to do once it has started.
    type Prompt;
    const CANCELLED: &'static str;
    /// No sign-in waits for a paste.
    const NOT_WAITING: &'static str;
    /// The sign-in ended before the paste reached it.
    const ENDED: &'static str;
    /// A second waiter for the same sign-in.
    const WAITED: &'static str;
}

struct SignIn {
    /// Which sign-in this is: its waiter names it, and a waiter of an older one never takes
    /// or clears a newer one.
    id: SignInId,
    /// Delivers a paste to the sign-in.
    paste: Option<oneshot::Sender<String>>,
    /// Stops the sign-in: nothing is saved, and whatever it runs or listens on stops.
    abort: tokio::task::AbortHandle,
    /// The running sign-in, taken by its one waiter.
    task: Option<SignInTask>,
}

/// The one sign-in of a flow in progress. It stays here, reachable for a paste or a cancel,
/// until it ends; only its own waiter clears it.
pub(super) struct SignIns<F> {
    current: Mutex<Option<SignIn>>,
    /// Stopped sign-ins nobody waits for, until they have ended: closing joins them.
    stopped: Mutex<Vec<SignInTask>>,
    started: std::sync::atomic::AtomicU32,
    flow: PhantomData<F>,
}

impl<F> Default for SignIns<F> {
    fn default() -> Self {
        Self {
            current: Mutex::default(),
            stopped: Mutex::default(),
            started: std::sync::atomic::AtomicU32::default(),
            flow: PhantomData,
        }
    }
}

impl<F: Flow> SignIns<F> {
    fn current(&self) -> std::sync::MutexGuard<'_, Option<SignIn>> {
        self.current.lock().expect("sign-in lock")
    }

    /// Stop `sign_in`. Its waiter, if it has one, joins it; otherwise closing does.
    fn halt(&self, sign_in: SignIn) {
        sign_in.abort.abort();
        if let Some(task) = sign_in.task {
            self.stopped.lock().expect("sign-in lock").push(task);
        }
    }

    /// Stop the sign-in in progress, before another starts and needs what it holds.
    pub(super) fn stop(&self) {
        let previous = self.current().take();
        if let Some(previous) = previous {
            self.halt(previous);
        }
    }

    /// Stop every sign-in; the returned future ends once those nobody waits for have ended.
    pub(super) fn close(&self) -> impl std::future::Future<Output = ()> + use<F> {
        self.stop();
        let stopped = std::mem::take(&mut *self.stopped.lock().expect("sign-in lock"));
        async move {
            for task in stopped {
                // Stopped: whether it was cancelled or failed does not matter, only that it has
                // ended; a panic is a bug and goes on here.
                let _ = artifactize_core::task::joined(task.await);
            }
        }
    }

    /// Make `task` the sign-in in progress (stopping any other) and return its id.
    pub(super) fn register(&self, task: SignInTask, paste: oneshot::Sender<String>) -> SignInId {
        let id = SignInId::allocate(&self.started);
        let mut current = self.current();
        if let Some(previous) = current.take() {
            self.halt(previous);
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
    pub(super) async fn start(
        &self,
        launch: impl FnOnce(oneshot::Receiver<String>, oneshot::Sender<F::Prompt>) -> SignInTask,
    ) -> Result<(SignInId, F::Prompt), Failure> {
        // The previous sign-in frees what it holds before this one starts.
        self.stop();
        let (paste, pasted) = oneshot::channel();
        let (shown, prompt) = oneshot::channel();
        let id = self.register(launch(pasted, shown), paste);
        match prompt.await {
            Ok(prompt) if self.is_current(id) => Ok((id, prompt)),
            // Cancelled or replaced while it started: stopped already.
            Ok(_) => Err(Failure::Account(F::CANCELLED.into())),
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
                    Err(error) => Err(Self::ended_badly(error)),
                }
            }
        }
    }

    /// Wait for sign-in `id` to end and forget it.
    pub(super) async fn wait(&self, id: SignInId) -> Result<(), Failure> {
        let task = self.take(id)?;
        let ended = task.await;
        self.finish(id);
        match ended {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(Failure::Account(error)),
            Err(error) => Err(Self::ended_badly(error)),
        }
    }

    /// A sign-in task that did not return: cancelled is reported, a panic is a bug and goes on.
    fn ended_badly(error: tokio::task::JoinError) -> Failure {
        match artifactize_core::task::joined::<()>(Err(error)) {
            Ok(()) => unreachable!("an error stays an error"),
            Err(_) => Failure::Account(F::CANCELLED.into()),
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
    pub(super) fn take(&self, id: SignInId) -> Result<SignInTask, Failure> {
        let mut current = self.current();
        let sign_in = current
            .as_mut()
            .filter(|sign_in| sign_in.id == id)
            .ok_or_else(|| Failure::Account(F::CANCELLED.into()))?;
        sign_in
            .task
            .take()
            .ok_or_else(|| Failure::Missing(F::WAITED.into()))
    }

    /// Sign-in `id` ended: forget it, unless another one replaced it meanwhile.
    pub(super) fn finish(&self, id: SignInId) {
        let mut current = self.current();
        if current.as_ref().is_some_and(|sign_in| sign_in.id == id) {
            *current = None;
        }
    }

    pub(super) fn paste(&self, text: String) -> Result<(), Failure> {
        let paste = self
            .current()
            .as_mut()
            .and_then(|sign_in| sign_in.paste.take())
            .ok_or_else(|| Failure::Missing(F::NOT_WAITING.into()))?;
        paste
            .send(text)
            .map_err(|_| Failure::Missing(F::ENDED.into()))
    }

    pub(super) fn cancel(&self) {
        let mut current = self.current();
        if let Some(sign_in) = current.as_mut() {
            sign_in.abort.abort();
            // No paste can reach a cancelled sign-in.
            sign_in.paste = None;
            // Nobody waits for it: forget it now, and let closing join it.
            if sign_in.task.is_some()
                && let Some(sign_in) = current.take()
            {
                drop(current);
                self.halt(sign_in);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sign-in whose prompt is the URL it shows.
    struct Test;

    impl Flow for Test {
        type Prompt = String;
        const CANCELLED: &'static str = "Sign-in was cancelled.";
        const NOT_WAITING: &'static str = "No sign-in is waiting for a paste.";
        const ENDED: &'static str = "The sign-in already ended.";
        const WAITED: &'static str = "The sign-in is already being waited for.";
    }

    /// A sign-in whose prompt the test releases: `go` shows it.
    fn held() -> (
        oneshot::Sender<()>,
        impl FnOnce(oneshot::Receiver<String>, oneshot::Sender<String>) -> SignInTask,
    ) {
        let (go, gate) = oneshot::channel::<()>();
        let launch = move |pasted: oneshot::Receiver<String>, shown: oneshot::Sender<String>| {
            tokio::spawn(async move {
                if gate.await.is_ok() {
                    let _ = shown.send("http://127.0.0.1:9/oauth/authorize".into());
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
        let sign_ins = SignIns::<Test>::default();
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
            Failure::Account("Sign-in was cancelled.".into())
        );
        assert_eq!(
            sign_ins.paste("http://localhost:1455/".into()).unwrap_err(),
            Failure::Missing("No sign-in is waiting for a paste.".into())
        );
    }

    /// A newer start while an older one waits for its prompt stops the older: it reads as
    /// cancelled and never replaces the newer one.
    #[tokio::test]
    async fn a_newer_start_during_a_start_supersedes_it() {
        let sign_ins = SignIns::<Test>::default();
        let (older_go, older) = held();
        let mut first = Box::pin(sign_ins.start(older));
        assert!(poll_once(&mut first).await.is_none());
        let (newer_go, newer) = held();
        let mut second = Box::pin(sign_ins.start(newer));
        assert!(poll_once(&mut second).await.is_none());
        let _ = older_go.send(());
        assert_eq!(
            first.await.unwrap_err(),
            Failure::Account("Sign-in was cancelled.".into())
        );
        let _ = newer_go.send(());
        let (id, prompt) = second.await.unwrap();
        assert_eq!(prompt, "http://127.0.0.1:9/oauth/authorize");
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
        let sign_ins = SignIns::<Test>::default();
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
            Failure::Account("Sign-in was cancelled.".into())
        );
        sign_ins.finish(newer);
        assert_eq!(
            sign_ins.take(newer).unwrap_err(),
            Failure::Account("Sign-in was cancelled.".into())
        );
    }
}
