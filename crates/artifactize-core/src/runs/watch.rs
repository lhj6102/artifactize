//! Shared change-driven Run stream. Notifications are hints; saved state is authority.
use super::{Context, Finding, RunObservation, RunResultPage};
use crate::types::{RequestId, RequestStatus, RunId};
use serde::Serialize;
use std::{collections::BTreeMap, future::Future, time::Duration};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum RunEvent {
    /// A live observation is never a final validation verdict.
    Update(RunObservation),
    /// A saved eval result, including reuse. Provisional GREEN is not final approval.
    EvalResult(Finding),
    /// The Run's final validation; findings may continue through next_cursor.
    Outcome(RunResultPage),
}

/// Replay a baseline, then consume shared state invalidations, not caller polling.
/// Reattachment replays saved completed evals. Within this attachment, each saved
/// result status is emitted once. Cancellation/deadline drops only the observer.
pub async fn watch<F, R>(
    context: &Context,
    id: &RunId,
    bound: Option<Duration>,
    cancellation: CancellationToken,
    mut emit: F,
) -> Result<RunObservation, String>
where
    F: FnMut(RunEvent) -> R,
    R: Future<Output = Result<(), String>>,
{
    let mut subscription = None;
    let mut last = None;
    let observing = async {
        let changes = subscription.insert(crate::changes::Subscription::new(&context.state).await);
        let mut seen: BTreeMap<RequestId, RequestStatus> = BTreeMap::new();
        let mut previous = None;
        loop {
            let view = super::view(context, id).await?;
            let current = super::status(context, id).await?;
            // Ignore elapsed time: reconciliation must not invent progress.
            let signature = (current.state, current.counts.clone());
            if previous.as_ref() != Some(&signature) {
                emit(RunEvent::Update(current.clone())).await?;
                previous = Some(signature);
            }
            for request in &view.requests {
                if (request.result().is_some() || request.error().is_some())
                    && seen.get(&request.id) != Some(&request.status())
                {
                    emit(RunEvent::EvalResult(super::result::finding(request))).await?;
                    seen.insert(request.id.clone(), request.status());
                }
            }
            let ended = super::observe::state(&view.run, &view.requests).is_terminal();
            last = Some(current);
            if ended {
                emit(RunEvent::Outcome(super::result(context, id, None).await?)).await?;
                return Ok::<(), String>(());
            }
            changes.next_reconciled().await;
        }
    };
    let result = {
        tokio::pin!(observing);
        tokio::select! {
            result = &mut observing => result,
            _ = cancellation.cancelled() => Err("Run observation was cancelled.".into()),
            _ = async { match bound { Some(bound) => tokio::time::sleep(bound).await, None => std::future::pending().await } } => Ok(()),
        }
    };
    let closed = match subscription {
        Some(subscription) => subscription.close().await,
        None => Ok(()),
    };
    result?;
    closed?;
    last.ok_or_else(|| "State unavailable within the observation bound.".into())
}
