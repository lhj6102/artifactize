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
    /// Open while its sign-offs are claimed or its idle grace runs.
    WaitingHuman,
    Cancelling,
    Green,
    Red,
    Incomplete,
    Yellow,
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
                | Self::Yellow
                | Self::Error
                | Self::Cancelled
        )
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
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
    #[serde(serialize_with = "seconds")]
    pub elapsed: Duration,
    #[serde(serialize_with = "seconds")]
    pub next_check: Duration,
}

fn seconds<S: serde::Serializer>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_u64(duration.as_secs())
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
pub(super) fn state_for(
    status: RunStatus,
    control: Option<&super::Control>,
    counts: &Counts,
) -> State {
    match status {
        RunStatus::Green => State::Green,
        RunStatus::Red => State::Red,
        RunStatus::Incomplete => State::Incomplete,
        RunStatus::Yellow => State::Yellow,
        RunStatus::Error
            if control
                .is_some_and(|control| control.failure_code == Some(FailureCode::Cancelled)) =>
        {
            State::Cancelled
        }
        RunStatus::Error => State::Error,
        RunStatus::WaitingHuman if control.is_some_and(super::Control::is_cancelling) => {
            State::Cancelling
        }
        RunStatus::WaitingHuman => State::WaitingHuman,
        RunStatus::Running => {
            if let Some(control) = control {
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
pub(super) fn state(run: &Run, requests: &[std::sync::Arc<Request>]) -> State {
    state_for(
        run.status(),
        run.control.as_ref(),
        &counts(requests.iter().map(|request| (request.status(), 1))),
    )
}

pub async fn view(context: &Context, id: &RunId) -> Result<RunView, String> {
    let view = store::read_run(&context.state, id).await?;
    if !crate::platform::paths_equal(&view.run.repo_path, &context.repo) {
        return Err("Run not found in this repository.".into());
    }
    super::reconcile(&context.state, view).await
}
pub async fn status(context: &Context, id: &RunId) -> Result<RunObservation, String> {
    const NEXT_CHECK: Duration = Duration::from_secs(30); // One ordinary bounded observation, not a polling cadence.
    let mut saved = store::run_control::observe(&context.state, id).await?;
    if !crate::platform::paths_equal(&saved.repo, &context.repo) {
        return Err("Run not found in this repository.".into());
    }
    if super::owner_died(saved.status, saved.control.as_ref())? {
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
            .since(saved.created_at),
        next_check: if state.is_terminal() {
            Duration::ZERO
        } else {
            NEXT_CHECK
        },
    })
}

/// Reconcile a known open Run if its owning process died without committing a result.
/// The cheap observation does not decode the saved graph unless recovery is needed.
pub async fn reconcile_interrupted(state: &std::path::Path, id: &RunId) -> Result<bool, String> {
    let saved = store::run_control::observe(state, id).await?;
    if !super::owner_died(saved.status, saved.control.as_ref())? {
        return Ok(false);
    }
    let settled = super::reconcile(state, store::read_run(state, id).await?).await?;
    Ok(!settled.run.status().is_open())
}

/// The optional bound covers registration and all reads. At its deadline return the last
/// authoritative snapshot; cancellation disposes only this observer, never the Run.
pub async fn wait(
    context: &Context,
    id: &RunId,
    bound: Option<Duration>,
    cancellation: CancellationToken,
) -> Result<RunObservation, String> {
    wait_observing(context, id, bound, cancellation, |_| {}).await
}

/// Like [`wait`], with each successfully read snapshot available to a caller while it waits.
pub async fn wait_observing(
    context: &Context,
    id: &RunId,
    bound: Option<Duration>,
    cancellation: CancellationToken,
    mut observe: impl FnMut(&RunObservation),
) -> Result<RunObservation, String> {
    if bound.is_some_and(|bound| bound.is_zero()) {
        let snapshot = status(context, id).await?;
        observe(&snapshot);
        return Ok(snapshot);
    }
    let deadline = bound
        .map(|bound| {
            tokio::time::Instant::now()
                .checked_add(bound)
                .ok_or("Run observation bound is too large.")
        })
        .transpose()?;
    let mut last = None;
    let mut subscription = None;
    let result = {
        let observing = async {
            let changes =
                subscription.insert(crate::changes::Subscription::new(&context.state).await);
            loop {
                let current = status(context, id).await?;
                let ended = current.state.is_terminal();
                observe(&current);
                last = Some(current);
                if ended {
                    return Ok::<(), String>(());
                }
                changes.next_reconciled().await;
            }
        };
        tokio::pin!(observing);
        tokio::select! {
            result = &mut observing => result,
            _ = cancellation.cancelled() => Err("Run observation was cancelled.".into()),
            _ = async { if let Some(deadline) = deadline { tokio::time::sleep_until(deadline).await; } else { std::future::pending::<()>().await; } } => Ok(()),
        }
    };
    // Completion, deadline and cancellation all release this observer before returning.
    // Otherwise CLI runtime shutdown can interrupt the detached watch's SQLite cleanup.
    let closed = match subscription {
        Some(changes) => changes.close().await,
        None => Ok(()),
    };
    match (result, closed) {
        (Ok(()), Ok(())) => {
            last.ok_or_else(|| "State unavailable within the observation bound.".into())
        }
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(error), Err(close)) => Err(format!(
            "{error}; closing observation subscription: {close}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[tokio::test]
    async fn an_unrepresentable_wait_bound_is_an_input_error() {
        let scratch = crate::test_os::tempdir();
        let context = Context::new(scratch.path(), Some(scratch.path()), Path::new("")).unwrap();
        let error = wait(
            &context,
            &"run-missing".parse().unwrap(),
            Some(Duration::MAX),
            CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(error, "Run observation bound is too large.");
    }

    #[test]
    fn core_durations_keep_the_existing_integer_second_wire_values() {
        let observed = RunObservation {
            run: "run-wire".parse().unwrap(),
            state: State::Running,
            counts: Counts::default(),
            elapsed: Duration::from_millis(1250),
            next_check: Duration::from_secs(30),
        };
        let wire = serde_json::to_value(observed).unwrap();
        assert_eq!(wire["elapsed"], 1);
        assert_eq!(wire["next_check"], 30);
    }
}
