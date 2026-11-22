//! Coordinate review actions across the IPC surfaces which share one desktop app.
use std::{collections::BTreeMap, sync::Mutex};

use artifactize_core::types::RequestId;
use tokio_util::sync::CancellationToken;

use crate::{Failure, ids::JobId};

/// Job numbers are local to the WebView that sent them, not part of the JSON payload.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Caller(String);

#[cfg(test)]
impl Caller {
    pub(crate) fn test_main() -> Self {
        Self("main".into())
    }
}

impl<'a, R: tauri::Runtime> tauri::ipc::CommandArg<'a, R> for Caller {
    fn from_command(
        command: tauri::ipc::CommandItem<'a, R>,
    ) -> Result<Self, tauri::ipc::InvokeError> {
        Ok(Self(command.message.webview_ref().label().into()))
    }
}

impl specta::function::FunctionArg for Caller {
    fn to_datatype(_: &mut specta::Types) -> Option<specta::datatype::DataType> {
        None
    }
}

type Key = (String, JobId);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    Ordinary,
    Tool,
    /// Submit or quit after stopping tools on the affected reviews.
    Finish,
}

struct Running {
    ids: Vec<RequestId>,
    action: Action,
    cancel: CancellationToken,
    done: CancellationToken,
}

#[derive(Default)]
pub(super) struct Jobs(Mutex<BTreeMap<Key, Running>>);

impl Jobs {
    pub(super) fn begin(
        &self,
        caller: Caller,
        job: JobId,
        ids: Vec<RequestId>,
        action: Action,
    ) -> Result<Lease<'_>, Failure> {
        let mut lease = self.register(caller, job, action)?;
        lease.admit(ids)?;
        Ok(lease)
    }

    /// Register before the first asynchronous read, so cancellation can reach preparation.
    pub(super) fn register(
        &self,
        caller: Caller,
        job: JobId,
        action: Action,
    ) -> Result<Lease<'_>, Failure> {
        let key = (caller.0, job);
        let mut live = self.0.lock().expect("jobs lock");
        if live.contains_key(&key) {
            return Err(Failure::Action(
                "That job is already running in this window.".into(),
            ));
        }
        let cancel = CancellationToken::new();
        live.insert(
            key.clone(),
            Running {
                ids: Vec::new(),
                action,
                cancel: cancel.clone(),
                done: CancellationToken::new(),
            },
        );
        Ok(Lease {
            jobs: self,
            key,
            cancel,
            tools: Vec::new(),
        })
    }

    pub(super) fn cancel(&self, caller: Caller, job: JobId) {
        if let Some(work) = self.0.lock().expect("jobs lock").get(&(caller.0, job)) {
            work.cancel.cancel();
        }
    }

    pub(super) fn cancel_all(&self) {
        for work in self.0.lock().expect("jobs lock").values() {
            work.cancel.cancel();
        }
    }
}

/// Holds admission until the IPC operation, including tool cleanup, has finished.
pub(crate) struct Lease<'a> {
    jobs: &'a Jobs,
    key: Key,
    pub(crate) cancel: CancellationToken,
    tools: Vec<CancellationToken>,
}

impl Lease<'_> {
    /// After resolving the exact owner, reserve it against the other review surfaces.
    pub(super) fn admit(&mut self, ids: Vec<RequestId>) -> Result<(), Failure> {
        let mut live = self.jobs.0.lock().expect("jobs lock");
        if self.cancel.is_cancelled() {
            return Err(Failure::Action("Review action cancelled.".into()));
        }
        let action = live[&self.key].action;
        let mut tools = Vec::new();
        for (key, work) in &*live {
            if key == &self.key || !work.ids.iter().any(|id| ids.contains(id)) {
                continue;
            }
            if action == Action::Finish && work.action == Action::Tool {
                tools.push((work.cancel.clone(), work.done.clone()));
            } else {
                return Err(Failure::Action(
                    "This review already has work in progress.".into(),
                ));
            }
        }
        live.get_mut(&self.key).expect("live lease").ids = ids;
        // Reserve the submit before cancellation; no other tool can start in the gap.
        self.tools = tools
            .into_iter()
            .map(|(cancel, done)| {
                cancel.cancel();
                done
            })
            .collect();
        Ok(())
    }

    /// A lookup does not hold a job open after its caller cancels it.
    pub(crate) async fn prepare<T>(
        &self,
        read: impl Future<Output = Result<T, Failure>>,
    ) -> Result<T, Failure> {
        tokio::select! {
            biased;
            () = self.cancel.cancelled() => Err(Failure::Action("Review action cancelled.".into())),
            result = read => result,
        }
    }

    pub(super) async fn ready(&self) -> Result<(), Failure> {
        for tool in &self.tools {
            tool.cancelled().await;
        }
        if self.cancel.is_cancelled() {
            return Err(Failure::Action("Review action cancelled.".into()));
        }
        Ok(())
    }
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        let mut live = self.jobs.0.lock().expect("jobs lock");
        if let Some(work) = live.remove(&self.key) {
            work.done.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancelling_an_artifact_tool_while_its_checkout_is_resolved_prevents_execution() {
        let reviews = crate::human::Reviews::default();
        let job = serde_json::from_str("1").unwrap();
        let work = reviews.start_artifact(Caller("main".into()), job).unwrap();
        let mut lookup =
            std::pin::pin!(work.prepare(std::future::pending::<Result<(), Failure>>()));
        assert!(futures_pending(lookup.as_mut()).await);
        reviews.jobs.cancel(Caller("main".into()), job);
        assert_eq!(
            lookup.await,
            Err(Failure::Action("Review action cancelled.".into()))
        );
    }

    async fn futures_pending(mut future: std::pin::Pin<&mut impl Future>) -> bool {
        std::future::poll_fn(|cx| std::task::Poll::Ready(future.as_mut().poll(cx).is_pending()))
            .await
    }

    #[tokio::test]
    async fn cancel_during_owner_lookup_prevents_admission_and_leaves_the_tool_running() {
        let jobs = Jobs::default();
        let id: RequestId = "run-a-1".parse().unwrap();
        let tool_job: JobId = serde_json::from_str("1").unwrap();
        let submit_job: JobId = serde_json::from_str("2").unwrap();
        let tool = jobs
            .begin(
                Caller("detail".into()),
                tool_job,
                vec![id.clone()],
                Action::Tool,
            )
            .unwrap();
        let mut submit = jobs
            .register(Caller("pet".into()), submit_job, Action::Finish)
            .unwrap();
        jobs.cancel(Caller("pet".into()), submit_job);
        assert!(submit.admit(vec![id.clone()]).is_err());
        assert!(!tool.cancel.is_cancelled());
        drop(submit);
        drop(tool);
        let next = jobs
            .begin(Caller("pet".into()), submit_job, vec![id], Action::Finish)
            .unwrap();
        assert!(next.ready().await.is_ok());
    }
}
