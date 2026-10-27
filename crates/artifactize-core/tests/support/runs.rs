//! Read-only fixture barriers inspect saved requests without assigning Run lifetimes.
//! Submission and cleanup cancellation are separate, explicit actions.
use artifactize_core::{
    project::{VerifyOptions, selection::Selection},
    runs, store,
    types::{RequestStatus, RunId},
};
use std::{
    path::Path,
    process::{Command, Output},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

// Included directly by TUI unit tests as well as integration-test support modules.
#[path = "os/worker.rs"]
#[expect(
    clippy::duplicate_mod,
    reason = "standalone Run barriers and OS executable fixtures share one Cargo receipt decoder without duplicating exposed types"
)]
mod fixture_worker;
#[path = "os/observation.rs"]
#[expect(
    clippy::duplicate_mod,
    reason = "this standalone barrier also loads under TUI's crate::test_runs; both consumers share one OS source and expose no duplicated types"
)]
mod process_observation;

/// Portable fixture observation bound, not a verification lifetime or timing assertion.
const ADMISSION_BOUND: Duration = Duration::from_secs(30);

/// A terminal receipt can precede the worker's final SQLite/notification cleanup. Waiting
/// here owns no execution: the executor and supervisor's saved birth identities are observed,
/// never the launcher, and a still-running Human/timeout result retains its real Run lifetime.
pub async fn terminal_worker_exited(
    view: &store::RunView,
    changes: &mut artifactize_core::changes::Subscription,
    cancellation: &CancellationToken,
) -> Result<(), String> {
    if view.run.status().is_open() {
        return Ok(());
    }
    if let Some(control) = &view.run.control {
        let owner = match control.worker {
            runs::Worker::Admitted { owner } => Some(owner),
            _ => None,
        };
        // The supervisor checks the executor after its exit, retaining its own state handles.
        for owner in owner.into_iter().chain(control.supervisor) {
            while process_observation::same_process_running(owner.pid.get(), owner.start_time) {
                tokio::select! {
                    _ = changes.next_reconciled() => {},
                    _ = cancellation.cancelled() => return Err("Fixture observation cancelled.".into()),
                }
            }
        }
    }
    Ok(())
}

async fn admitted(
    state: &Path,
    id: &RunId,
    cancellation: CancellationToken,
) -> Result<store::RunView, String> {
    let mut changes = artifactize_core::changes::Subscription::new(state).await;
    let result = tokio::time::timeout(ADMISSION_BOUND, async {
        loop {
            // Run status and requests come from one read-only transaction. The observer
            // must not reconcile a dead owner or finish/cancel the Run it is waiting for.
            let view = store::read_run(state, id).await?;
            let waiting = view.run.control.as_ref().map_or_else(
                || {
                    !view
                        .requests
                        .iter()
                        .any(|request| request.status() == RequestStatus::Running)
                },
                |control| {
                    !control.cancellation_requested && control.activity == runs::Activity::Waiting
                },
            );
            if !view.run.status().is_open()
                || (waiting
                    && view
                        .requests
                        .iter()
                        .any(|request| request.status().is_human_wait()))
            {
                terminal_worker_exited(&view, &mut changes, &cancellation).await?;
                return Ok::<_, String>(view);
            }
            tokio::select! {
                _ = changes.next_state() => {},
                _ = cancellation.cancelled() => return Err("Fixture observation cancelled.".into()),
            }
        }
    })
    .await
    .map_err(|_| "Fixture admission barrier expired.".to_owned())
    .and_then(|result| result);
    changes.close().await?;
    result
}
/// Submit a fixture Run, then observe its saved admission state without reconciling it.
pub async fn record(
    repo: &Path,
    state: Option<&Path>,
    selection: &Selection,
    options: &VerifyOptions,
    cancellation: CancellationToken,
) -> Result<store::RunView, String> {
    let context = runs::Context::new(repo, state, fixture_worker::executable()?)?;
    let observation = runs::start(&context, selection, options, None).await?;
    admitted(&store::state_dir(state)?, &observation.run, cancellation).await
}

/// Own every Run submitted to one fixture repository, including its temporary input/state.
/// Successful teardown cancels active Runs, observes saved terminal state and waits for the
/// admitted workers' birth identities to exit before deleting the directory. Failed cleanup
/// retains the directory so a surviving worker never loses its inputs during test unwinding.
pub struct RepoLease {
    root: Option<tempfile::TempDir>,
    state: std::path::PathBuf,
    repo: std::path::PathBuf,
}

impl RepoLease {
    pub fn new(root: tempfile::TempDir, state: &Path, repo: &Path) -> Self {
        Self {
            root: Some(root),
            state: state.to_owned(),
            repo: repo.to_owned(),
        }
    }

    pub fn path(&self) -> &Path {
        self.root.as_ref().expect("fixture root is owned").path()
    }

    /// Explicit teardown before destructive database replacement: cancel owned Runs and
    /// observe saved terminal state plus worker exit, while retaining the fixture root.
    /// This does not disarm the lease; Drop also releases any subsequently submitted Runs.
    pub fn release(&self) -> Result<(), String> {
        if !self.state.join(store::DATABASE).is_file() {
            return Ok(());
        }
        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|error| error.to_string())?;
                    runtime.block_on(async {
                        let context = runs::Context::new(
                            &self.repo,
                            Some(&self.state),
                            fixture_worker::executable()?,
                        )?;
                        // This bounds query allocation only, not the number of owned Runs.
                        const PAGE_SIZE: u32 = 128;
                        let mut offset = 0;
                        loop {
                            let page =
                                store::read_runs(&self.state, Some(&self.repo), PAGE_SIZE, offset)
                                    .await?;
                            let count = page.len();
                            for run in page {
                                release_run(&context, &self.state, &run.id).await?;
                            }
                            if count < PAGE_SIZE as usize {
                                return Ok::<_, String>(());
                            }
                            offset += PAGE_SIZE;
                        }
                    })
                })
                .join()
        })
        .unwrap_or_else(|_| Err("Fixture Run cleanup panicked.".into()))
    }
}

impl Drop for RepoLease {
    fn drop(&mut self) {
        if let Err(error) = self.release() {
            let retained = self.root.take().expect("fixture root is owned").keep();
            let message = format!(
                "Fixture Run cleanup failed; inputs retained at {}: {error}",
                fixture_worker::path_text(&retained)
            );
            if std::thread::panicking() {
                // Preserve the original test panic, but report the cleanup failure too.
                eprintln!("{message}");
            } else {
                panic!("{message}");
            }
        } else if let Err(error) = {
            // The state's IPC namespace lives in the OS temp root, outside the fixture root;
            // name it while the state still exists, then remove both.
            let namespace = artifactize_core::changes::namespace(&self.state);
            let closed = self.root.take().expect("fixture root is owned").close();
            closed.and(namespace.and_then(std::fs::remove_dir_all))
        } {
            let message = format!("Fixture directory cleanup failed: {error}");
            if std::thread::panicking() {
                eprintln!("{message}");
            } else {
                panic!("{message}");
            }
        }
    }
}

async fn release_run(context: &runs::Context, state: &Path, id: &RunId) -> Result<(), String> {
    // A fixture watchdog bounds failed cleanup, never a verification execution lifetime.
    let mut changes = artifactize_core::changes::Subscription::new(state).await;
    let result = tokio::time::timeout(ADMISSION_BOUND, async {
        if store::read_run(state, id).await?.run.status().is_open() {
            // This is an explicit cleanup action, not part of an observation barrier.
            runs::cancel(context, id).await?;
        }
        loop {
            let view = store::read_run(state, id).await?;
            if !view.run.status().is_open() {
                terminal_worker_exited(&view, &mut changes, &CancellationToken::new()).await?;
                return Ok::<_, String>(());
            }
            changes.next_reconciled().await;
        }
    })
    .await
    .map_err(|_| "Fixture Run cleanup expired.".to_owned())
    .and_then(|result| result);
    changes.close().await?;
    result
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
        let verifies = args.iter().any(|arg| arg == "verify");
        let is_submission = verifies && !args.iter().any(|arg| arg == "--wait");
        let mut output = fixture_worker::output(self)?;
        if !verifies {
            return Ok(output);
        }
        let accepted: serde_json::Value = match serde_json::from_slice(&output.stdout) {
            Ok(value) => value,
            Err(_) => return Ok(output),
        };
        let submission_id = accepted["run"].as_str();
        let Some(id) = submission_id.or_else(|| accepted["id"].as_str()) else {
            return Ok(output);
        };
        // Rewrite successful submission JSON only. --wait and RED/ERROR outputs retain
        // their original bytes and exit status; their saved terminal worker still exits.
        let rewrite = is_submission && output.status.success() && submission_id.is_some();
        let id: RunId = id.parse().map_err(std::io::Error::other)?;
        // A terminal Run reports the state chosen by the child, including its default
        // environment and working directory. Never resolve defaults in this observer's
        // different environment. Submission-only JSON instead uses the declared argument.
        let state = accepted["stateDir"]
            .as_str()
            .map(std::path::PathBuf::from)
            .or_else(|| {
                args.windows(2)
                    .find(|args| args[0] == "--state-dir")
                    .map(|args| std::path::PathBuf::from(&args[1]))
            })
            .ok_or_else(|| {
                std::io::Error::other("Fixture observation needs stateDir or --state-dir.")
            })?;
        let view = std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|e| e.to_string())?;
                    runtime.block_on(async {
                        if rewrite {
                            admitted(&state, &id, CancellationToken::new())
                                .await
                                .map(Some)
                        } else {
                            let mut changes =
                                artifactize_core::changes::Subscription::new(&state).await;
                            let result = tokio::time::timeout(ADMISSION_BOUND, async {
                                let view = store::read_run(&state, &id).await?;
                                terminal_worker_exited(
                                    &view,
                                    &mut changes,
                                    &CancellationToken::new(),
                                )
                                .await
                            })
                            .await
                            .map_err(|_| "Fixture worker exit barrier expired.".to_owned())
                            .and_then(|result| result);
                            changes.close().await?;
                            result?;
                            Ok(None)
                        }
                    })
                })
                .join()
                .unwrap()
        })
        .map_err(std::io::Error::other)?;
        if let Some(view) = view {
            output.stdout = serde_json::to_vec(&artifactize_core::query::run_output(
                &view,
                time::OffsetDateTime::now_utc(),
            ))?;
        }
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
                        .find(|run| run.status.is_open())
                        .expect("fixture has an active Run");
                    let context = runs::Context::new(
                        repo,
                        Some(state),
                        fixture_worker::executable().unwrap(),
                    )
                    .unwrap();
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
                    let view = store::read_run(state, id).await.unwrap();
                    assert_eq!(
                        view.run.repo_path,
                        artifactize_core::workspace::canonical_target(repo).unwrap(),
                        "fixture Run belongs to this repository"
                    );
                    assert!(!view.run.status().is_open(), "fixture Run must be terminal");
                    let control = view.run.control.unwrap();
                    let runs::Worker::Admitted { owner } = control.worker else {
                        panic!("fixture worker was admitted");
                    };
                    let result = tokio::time::timeout(ADMISSION_BOUND, async {
                        for owner in std::iter::once(owner).chain(control.supervisor) {
                            while running(owner.pid.get())
                                && process_observation::same_process_running(
                                    owner.pid.get(),
                                    owner.start_time,
                                )
                            {
                                // Exit itself need not write SQLite. This subscription wait
                                // supplies only an invalidation or a liveness-observation timeout;
                                // it never runs the mutating Run reconciliation APIs.
                                changes.next_reconciled().await;
                            }
                        }
                    })
                    .await;
                    changes.close().await.unwrap();
                    result.expect("terminal fixture worker exits before the byte baseline");
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
                        .find(|run| run.status.is_open())
                        .expect("active Run");
                    let mut changes = artifactize_core::changes::Subscription::new(state).await;
                    let result = tokio::time::timeout(ADMISSION_BOUND, async {
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
                    .await;
                    changes.close().await.unwrap();
                    result.unwrap()
                })
            })
            .join()
            .unwrap()
    })
}
