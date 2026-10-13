//! One subscribed observer, independent of execution and Eval kind.
use super::{Activity, Context};
use crate::{
    store::{self, Request, Run, RunView},
    types::{FailureCode, RequestStatus, RunId, RunStatus, Timestamp},
};
use serde::Serialize;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Preparing,
    Running,
    Waiting,
    Cancelling,
    Green,
    Red,
    Incomplete,
    SignoffsPending,
    Error,
    Cancelled,
}
impl State {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Green
                | Self::Red
                | Self::Incomplete
                | Self::SignoffsPending
                | Self::Error
                | Self::Cancelled
        )
    }
}
#[derive(Debug, Clone, Default, Serialize)]
pub struct Counts {
    pub total: u64,
    pub completed: u64,
    pub running: u64,
    pub waiting: u64,
    pub pending: u64,
}
#[derive(Debug, Clone, Serialize)]
pub struct RunObservation {
    pub run: RunId,
    pub state: State,
    pub counts: Counts,
    pub elapsed: u64,
    pub next_check: u32,
}

fn counts(states: impl IntoIterator<Item = (RequestStatus, u64)>) -> Counts {
    states
        .into_iter()
        .fold(Counts::default(), |mut counts, (state, count)| {
            counts.total += count;
            match state {
                RequestStatus::Green
                | RequestStatus::Provisional
                | RequestStatus::Red
                | RequestStatus::Error => counts.completed += count,
                RequestStatus::Running => counts.running += count,
                RequestStatus::WaitingHuman
                | RequestStatus::Deferred
                | RequestStatus::WaitDependency => counts.waiting += count,
                _ => counts.pending += count,
            }
            counts
        })
}
fn state_for(status: RunStatus, control: Option<&super::Control>, counts: &Counts) -> State {
    match status {
        RunStatus::Green => State::Green,
        RunStatus::Red => State::Red,
        RunStatus::Incomplete => State::Incomplete,
        RunStatus::SignoffsPending => State::SignoffsPending,
        RunStatus::Error
            if control
                .is_some_and(|control| control.failure_code == Some(FailureCode::Cancelled)) =>
        {
            State::Cancelled
        }
        RunStatus::Error => State::Error,
        RunStatus::Running => {
            if let Some(control) = control {
                if control.cancellation_requested {
                    return State::Cancelling;
                }
                return match control.activity {
                    Activity::Preparing => State::Preparing,
                    Activity::Executing | Activity::Publishing => State::Running,
                    Activity::Waiting => State::Waiting,
                    Activity::Cancelling => State::Cancelling,
                };
            }
            if counts.waiting > 0 && counts.running == 0 {
                State::Waiting
            } else {
                State::Running
            }
        }
    }
}
pub(super) fn state(run: &Run, requests: &[Request]) -> State {
    state_for(
        run.status(),
        run.control.as_ref(),
        &counts(requests.iter().map(|request| (request.status(), 1))),
    )
}

pub async fn view(context: &Context, id: &RunId) -> Result<RunView, String> {
    let view = store::read_run(&context.state, id).await?;
    if view.run.repo_path != context.repo {
        return Err("Run not found in this repository.".into());
    }
    super::reconcile(&context.state, view).await
}
pub async fn status(context: &Context, id: &RunId) -> Result<RunObservation, String> {
    const NEXT_CHECK_SECONDS: u32 = 30; // One ordinary bounded observation, not a polling cadence.
    let mut saved = store::run_control::observe(&context.state, id).await?;
    if saved.repo != context.repo {
        return Err("Run not found in this repository.".into());
    }
    let owner = saved
        .control
        .as_ref()
        .and_then(|control| match control.worker {
            super::Worker::Starting { launcher } => Some(launcher),
            super::Worker::Admitted { owner } => Some(owner),
            super::Worker::Legacy => None,
        });
    if saved.status == RunStatus::Running
        && owner.is_some()
        && !crate::process::is_alive(owner).map_err(|e| e.to_string())?
    {
        view(context, id).await?;
        saved = store::run_control::observe(&context.state, id).await?;
    }
    let counts = counts(saved.counts);
    let state = state_for(saved.status, saved.control.as_ref(), &counts);
    Ok(RunObservation {
        run: id.clone(),
        state,
        counts,
        elapsed: saved
            .completed_at
            .unwrap_or_else(Timestamp::now)
            .since(saved.created_at)
            .as_secs(),
        next_check: if state.is_terminal() {
            0
        } else {
            NEXT_CHECK_SECONDS
        },
    })
}

/// The optional bound covers registration and all reads. At its deadline return the last
/// authoritative snapshot; cancellation disposes only this observer, never the Run.
pub async fn wait(
    context: &Context,
    id: &RunId,
    bound: Option<Duration>,
    cancellation: CancellationToken,
) -> Result<RunObservation, String> {
    if bound.is_some_and(|bound| bound.is_zero()) {
        return status(context, id).await;
    }
    let deadline = bound.map(|bound| tokio::time::Instant::now() + bound);
    let mut last = None;
    {
        let observing = async {
            let mut changes = crate::changes::Subscription::new(&context.state).await;
            loop {
                let current = status(context, id).await?;
                let ended = current.state.is_terminal();
                last = Some(current);
                if ended {
                    return Ok::<(), String>(());
                }
                changes.next_reconciled().await;
            }
        };
        tokio::pin!(observing);
        tokio::select! {
            result = &mut observing => { result?; }
            _ = cancellation.cancelled() => return Err("Run observation was cancelled.".into()),
            _ = async { if let Some(deadline) = deadline { tokio::time::sleep_until(deadline).await; } else { std::future::pending::<()>().await; } } => {}
        }
    }
    last.ok_or_else(|| "State unavailable within the observation bound.".into())
}
