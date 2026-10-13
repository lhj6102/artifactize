//! Artifactize-owned verification: durable submission, observation and explicit cancellation.
mod observe;
mod result;
mod worker;
use crate::{
    process,
    project::{VerifyOptions, selection::Selection},
    store::{self, Receipts},
    types::{FailureCode, RequestKey, RunId, RunStatus},
};
pub use observe::{Counts, RunObservation, State, status, view, wait};
pub use result::{Finding, ResultCursor, RunResultPage, RunVerdict, result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
pub(crate) use worker::{execute, supervise};

/// Per-Run execution modes. Future modes extend this type, not transport signatures.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RunModes {
    pub speculate: bool,
    pub defer_human: bool,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
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
#[serde(rename_all = "camelCase")]
pub struct Control {
    pub worker: Worker,
    pub activity: Activity,
    #[serde(default)]
    pub cancellation_requested: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_code: Option<FailureCode>,
}
impl Control {
    pub(crate) fn starting() -> Result<Self, String> {
        Ok(Self {
            worker: Worker::Starting {
                launcher: process::child_identity(std::process::id()).map_err(|e| e.to_string())?,
            },
            activity: Activity::Preparing,
            cancellation_requested: false,
            failure_code: None,
        })
    }
    pub(crate) fn legacy() -> Self {
        Self {
            worker: Worker::Legacy,
            activity: Activity::Executing,
            cancellation_requested: false,
            failure_code: None,
        }
    }
}

/// Bound repository/state and trusted executable, resolved once by the interface at startup.
#[derive(Debug, Clone)]
pub struct Context {
    pub(crate) repo: PathBuf,
    pub(crate) state: PathBuf,
    executable: PathBuf,
}
impl Context {
    pub fn new(repo: &Path, state: Option<&Path>, executable: &Path) -> Result<Self, String> {
        Ok(Self {
            repo: crate::workspace::canonical_target(repo).map_err(|e| e.to_string())?,
            state: crate::workspace::canonical_target(&store::state_dir(state)?)
                .map_err(|e| e.to_string())?,
            executable: executable.to_path_buf(),
        })
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
    // Keys are attempt identity, not checkout identity. Retry before discovering declarations.
    if let Some(key) = &request_key
        && context.state.join(store::DATABASE).is_file()
    {
        let receipts = Receipts::open(&context.state, &context.repo).await?;
        if let Some(id) = receipts.keyed_run(&context.repo, key).await? {
            return status(context, &id).await;
        }
    }
    let (id, accepted) = crate::project::verify::reserve(
        &context.repo,
        Some(&context.state),
        selection,
        options,
        request_key.as_ref(),
    )
    .await?;
    if accepted {
        let receipts = Receipts::open(&context.state, &context.repo).await?;
        let launch = process::launch_run(&context.executable, &id, &context.repo, &context.state);
        let admitted = match launch {
            Ok(identity) => receipts.admit(&id, identity).await,
            Err(error) => Err(error.to_string()),
        };
        if let Err(error) = admitted {
            worker::fail(&receipts, &id, FailureCode::SpawnFailed, error.clone()).await?;
            return Err(format!("Run {id} handoff failed: {error}"));
        }
    }
    status(context, &id).await
}

#[derive(Debug, Clone, Serialize)]
pub struct CancelAcknowledgement {
    pub run: RunId,
    pub state: State,
    pub cancellation_requested: bool,
}
pub async fn cancel(context: &Context, id: &RunId) -> Result<CancelAcknowledgement, String> {
    let current = view(context, id).await?;
    let receipts = Receipts::open(&context.state, &context.repo).await?;
    let run = receipts.request_cancel(&current.run.id).await?;
    Ok(CancelAcknowledgement {
        run: id.clone(),
        state: observe::state(&run, &current.requests),
        cancellation_requested: run
            .control
            .as_ref()
            .is_some_and(|control| control.cancellation_requested),
    })
}

pub(crate) async fn reconcile(
    state: &Path,
    view: store::RunView,
) -> Result<store::RunView, String> {
    if view.run.status() == RunStatus::Running
        && let Some(control) = &view.run.control
    {
        let owner = match control.worker {
            Worker::Starting { launcher } => Some(launcher),
            Worker::Admitted { owner } => Some(owner),
            Worker::Legacy => None,
        };
        if owner.is_some() && !process::is_alive(owner).map_err(|e| e.to_string())? {
            worker::recover(state, &view).await?;
            return store::read_run(state, &view.run.id).await;
        }
    }
    Ok(view)
}
