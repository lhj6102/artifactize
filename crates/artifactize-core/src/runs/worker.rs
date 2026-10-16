//! Internal admission gate, execution and identity-checked dead-worker settlement.
use super::{Context, Worker};
use crate::{
    process,
    store::{self, Receipts, RunView},
    types::{FailureCode, RequestStatus, RunId, RunStatus},
};
use tokio_util::sync::CancellationToken;
/// Bound only the internal handoff, not a verification's lifetime.
const ADMISSION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

async fn admission(context: &Context, id: &RunId) -> Result<store::Run, String> {
    let identity = process::child_identity(std::process::id()).map_err(|e| e.to_string())?;
    let mut changes = crate::changes::Subscription::new(&context.state).await;
    let admitted = async {
        loop {
            let run = store::read_run(&context.state, id).await?.run;
            if run.status() != RunStatus::Running {
                return Err("Run already settled before admission.".into());
            }
            let worker = run.control.as_ref().map(|control| control.worker);
            match worker {
                Some(Worker::Admitted { owner }) if owner == identity => {
                    return Ok::<_, String>(run);
                }
                Some(Worker::Starting { launcher })
                | Some(Worker::Admitted { owner: launcher }) => {
                    if process::is_alive(Some(launcher)).map_err(|e| e.to_string())? {
                        changes.next_reconciled().await;
                    } else {
                        // The submitter may commit admission and exit while this saved
                        // graph is decoded. A dead old identity is not a rejected handoff:
                        // compare fresh, compact SQLite control before refusing it.
                        let current = store::run_control::observe(&context.state, id).await?;
                        if current.status == RunStatus::Running
                            && current.control.as_ref().map(|control| control.worker) == worker
                        {
                            return Err("Run worker identity was not admitted.".into());
                        }
                    }
                }
                _ => return Err("Run worker identity was not admitted.".into()),
            }
        }
    };
    let result = tokio::time::timeout(ADMISSION_TIMEOUT, admitted)
        .await
        .map_err(|_| "Run admission expired.".to_owned())
        .and_then(std::convert::identity);
    let closed = changes.close().await;
    match (result, closed) {
        (Ok(run), Ok(())) => Ok(run),
        (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(close)) => {
            Err(format!("{error}; closing admission subscription: {close}"))
        }
    }
}

/// A separate artifactize supervisor settles an executor crash even with no observers.
pub(crate) async fn supervise(context: &Context, id: &RunId) -> Result<(), String> {
    admission(context, id).await?;
    let receipts = Receipts::open(&context.state, &context.repo).await?;
    let result = supervise_open(context, id, &receipts).await;
    let closed = receipts.close().await;
    result.and(closed)
}

async fn supervise_open(context: &Context, id: &RunId, receipts: &Receipts) -> Result<(), String> {
    let supervisor = process::child_identity(std::process::id()).map_err(|e| e.to_string())?;
    let (owner, mut child) = match process::spawn_run(
        &context.executable,
        id,
        &context.repo,
        &context.state,
        process::RunEntry::Executor,
    ) {
        Ok(child) => child,
        Err(error) => {
            return fail(receipts, id, FailureCode::SpawnFailed, error.to_string()).await;
        }
    };
    let handed = receipts
        .control(id, move |run| {
            let control = run.control.as_mut().ok_or("Run has no worker control.")?;
            if !matches!(control.worker, Worker::Admitted { owner } if owner == supervisor) {
                return Err("Run supervision identity changed.".into());
            }
            control.worker = Worker::Admitted { owner };
            Ok(())
        })
        .await;
    if let Err(error) = handed {
        fail(receipts, id, FailureCode::SpawnFailed, error).await?;
    }
    child.wait().await.map_err(|e| e.to_string())?;
    let view = store::read_run(&context.state, id).await?;
    if view.run.status() == RunStatus::Running {
        recover(&context.state, &view).await?;
    }
    Ok(())
}

pub(crate) async fn execute(context: &Context, id: &RunId) -> Result<(), String> {
    let run = admission(context, id).await?;
    let receipts = Receipts::open(&context.state, &context.repo).await?;
    let mut changes = crate::changes::Subscription::new(&context.state).await;
    let result = execute_admitted(context, id, run, &mut changes, &receipts).await;
    let closed = changes.close().await;
    let result = match (result, closed) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(error), Err(close)) => Err(format!("{error}; closing worker subscription: {close}")),
    };
    let closed = receipts.close().await;
    result.and(closed)
}

async fn execute_admitted(
    context: &Context,
    id: &RunId,
    run: store::Run,
    changes: &mut crate::changes::Subscription,
    receipts: &Receipts,
) -> Result<(), String> {
    let cancellation = CancellationToken::new();
    if run
        .control
        .as_ref()
        .is_some_and(|control| control.cancellation_requested)
    {
        cancellation.cancel();
    }
    let observing = async {
        loop {
            let view = store::read_run(&context.state, id).await?;
            if view
                .run
                .control
                .as_ref()
                .is_some_and(|control| control.cancellation_requested)
            {
                cancellation.cancel();
                return Ok::<(), String>(());
            }
            changes.next_reconciled().await;
        }
    };
    tokio::pin!(observing);
    let execution = crate::project::verify::execute(run, receipts, cancellation.clone());
    tokio::pin!(execution);
    let result = tokio::select! {
        result = &mut execution => result,
        result = &mut observing => {
            if let Err(error) = result { cancellation.cancel(); let _ = execution.await; return fail(receipts, id, FailureCode::PreparationFailed, error).await; }
            execution.await
        }
    };
    match result {
        Ok(_) => Ok(()),
        Err(error) => {
            let code = if cancellation.is_cancelled() {
                FailureCode::Cancelled
            } else {
                error.code()
            };
            fail(receipts, id, code, error.to_string()).await
        }
    }
}

pub(super) async fn fail(
    receipts: &Receipts,
    id: &RunId,
    code: FailureCode,
    error: String,
) -> Result<(), String> {
    receipts.fail_run(id, code, error).await
}

pub(super) async fn recover(state: &std::path::Path, view: &RunView) -> Result<(), String> {
    if let Some(super::Control {
        worker: Worker::Admitted { owner },
        ..
    }) = &view.run.control
    {
        crate::platform::kill_saved_session(*owner).map_err(|e| e.to_string())?;
    }
    // Joined requests never confer ownership of another Run's child.
    for request in &view.requests {
        if !request.joined
            && let Some(child) = &request.child
        {
            crate::platform::kill_saved_group(process::ChildIdentity {
                pid: child.pid,
                start_time: child.start_time,
            })
            .map_err(|e| e.to_string())?;
        }
    }
    crate::tools::sessions::recover(state).map_err(|e| e.to_string())?;
    let receipts = Receipts::open(state, &view.run.repo_path).await?;
    let result = async {
        receipts
            .retire_worker(&view.run.id, FailureCode::OwnerDied)
            .await?;
        let mut run = view.run.clone();
        run.state = store::RunState::Error {
            at: crate::broker::now(),
            error: Some("Run worker died; interrupted work is not replayed.".into()),
        };
        if let Some(control) = &mut run.control {
            control.failure_code = Some(FailureCode::OwnerDied);
        }
        let requests: Vec<_> = view
            .requests
            .iter()
            .map(|view| {
                let mut request = view.clone();
                if !matches!(
                    request.status(),
                    RequestStatus::Green | RequestStatus::Red | RequestStatus::Error
                ) {
                    request.state = store::RequestState::failed(
                        "Run worker died.".into(),
                        Some(FailureCode::OwnerDied),
                        crate::broker::now(),
                    );
                }
                request
            })
            .collect();
        receipts.finish(&run, &requests).await
    }
    .await;
    let closed = receipts.close().await;
    result.and(closed)
}
