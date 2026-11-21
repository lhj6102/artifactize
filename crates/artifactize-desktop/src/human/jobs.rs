//! Coordinate review actions across the IPC surfaces which share one desktop app.
use std::{collections::BTreeMap, sync::Mutex};

use artifactize_core::types::RequestId;
use tokio_util::sync::CancellationToken;

use crate::{Failure, ids::JobId};

/// Job numbers are local to the WebView that sent them, not part of the JSON payload.
pub(crate) struct Caller(String);

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
    Submit,
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
        let key = (caller.0, job);
        let mut live = self.0.lock().expect("jobs lock");
        if live.contains_key(&key) {
            return Err(Failure::Action(
                "That job is already running in this window.".into(),
            ));
        }
        let mut tools = Vec::new();
        for work in live
            .values()
            .filter(|work| work.ids.iter().any(|id| ids.contains(id)))
        {
            if action == Action::Submit && work.action == Action::Tool {
                tools.push((work.cancel.clone(), work.done.clone()));
            } else {
                return Err(Failure::Action(
                    "This review already has work in progress.".into(),
                ));
            }
        }
        let cancel = CancellationToken::new();
        live.insert(
            key.clone(),
            Running {
                ids,
                action,
                cancel: cancel.clone(),
                done: CancellationToken::new(),
            },
        );
        // Reserve the submit so no other window can start a tool between cancellation and the
        // write. Do not cancel anything if admission itself failed.
        for (cancel, _) in &tools {
            cancel.cancel();
        }
        Ok(Lease {
            jobs: self,
            key,
            cancel,
            tools: tools.into_iter().map(|(_, done)| done).collect(),
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
