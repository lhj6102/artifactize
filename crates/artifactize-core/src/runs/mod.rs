//! Artifactize-owned verification: durable submission, observation and explicit cancellation.
#[cfg(test)]
mod lifetime_tests;
mod observe;
mod result;
mod watch;
mod worker;
use crate::{
    process,
    project::{VerifyOptions, selection::Selection},
    store::{self, Receipts},
    types::{FailureCode, RequestKey, RunId},
};
pub use observe::{
    Counts, RunObservation, State, reconcile_interrupted, status, view, wait, wait_observing,
};
pub use result::{
    Finding, PAGE_SIZE as RESULT_PAGE_SIZE, ResultCursor, ResultState, RunResultPage, RunVerdict,
    result,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
pub use watch::{RunEvent, watch};
#[doc(hidden)]
pub use worker::{execute, supervise, supervise_observing_launch};

/// Per-Run execution modes. Future modes extend this type, not transport signatures.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RunModes {
    pub speculate: bool,
}
impl RunModes {
    pub(crate) fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    Preparing,
    Executing,
    Publishing,
    Waiting,
    Cancelling,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Worker {
    Starting {
        launcher: process::ChildIdentity,
    },
    Admitted {
        owner: process::ChildIdentity,
    },
    /// Old records have no worker identity and are not evidence of owner death.
    Legacy,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "ControlWire", into = "ControlWire")]
pub struct Control {
    pub worker: Worker,
    /// The supervisor keeps worker files open until its executor has exited and been checked.
    pub supervisor: Option<process::ChildIdentity>,
    pub activity: Activity,
    pub failure_code: Option<FailureCode>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ControlWire {
    worker: Worker,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    supervisor: Option<process::ChildIdentity>,
    activity: Activity,
    #[serde(default)]
    cancellation_requested: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    failure_code: Option<FailureCode>,
}

impl From<ControlWire> for Control {
    fn from(wire: ControlWire) -> Self {
        Self {
            worker: wire.worker,
            supervisor: wire.supervisor,
            activity: if wire.cancellation_requested {
                Activity::Cancelling
            } else {
                wire.activity
            },
            failure_code: wire.failure_code,
        }
    }
}

impl From<Control> for ControlWire {
    fn from(control: Control) -> Self {
        Self {
            worker: control.worker,
            supervisor: control.supervisor,
            activity: control.activity,
            cancellation_requested: control.is_cancelling(),
            failure_code: control.failure_code,
        }
    }
}

impl Control {
    pub fn is_cancelling(&self) -> bool {
        self.activity == Activity::Cancelling
    }

    pub(crate) fn starting() -> Result<Self, String> {
        Ok(Self {
            worker: Worker::Starting {
                launcher: process::child_identity(std::process::id()).map_err(|e| e.to_string())?,
            },
            supervisor: None,
            activity: Activity::Preparing,
            failure_code: None,
        })
    }
    pub(crate) fn legacy() -> Self {
        Self {
            worker: Worker::Legacy,
            supervisor: None,
            activity: Activity::Executing,
            failure_code: None,
        }
    }
}

/// The surface that accepted a new Run. Old receipts have no known origin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Origin {
    Cli,
    Mcp,
    Chat {
        session_id: crate::types::SessionId,
        title: String,
    },
}

/// Bound repository/state and trusted executable, resolved once by the interface at startup.
#[derive(Debug, Clone)]
pub struct Context {
    pub(crate) repo: PathBuf,
    pub(crate) state: PathBuf,
    executable: PathBuf,
    origin: Origin,
}
impl Context {
    /// The canonical repository this endpoint serves.
    pub fn repo(&self) -> &Path {
        &self.repo
    }

    pub(crate) fn executable(&self) -> &Path {
        &self.executable
    }

    /// The canonical state directory shared by this endpoint and its Run workers.
    pub fn state(&self) -> &Path {
        &self.state
    }

    pub fn new(repo: &Path, state: Option<&Path>, executable: &Path) -> Result<Self, String> {
        Ok(Self {
            repo: crate::workspace::canonical_target(repo).map_err(|e| e.to_string())?,
            state: crate::workspace::canonical_target(&store::state_dir(state)?)
                .map_err(|e| e.to_string())?,
            executable: executable.to_path_buf(),
            origin: Origin::Cli,
        })
    }
    /// Set the origin for new Runs started in this context, without changing read scope.
    #[must_use]
    pub fn with_origin(self, origin: Origin) -> Self {
        Self { origin, ..self }
    }

    /// Public-library convenience; packaged callers normally pass their own executable to `new`.
    pub fn discover(repo: &Path, state: Option<&Path>) -> Result<Self, String> {
        let executable = process::artifactize_executable()?;
        Self::new(repo, state, &executable)
    }
}

pub async fn start(
    context: &Context,
    selection: &Selection,
    options: &VerifyOptions,
    request_key: Option<RequestKey>,
) -> Result<RunObservation, String> {
    start_observing_launch(context, selection, options, request_key, |_, _| {}).await
}

/// Transfer the supervisor's birth identity at launch, before saving or reading any receipt.
#[doc(hidden)]
pub async fn start_observing_launch(
    context: &Context,
    selection: &Selection,
    options: &VerifyOptions,
    request_key: Option<RequestKey>,
    launched: impl FnOnce(&RunId, process::ChildIdentity),
) -> Result<RunObservation, String> {
    // Keys are attempt identity, not checkout identity. Retry before discovering declarations.
    if let Some(key) = &request_key
        && crate::platform::paths::target_is_file(&context.state.join(store::DATABASE))
    {
        let receipts = Receipts::open(&context.state, &context.repo).await?;
        let result = receipts.keyed_run(&context.repo, key).await;
        let closed = receipts.close().await;
        if let Some(id) = result.and_then(|id| closed.map(|()| id))? {
            return status(context, &id).await;
        }
    }
    let (id, accepted) = crate::project::verify::reserve(
        &context.repo,
        Some(&context.state),
        selection,
        options,
        request_key.as_ref(),
        &context.origin,
    )
    .await?;
    if accepted {
        let receipts = Receipts::open(&context.state, &context.repo).await?;
        let result = async {
            let launch =
                process::launch_run(&context.executable, &id, &context.repo, &context.state);
            let admitted = match launch {
                Ok(identity) => {
                    launched(&id, identity);
                    receipts.admit(&id, identity).await
                }
                Err(error) => Err(error.to_string()),
            };
            if let Err(error) = admitted {
                worker::fail(&receipts, &id, FailureCode::SpawnFailed, error.clone()).await?;
                return Err(format!("Run {id} handoff failed: {error}"));
            }
            Ok(())
        }
        .await;
        let closed = receipts.close().await;
        result.and(closed)?;
    }
    // Preparing the Run read the whole workspace and built its definitions, all freed by now;
    // a caller that goes on to wait for the Run should not keep that memory while it waits.
    crate::platform::release_free_memory();
    status(context, &id).await
}

#[derive(Debug, Clone, Serialize)]
pub struct CancelAcknowledgement {
    pub run: RunId,
    pub state: State,
    pub cancellation_requested: bool,
}
pub async fn cancel(context: &Context, id: &RunId) -> Result<CancelAcknowledgement, String> {
    // The compact observation checks the repository and settles a Run whose worker died.
    let current = status(context, id).await?;
    let receipts = Receipts::open(&context.state, &context.repo).await?;
    let result = receipts.request_cancel(id).await;
    let closed = receipts.close().await;
    let (saved, control) = result.and_then(|saved| closed.map(|()| saved))?;
    Ok(CancelAcknowledgement {
        run: id.clone(),
        state: observe::state_for(saved, control.as_ref(), &current.counts),
        cancellation_requested: control.as_ref().is_some_and(Control::is_cancelling),
    })
}

fn owner_died(status: crate::types::RunStatus, control: Option<&Control>) -> Result<bool, String> {
    if !status.is_open() {
        return Ok(false);
    }
    let owner = control.and_then(|control| match control.worker {
        Worker::Starting { launcher } => Some(launcher),
        Worker::Admitted { owner } => Some(owner),
        Worker::Legacy => None,
    });
    Ok(owner.is_some() && !process::is_alive(owner).map_err(|error| error.to_string())?)
}

pub(crate) async fn reconcile(
    state: &Path,
    view: store::RunView,
) -> Result<store::RunView, String> {
    if owner_died(view.run.status(), view.run.control.as_ref())? {
        worker::recover(state, &view).await?;
        return store::read_run(state, &view.run.id).await;
    }
    Ok(view)
}
