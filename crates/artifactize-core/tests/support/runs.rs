//! Read-only fixture barriers inspect saved requests without assigning Run lifetimes.
//! Submission and cleanup cancellation are separate, explicit actions.
use artifactize_core::{
    project::{VerifyOptions, selection::Selection},
    runs, store,
    types::{RequestStatus, RunId},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::{Arc, Mutex, Weak},
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

/// Join the subscription before letting an observation panic unwind into its state owner.
pub async fn with_subscription<T>(
    state: &Path,
    observe: impl AsyncFnOnce(&mut artifactize_core::changes::Subscription) -> T,
) -> T {
    use std::{
        future::poll_fn,
        panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
        pin::pin,
        task::Poll,
    };
    let mut changes = artifactize_core::changes::Subscription::new(state).await;
    let observed = {
        let mut observation = pin!(async { observe(&mut changes).await });
        poll_fn(
            |cx| match catch_unwind(AssertUnwindSafe(|| observation.as_mut().poll(cx))) {
                Ok(poll) => poll.map(Ok),
                Err(panic) => Poll::Ready(Err(panic)),
            },
        )
        .await
    };
    let closed = changes.close().await;
    match observed {
        Err(panic) => resume_unwind(panic),
        Ok(result) => {
            closed.expect("join the fixture subscription's shutdown");
            result
        }
    }
}

/// A terminal receipt can precede the worker's final SQLite/notification cleanup. Waiting
/// here owns no execution: the executor and supervisor's saved birth identities are observed,
/// never the launcher, and a still-running Human/timeout result retains its real Run lifetime.
pub async fn terminal_worker_exited(
    view: &store::RunView,
    changes: &mut artifactize_core::changes::Subscription,
    cancellation: &CancellationToken,
) -> Result<(), String> {
    terminal_worker_exited_on(view, cancellation, async || {
        changes.next_reconciled().await;
    })
    .await
}

/// Wait for another reconciliation before observing a still-live owner again.
pub async fn terminal_worker_exited_on(
    view: &store::RunView,
    cancellation: &CancellationToken,
    mut reconcile: impl AsyncFnMut(),
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
                    _ = reconcile() => {},
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
    expected: Option<(&artifactize_core::types::EvalId, RequestStatus)>,
) -> Result<store::RunView, String> {
    let mut changes = artifactize_core::changes::Subscription::new(state).await;
    let result = tokio::time::timeout(ADMISSION_BOUND, async {
        loop {
            // Run status and requests come from one read-only transaction. The observer
            // must not reconcile a dead owner or finish/cancel the Run it is waiting for.
            let view = store::read_run_snapshot(state, id).await?;
            remember_owners(state, &view);
            let waiting = view.run.control.as_ref().map_or_else(
                || {
                    !view
                        .requests
                        .iter()
                        .any(|request| request.status() == RequestStatus::Running)
                },
                |control| control.activity == runs::Activity::Waiting,
            );
            let admitted = match expected {
                Some((eval, status)) => view
                    .requests
                    .iter()
                    .any(|request| &request.eval_id == eval && request.status() == status),
                None => {
                    !view.run.status().is_open()
                        || (waiting
                            && view
                                .requests
                                .iter()
                                .any(|request| request.status().is_human_wait()))
                }
            };
            if admitted {
                terminal_worker_exited(&view, &mut changes, &cancellation).await?;
                return Ok::<_, String>(view);
            }
            if !view.run.status().is_open() {
                return Err("Fixture Run ended before the expected request state.".into());
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
    let observation = submit_fixture(&context, selection, options).await?;
    admitted(context.state(), &observation.run, cancellation, None).await
}

/// Submit the single Eval fixture and wait for the exact Deferred receipt, not admission alone.
pub async fn record_deferred(
    repo: &Path,
    state: &Path,
    eval: &artifactize_core::types::EvalId,
) -> Result<store::RunView, String> {
    let context = runs::Context::new(repo, Some(state), fixture_worker::executable()?)?;
    let observation = submit_fixture(&context, &Selection::All, &VerifyOptions::default()).await?;
    admitted(
        context.state(),
        &observation.run,
        CancellationToken::new(),
        Some((eval, RequestStatus::Deferred)),
    )
    .await
}

pub async fn submit_fixture(
    context: &runs::Context,
    selection: &Selection,
    options: &VerifyOptions,
) -> Result<runs::RunObservation, String> {
    runs::start_observing_launch(context, selection, options, None, |id, supervisor| {
        if let Some(owners) = fixture_owners(context.state(), context.repo()) {
            owners
                .lock()
                .unwrap()
                .entry(id.clone())
                .or_default()
                .supervisor = Some(supervisor);
        }
    })
    .await
}

/// The captured birth identities of a Run this fixture submitted, independent of SQLite.
#[derive(Default)]
struct Owners {
    children: Vec<artifactize_core::process::ChildIdentity>,
    worker: Option<artifactize_core::process::ChildIdentity>,
    supervisor: Option<artifactize_core::process::ChildIdentity>,
}

type OwnedRuns = Mutex<BTreeMap<RunId, Owners>>;
type FixturePaths = (PathBuf, PathBuf);
static LEASES: Mutex<BTreeMap<FixturePaths, Weak<OwnedRuns>>> = Mutex::new(BTreeMap::new());

fn fixture_owners(state: &Path, repo: &Path) -> Option<Arc<OwnedRuns>> {
    LEASES
        .lock()
        .unwrap()
        .get(&(state.to_owned(), repo.to_owned()))
        .and_then(Weak::upgrade)
}

fn remember_owners(state: &Path, view: &store::RunView) {
    let owners = fixture_owners(state, &view.run.repo_path);
    if let (Some(owners), Some(control)) = (owners, &view.run.control) {
        let worker = match control.worker {
            runs::Worker::Admitted { owner } => Some(owner),
            _ => None,
        };
        let children = view
            .requests
            .iter()
            .filter(|request| !request.joined)
            .filter_map(|request| request.child.as_ref())
            .map(|child| artifactize_core::process::ChildIdentity {
                pid: child.pid,
                start_time: child.start_time,
            })
            .collect();
        let mut owners = owners.lock().unwrap();
        let owned = owners.entry(view.run.id.clone()).or_default();
        owned.children = children;
        owned.worker = worker.or(owned.worker);
        owned.supervisor = control.supervisor.or(owned.supervisor);
    }
}

/// Own every Run submitted to one fixture repository, including its temporary input/state.
/// Teardown normally cancels through the saved Run, then waits for worker exit. An unreadable
/// store falls back to the birth identities observed during fixture submission.
pub struct RepoLease {
    root: Option<tempfile::TempDir>,
    state: PathBuf,
    repo: PathBuf,
    owners: Arc<OwnedRuns>,
}

impl RepoLease {
    pub fn new(root: tempfile::TempDir, state: &Path, repo: &Path) -> Self {
        let state = artifactize_core::workspace::canonical_target(state).unwrap();
        let repo = artifactize_core::workspace::canonical_target(repo).unwrap();
        let owners = Arc::new(OwnedRuns::default());
        let previous = LEASES
            .lock()
            .unwrap()
            .insert((state.clone(), repo.clone()), Arc::downgrade(&owners));
        assert!(
            previous.and_then(|lease| lease.upgrade()).is_none(),
            "fixture already has a RepoLease"
        );
        Self {
            root: Some(root),
            state,
            repo,
            owners,
        }
    }

    fn stop_saved_owners(&self) -> Result<(), String> {
        let owners = self.owners.lock().unwrap();
        let mut errors = Vec::new();
        for (id, run) in &*owners {
            let directory = self.state.join("runs").join(id.as_str());
            // Exclude a racing executor launch until both owned processes are stopped.
            let mut executor = run.worker;
            let _launch_guard = match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(directory.join("fixture-launch.lock"))
            {
                Ok(lock) => match lock.lock() {
                    Ok(()) => {
                        match std::fs::read(directory.join("fixture-executor.json")) {
                            Ok(bytes) => match serde_json::from_slice(&bytes) {
                                Ok(owned) => executor = Some(owned),
                                Err(error) => errors.push(error.to_string()),
                            },
                            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                            Err(error) => errors.push(error.to_string()),
                        }
                        Some(lock)
                    }
                    Err(error) => {
                        errors.push(error.to_string());
                        None
                    }
                },
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => {
                    errors.push(error.to_string());
                    None
                }
            };
            let members = run
                .children
                .iter()
                .copied()
                .map(|child| (child, true))
                .chain(
                    executor
                        .into_iter()
                        .chain(run.supervisor)
                        .map(|owner| (owner, false)),
                );
            for (member, group) in members {
                if let Err(error) = process_observation::stop_owned(
                    member.pid.get(),
                    member.start_time,
                    group,
                    ADMISSION_BOUND,
                ) {
                    errors.push(format!("{}: {error}", member.pid.get()));
                }
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    pub fn path(&self) -> &Path {
        self.root.as_ref().expect("fixture root is owned").path()
    }

    /// Explicit teardown before destructive database replacement: cancel owned Runs and
    /// observe saved terminal state plus worker exit, while retaining the fixture root.
    /// This does not disarm the lease; Drop also releases any subsequently submitted Runs.
    pub fn release(&self) -> Result<(), String> {
        match self.state.join(store::DATABASE).try_exists() {
            Ok(true) => {}
            Ok(false) => return self.stop_saved_owners(),
            Err(error) => return Err(error.to_string()),
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
        let released = self.release();
        // A terminal Run and an exited worker do not by themselves prove that every
        // recorded child exited. Recheck all recorded lifetimes before deleting inputs.
        let stopped = self.stop_saved_owners();
        LEASES
            .lock()
            .unwrap()
            .remove(&(self.state.clone(), self.repo.clone()));
        let root = self.root.take().expect("fixture root is owned");
        let (closed, ipc) = if stopped.is_ok() {
            // Name the namespace before closing the fixture; attempt both removals.
            let namespace = artifactize_core::changes::namespace(&self.state);
            (root.close(), namespace.and_then(std::fs::remove_dir_all))
        } else {
            // A surviving owned process still needs its inputs. Keep them only when
            // a checked, receipt-independent stop failed, never for receipt failure alone.
            let retained = root.keep();
            (
                Err(std::io::Error::other(format!(
                    "process exit unconfirmed; fixture retained at {}",
                    fixture_worker::path_text(&retained)
                ))),
                Ok(()),
            )
        };
        let failure = [
            released,
            stopped,
            closed.map_err(|error| error.to_string()),
            ipc.map_err(|error| error.to_string()),
        ]
        .into_iter()
        .filter_map(Result::err)
        .collect::<Vec<_>>();
        if !failure.is_empty() {
            let message = format!("Fixture Run cleanup failed: {}", failure.join("; "));
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
        if store::read_run_snapshot(state, id)
            .await?
            .run
            .status()
            .is_open()
        {
            // This is an explicit cleanup action, not part of an observation barrier.
            runs::cancel(context, id).await?;
        }
        loop {
            let view = store::read_run_snapshot(state, id).await?;
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

/// Original command output and, separately, the saved view for a submit-only verify.
pub struct Recorded {
    pub output: Output,
    pub saved: Option<store::RunView>,
}

impl Recorded {
    /// The admission snapshot from the store, not a claim about the command's stdout.
    pub fn saved_json(&self) -> Option<serde_json::Value> {
        self.saved.as_ref().map(|view| {
            serde_json::to_value(artifactize_core::query::run_output(
                view,
                time::OffsetDateTime::now_utc(),
            ))
            .expect("saved Run view serializes")
        })
    }
}

pub trait RecordOutput {
    fn recorded(&mut self) -> std::io::Result<Recorded>;

    fn record_output(&mut self) -> std::io::Result<Output> {
        self.recorded().map(|recorded| recorded.output)
    }
}
impl RecordOutput for Command {
    fn recorded(&mut self) -> std::io::Result<Recorded> {
        let args = self
            .get_args()
            .map(|arg| arg.to_os_string())
            .collect::<Vec<_>>();
        let verifies = args.iter().any(|arg| arg == "verify");
        let is_submission = verifies && !args.iter().any(|arg| arg == "--wait");
        let output = fixture_worker::output(self)?;
        if !verifies {
            return Ok(Recorded {
                output,
                saved: None,
            });
        }
        let accepted: serde_json::Value = match serde_json::from_slice(&output.stdout) {
            Ok(value) => value,
            Err(_) => {
                return Ok(Recorded {
                    output,
                    saved: None,
                });
            }
        };
        let submission_id = accepted["run"].as_str();
        let Some(id) = submission_id.or_else(|| accepted["id"].as_str()) else {
            return Ok(Recorded {
                output,
                saved: None,
            });
        };
        // Submission-only fixtures can inspect the saved request after admission. A waiting
        // command's own result is already terminal; observe worker exit before returning it.
        let inspect_admission = is_submission && output.status.success() && submission_id.is_some();
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
                        if inspect_admission {
                            admitted(&state, &id, CancellationToken::new(), None)
                                .await
                                .map(Some)
                        } else {
                            let mut changes =
                                artifactize_core::changes::Subscription::new(&state).await;
                            let result = tokio::time::timeout(ADMISSION_BOUND, async {
                                let view = store::read_run_snapshot(&state, &id).await?;
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
        Ok(Recorded {
            output,
            saved: view,
        })
    }
}

/// What runtime request `request` printed on `stream` (`stdout` or `stderr`), as `request logs`
/// reads it from the state at `state`, each line ended by a newline. A runtime result keeps no
/// output of its own.
pub fn logged(state: &Path, request: &str, stream: &str) -> String {
    let output = std::process::Command::new(super::os::cli())
        .arg("--state-dir")
        .arg(state)
        .args(["request", "logs", request, "--stream", stream, "--json"])
        .env("ARTIFACTIZE_REMOTE", "off")
        .record_output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let logs: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    logs["streams"][0]["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| format!("{}\n", line["text"].as_str().unwrap()))
        .collect()
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

/// Deadline of the terminal worker barrier, with a short controlled clock in its own test.
pub async fn worker_exit_watchdog<T>(bound: Duration, wait: impl Future<Output = T>) -> T {
    tokio::time::timeout(bound, wait)
        .await
        .expect("terminal fixture worker exits before the byte baseline")
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
                runtime.block_on(with_subscription(state, async |changes| {
                    let view = store::read_run_snapshot(state, id).await.unwrap();
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
                    worker_exit_watchdog(ADMISSION_BOUND, async {
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
                }));
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
                            let view = store::read_run_snapshot(state, &active.id).await.unwrap();
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
