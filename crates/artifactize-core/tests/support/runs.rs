//! Fixture admission barriers: inspect saved waiting requests without assigning Run lifetimes.
use artifactize_core::{
    project::{VerifyOptions, selection::Selection},
    runs, store,
};
use std::{
    path::Path,
    process::{Command, Output},
    time::Duration,
};
use tokio_util::sync::CancellationToken;
/// Portable fixture observation bound, not a verification lifetime or timing assertion.
const ADMISSION_BOUND: Duration = Duration::from_secs(30);

async fn admitted(
    context: &runs::Context,
    state: &Path,
    id: &artifactize_core::types::RunId,
    cancellation: CancellationToken,
) -> Result<store::RunView, String> {
    let mut changes = artifactize_core::changes::Subscription::new(state).await;
    tokio::time::timeout(ADMISSION_BOUND, async {
        loop {
            let observed = runs::status(context, id).await?;
            let view = runs::view(context, id).await?;
            if observed.state.is_terminal() || (observed.state == runs::State::Waiting && view.requests.iter().any(|request| request.status() == artifactize_core::types::RequestStatus::WaitingHuman)) { return Ok::<_, String>(view); }
            tokio::select! { _ = changes.next_state() => {}, _ = cancellation.cancelled() => return Err("Fixture observation cancelled.".into()) }
        }
    }).await.map_err(|_| "Fixture admission barrier expired.".to_owned())?
}
pub async fn record(
    repo: &Path,
    state: Option<&Path>,
    selection: &Selection,
    options: &VerifyOptions,
    cancellation: CancellationToken,
) -> Result<store::RunView, String> {
    let context = runs::Context::discover(repo, state)?;
    let observation = runs::start(&context, selection, options, None).await?;
    admitted(
        &context,
        &store::state_dir(state)?,
        &observation.run,
        cancellation,
    )
    .await
}

/// Existing Human fixtures need a saved request, not a validation result. Submission stdout
/// is inspected first, then a subscribed barrier yields the authoritative saved Run JSON.
pub trait RecordOutput {
    fn record_output(&mut self) -> std::io::Result<Output>;
}
impl RecordOutput for Command {
    fn record_output(&mut self) -> std::io::Result<Output> {
        let args = self
            .get_args()
            .map(|arg| arg.to_os_string())
            .collect::<Vec<_>>();
        let is_submission =
            args.iter().any(|arg| arg == "verify") && !args.iter().any(|arg| arg == "--wait");
        let mut output = self.output()?;
        if !is_submission || !output.status.success() {
            return Ok(output);
        }
        let accepted: serde_json::Value = match serde_json::from_slice(&output.stdout) {
            Ok(value) => value,
            Err(_) => return Ok(output),
        };
        let Some(id) = accepted["run"].as_str() else {
            return Ok(output);
        };
        let id: artifactize_core::types::RunId = id.parse().map_err(std::io::Error::other)?;
        let state = args
            .windows(2)
            .find(|args| args[0] == "--state-dir")
            .map(|args| std::path::PathBuf::from(&args[1]))
            .expect("fixture state is explicit");
        let view = std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|e| e.to_string())?;
                    runtime.block_on(async {
                        let initial = store::read_run(&state, &id).await?;
                        let context =
                            runs::Context::discover(&initial.run.repo_path, Some(&state))?;
                        admitted(&context, &state, &id, CancellationToken::new()).await
                    })
                })
                .join()
                .unwrap()
        })
        .map_err(std::io::Error::other)?;
        output.stdout = serde_json::to_vec(&artifactize_core::query::run_output(
            &view,
            time::OffsetDateTime::now_utc(),
        ))?;
        Ok(output)
    }
}

/// Tests of cleanup cancel the Run explicitly rather than assigning execution to a watcher.
pub fn cancel_active(state: &Path, repo: &Path) -> artifactize_core::types::RunId {
    std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                runtime.block_on(async {
                    let active = store::read_runs(state, Some(repo), 100, 0)
                        .await
                        .unwrap()
                        .into_iter()
                        .find(|run| run.status == artifactize_core::types::RunStatus::Running)
                        .expect("fixture has an active Run");
                    let context = runs::Context::discover(repo, Some(state)).unwrap();
                    runs::cancel(&context, &active.id).await.unwrap();
                    active.id
                })
            })
            .join()
            .unwrap()
    })
}

/// A terminal Run's worker can still close its last SQLite connection and checkpoint.
/// Byte-preservation fixtures start their baseline only after that exact owner exits.
pub fn finished_worker(
    state: &Path,
    repo: &Path,
    id: &artifactize_core::types::RunId,
    running: fn(u32) -> bool,
) {
    std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                runtime.block_on(async {
                    let mut changes = artifactize_core::changes::Subscription::new(state).await;
                    let context = runs::Context::discover(repo, Some(state)).unwrap();
                    let observed = runs::status(&context, id).await.unwrap();
                    assert!(observed.state.is_terminal(), "fixture Run must be terminal");
                    let view = runs::view(&context, id).await.unwrap();
                    let runs::Worker::Admitted { owner } = view.run.control.unwrap().worker else {
                        panic!("fixture worker was admitted");
                    };
                    tokio::time::timeout(ADMISSION_BOUND, async {
                        while running(owner.pid.get()) {
                            // Exit itself need not write SQLite; use the subscription's
                            // existing owner-liveness reconciliation, not a fixture sleep.
                            changes.next_reconciled().await;
                        }
                    })
                    .await
                    .expect("terminal fixture worker exits before the byte baseline");
                });
            })
            .join()
            .unwrap();
    });
}

/// Saved executor identity, not the process watching it.
pub fn worker_pid(state: &Path, repo: &Path) -> u32 {
    std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                runtime.block_on(async {
                    let active = store::read_runs(state, Some(repo), 100, 0)
                        .await
                        .unwrap()
                        .into_iter()
                        .find(|run| run.status == artifactize_core::types::RunStatus::Running)
                        .expect("active Run");
                    let mut changes = artifactize_core::changes::Subscription::new(state).await;
                    tokio::time::timeout(ADMISSION_BOUND, async {
                        loop {
                            let view = store::read_run(state, &active.id).await.unwrap();
                            let control = view.run.control.unwrap();
                            if control.activity == runs::Activity::Waiting
                                || view.requests.iter().any(|request| {
                                    request.status()
                                        == artifactize_core::types::RequestStatus::Running
                                })
                            {
                                let runs::Worker::Admitted { owner } = control.worker else {
                                    panic!("admitted worker");
                                };
                                break owner.pid.get();
                            }
                            changes.next_state().await;
                        }
                    })
                    .await
                    .unwrap()
                })
            })
            .join()
            .unwrap()
    })
}
