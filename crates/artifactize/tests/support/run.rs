//! Fixture ownership is separate from the public lifetime of a Run observer.
#![expect(
    dead_code,
    reason = "each CLI test binary uses a different fixture seam"
)]

#[path = "root.rs"]
mod root;
pub type Root = root::Root;

#[path = "launch.rs"]
pub mod launch;

use artifactize_core::{changes::Subscription, runs, types::RunId};
use futures_util::FutureExt;
use std::{
    collections::BTreeSet,
    io::{self, BufRead},
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
    process::{Child, Output},
    time::Duration,
};

/// Register before observing and join watch/hub/SQLite shutdown before returning a value,
/// an error or an assertion panic. Watchdogs belong inside the observation scope too.
pub async fn subscribed<T>(state: &Path, observe: impl AsyncFnOnce(&mut Subscription) -> T) -> T {
    let mut changes = Subscription::new(state).await;
    let result = std::panic::AssertUnwindSafe(async { observe(&mut changes).await })
        .catch_unwind()
        .await;
    let closed = changes.close().await;
    match result {
        Ok(value) => {
            closed.expect("join the fixture subscription's shutdown");
            value
        }
        Err(panic) => {
            if let Err(error) = closed {
                eprintln!("Fixture subscription shutdown failed: {error}");
            }
            std::panic::resume_unwind(panic)
        }
    }
}

/// Keep a sign-off available across a fixture's unrelated state reads and commands.
/// The watchdogs bound all observations; the production default grace is not test input.
pub fn hold_signoffs(state: &Path) {
    crate::support::os::create_private_dir_all(state);
    let limits = serde_json::json!({"signoffIdleGraceSeconds":artifactize_core::limits::MAX_SIGNOFF_IDLE_GRACE.as_secs()});
    crate::support::os::publish_file(&state.join("limits.json"), limits.to_string());
}

/// Open the provider response gate on unwind before dropping its owning fixture.
pub struct ReleaseGate(crate::support::Gate);
impl ReleaseGate {
    pub fn new(gate: crate::support::Gate) -> Self {
        gate.close();
        Self(gate)
    }
}
impl Drop for ReleaseGate {
    fn drop(&mut self) {
        self.0.open();
    }
}

/// Release a file-gated fixture even when its observer unwinds.
pub struct FileGate(PathBuf);
impl FileGate {
    pub fn new(release: PathBuf) -> Self {
        Self(release)
    }
    pub fn release(&self) -> io::Result<()> {
        std::fs::write(crate::support::declaration::source_path(&self.0), "ready")
    }
}
impl Drop for FileGate {
    fn drop(&mut self) {
        let _ = self.release();
    }
}

/// Own only the caller process. Disposing a `run show --wait` observer must not cancel its Run.
pub struct Watcher(Option<Child>);
impl Watcher {
    pub fn new(child: Child) -> Self {
        Self(Some(child))
    }
    /// Stop only an unreaped caller, never signal a PID after its cached exit.
    pub fn stop(&mut self) -> io::Result<std::process::ExitStatus> {
        crate::support::os::stop_child(self)
    }
    pub fn exited(&mut self) -> bool {
        crate::support::os::child_exited(self)
    }
    /// Reap a fixture owner whose exit has already been established externally.
    pub fn reap(&mut self) -> io::Result<std::process::ExitStatus> {
        const EXIT_BOUND: Duration = Duration::from_secs(20);
        crate::support::os::wait_child_for(self, EXIT_BOUND)
    }
    pub fn first_stderr_line(&mut self, bound: Duration) -> io::Result<String> {
        let mut stderr = self.stderr.take().expect("piped fixture stderr");
        let (send, receive) = std::sync::mpsc::channel();
        let line = std::thread::scope(|scope| {
            scope.spawn(|| {
                let mut line = String::new();
                // A one-byte reader cannot swallow later stderr from the caller.
                let read = io::BufReader::with_capacity(1, &mut stderr).read_line(&mut line);
                let _ = send.send(read.map(|_| line));
            });
            match receive.recv_timeout(crate::support::os::patience(bound)) {
                Ok(line) => line,
                Err(error) => {
                    // The CLI's detached workers use their own stderr files. Stopping and
                    // reaping the caller closes its pipe and lets the scoped reader join.
                    self.stop()?;
                    Err(match error {
                        std::sync::mpsc::RecvTimeoutError::Timeout => io::Error::new(
                            io::ErrorKind::TimedOut,
                            "fixture caller did not announce its Run",
                        ),
                        std::sync::mpsc::RecvTimeoutError::Disconnected => {
                            io::Error::other("fixture stderr reader disconnected")
                        }
                    })
                }
            }
        });
        self.stderr = Some(stderr);
        line
    }

    pub fn wait_with_output(mut self) -> io::Result<Output> {
        // Retain the caller guard while draining both pipes, including I/O-error exits.
        fn drain(pipe: Option<impl io::Read>) -> io::Result<Vec<u8>> {
            let mut bytes = Vec::new();
            if let Some(mut pipe) = pipe {
                pipe.read_to_end(&mut bytes)?;
            }
            Ok(bytes)
        }
        let stdout = self.stdout.take();
        let stderr = self.stderr.take();
        self.stdin.take();
        std::thread::scope(|scope| {
            let stdout = scope.spawn(|| drain(stdout));
            let stderr = scope.spawn(|| drain(stderr));
            // Drain both pipes before waiting so a full pipe cannot block the caller's exit.
            // The deadline only bounds fixture failure; the native exit is the barrier.
            const EXIT_BOUND: Duration = Duration::from_secs(60);
            let status = crate::support::os::wait_child_for(&mut self, EXIT_BOUND);
            if status.is_err() {
                let _ = self.stop();
            }
            let stdout = stdout.join().expect("fixture stdout reader")?;
            let stderr = stderr.join().expect("fixture stderr reader")?;
            Ok(Output {
                status: status?,
                stdout,
                stderr,
            })
        })
    }
}
impl Deref for Watcher {
    type Target = Child;
    fn deref(&self) -> &Child {
        self.0.as_ref().expect("owned caller")
    }
}
impl DerefMut for Watcher {
    fn deref_mut(&mut self) -> &mut Child {
        self.0.as_mut().expect("owned caller")
    }
}
impl Drop for Watcher {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = crate::support::os::stop_child(child);
        }
    }
}

/// Own detached tool leaders and descendants by birth identity. Each child is registered
/// independently so a leader exiting cannot leave its leaderless descendants unowned.
#[derive(Default)]
pub struct Processes {
    identities: BTreeSet<(u32, u64)>,
    inputs: root::Inputs,
}
impl Processes {
    /// Retain the owning temporary root if cleanup cannot confirm process exit.
    pub fn protect(root: &Root) -> Self {
        Self {
            inputs: root.retention(),
            identities: BTreeSet::new(),
        }
    }

    pub fn own_identity(&mut self, pid: u32, start: u64) {
        self.identities.insert((pid, start));
    }
    pub fn own(&mut self, pid: u32) {
        if let Some(start) = crate::support::os::observed_start_time(pid) {
            self.own_identity(pid, start);
        }
    }

    pub fn release(&mut self) -> Result<(), String> {
        for &(pid, start) in &self.identities {
            if crate::support::os::same_process_running(pid, start) {
                crate::support::os::kill_tree(pid);
                crate::support::os::kill(pid);
            }
        }
        // Liveness polling observes release only; this interval orders no test behavior.
        const RELEASE_BOUND: Duration = Duration::from_secs(30);
        const RELEASE_POLL: Duration = Duration::from_millis(10);
        let deadline = std::time::Instant::now() + crate::support::os::patience(RELEASE_BOUND);
        loop {
            self.identities
                .retain(|&(pid, start)| crate::support::os::same_process_running(pid, start));
            if self.identities.is_empty() {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                self.inputs.retain();
                return Err(format!(
                    "owned detached fixture processes did not exit: {:?}",
                    self.identities
                ));
            }
            std::thread::sleep(RELEASE_POLL);
        }
    }
}
impl Drop for Processes {
    fn drop(&mut self) {
        if let Err(error) = self.release() {
            if std::thread::panicking() {
                eprintln!("{error}");
            } else {
                panic!("{error}");
            }
        }
    }
}

/// Own the submitted Run in addition to its watcher, until the fixture releases both.
pub struct RunChild {
    child: Watcher,
    state: PathBuf,
    repo: PathBuf,
    run: Option<RunId>,
    announcement: String,
}
impl RunChild {
    pub fn new(child: Child, state: &Path, repo: &Path) -> Self {
        // Install the watcher guard before any I/O, path resolution or parsing can unwind.
        let mut owned = Self {
            child: Watcher::new(child),
            state: state.to_owned(),
            repo: repo.to_owned(),
            run: None,
            announcement: String::new(),
        };
        // Startup is only a fixture watchdog; a slow evaluation does not use this bound.
        const ANNOUNCEMENT_BOUND: Duration = Duration::from_secs(30);
        owned.announcement = owned
            .child
            .first_stderr_line(ANNOUNCEMENT_BOUND)
            .expect("read the fixture caller's Run announcement");
        owned.run = owned.announcement.starts_with("Started run-").then(|| {
            owned
                .announcement
                .split_whitespace()
                .nth(1)
                .unwrap()
                .parse()
                .unwrap()
        });
        owned
    }
    pub fn exited(&mut self) -> bool {
        self.child.exited()
    }
    pub fn announcement(&self) -> &str {
        &self.announcement
    }
    pub fn run(&self) -> RunId {
        self.run
            .clone()
            .expect("valid submission announced its Run")
    }
    /// Observe the saved Run without asking a caller to reconcile it first.
    pub fn saved(&self) -> artifactize_core::store::RunView {
        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .unwrap();
                    runtime
                        .block_on(artifactize_core::store::read_run(&self.state, &self.run()))
                        .unwrap()
                })
                .join()
                .unwrap()
        })
    }
    /// The sync CLI fixture keeps the runtime/thread scoped to this read-only barrier.
    pub fn human_waiting(&self) -> artifactize_core::store::RunView {
        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .unwrap();
                    runtime.block_on(human_waiting(&self.state, &self.run()))
                })
                .join()
                .unwrap()
        })
    }
    pub fn wait_with_output(mut self) -> io::Result<Output> {
        self.collect_output()
    }
    /// Join the watcher without releasing the Run before its saved audit is read.
    pub fn collect_output(&mut self) -> io::Result<Output> {
        let child = std::mem::replace(&mut self.child, Watcher(None));
        let mut output = child.wait_with_output()?;
        let mut stderr = self.announcement.as_bytes().to_vec();
        stderr.extend_from_slice(&output.stderr);
        output.stderr = stderr;
        Ok(output)
    }
}
impl Deref for RunChild {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.child
    }
}
impl DerefMut for RunChild {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.child
    }
}
impl Drop for RunChild {
    fn drop(&mut self) {
        let Some(run) = &self.run else { return };
        let cleanup = release_run(&self.state, &self.repo, run);
        // Reap the watcher even if Run cleanup fails; the field guard is also unwind-safe.
        if let Some(child) = &mut self.child.0 {
            let _ = crate::support::os::stop_child(child);
        }
        if !std::thread::panicking() {
            cleanup.expect("fixture releases its exact Run and worker before temporary state");
        }
    }
}

/// Release only Runs in this fixture's repository/state, independently of any observers.
/// Submit-only fixtures own execution here; dropping an individual watcher still does not.
pub fn release_repo(state: &Path, repo: &Path) {
    let cleanup = release_repo_result(state, repo);
    if !std::thread::panicking() {
        cleanup.expect("fixture releases all its submitted Runs before state");
    }
}

pub(super) fn release_repo_result(state: &Path, repo: &Path) -> Result<(), String> {
    if !state.join(artifactize_core::store::DATABASE).is_file() {
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
                    let context =
                        runs::Context::new(repo, Some(state), &crate::support::os::cli())?;
                    // Bound page size only; no fixture Run is excluded by a count ceiling.
                    const PAGE_SIZE: u32 = 128;
                    let mut offset = 0;
                    let mut errors = Vec::new();
                    loop {
                        let page = artifactize_core::store::read_runs(
                            state,
                            Some(repo),
                            PAGE_SIZE,
                            offset,
                        )
                        .await?;
                        let count = page.len();
                        // A broken Run must not prevent releasing the fixture's other Runs.
                        for run in page {
                            if let Err(error) = release(&context, state, &run.id).await {
                                errors.push(format!("{}: {error}", run.id));
                            }
                        }
                        if count < PAGE_SIZE as usize {
                            return if errors.is_empty() {
                                Ok(())
                            } else {
                                Err(errors.join("; "))
                            };
                        }
                        offset += PAGE_SIZE;
                    }
                })
            })
            .join()
    })
    .unwrap_or_else(|_| Err("fixture repository cleanup panicked".into()))
}

async fn release(context: &runs::Context, state: &Path, id: &RunId) -> Result<(), String> {
    // A watchdog bounds broken fixture cleanup, never the execution's lifetime.
    const CLEANUP_BOUND: Duration = Duration::from_secs(30);
    subscribed(state, async |changes| {
        let result = tokio::time::timeout(crate::support::os::patience(CLEANUP_BOUND), async {
            let saved = artifactize_core::store::read_run(state, id).await?;
            if saved.run.status().is_open() {
                // This is an explicit fixture cleanup stimulus, never an observation barrier.
                runs::cancel(context, id).await?;
                loop {
                    let saved = artifactize_core::store::read_run(state, id).await?;
                    if !saved.run.status().is_open() {
                        break;
                    }
                    changes.next_reconciled().await;
                }
            }
            // Terminal state is saved before the durable worker closes its last
            // connection. Its exact identity must also exit before TempDir drops.
            let view = artifactize_core::store::read_run(state, id).await?;
            if let Some(control) = view.run.control {
                let owner = match control.worker {
                    runs::Worker::Starting { launcher } => Some(launcher),
                    runs::Worker::Admitted { owner } => Some(owner),
                    runs::Worker::Legacy => None,
                };
                for owner in owner.into_iter().chain(control.supervisor) {
                    while crate::support::os::same_process_running(
                        owner.pid.get(),
                        owner.start_time,
                    ) {
                        changes.next_reconciled().await;
                    }
                }
            }
            Ok::<_, String>(())
        })
        .await;
        result.map_err(|_| "owned Run cleanup timed out".to_owned())?
    })
    .await
}

/// Observe the saved terminal state without reconciling, killing owners or rewriting audit.
pub async fn terminal(state: &Path, id: &RunId) -> artifactize_core::store::RunView {
    // Bound a broken fixture observation, not the Run's execution lifetime.
    const OBSERVATION_BOUND: Duration = Duration::from_secs(30);
    subscribed(state, async |changes| {
        let mut latest = None;
        let result = tokio::time::timeout(crate::support::os::patience(OBSERVATION_BOUND), async {
            loop {
                let view = artifactize_core::store::read_run(state, id).await?;
                if !view.run.status().is_open() {
                    return Ok::<_, String>(view);
                }
                latest = Some(view);
                changes.next_reconciled().await;
            }
        })
        .await;
        result
            .unwrap_or_else(|error| {
                panic!(
                    "saved terminal fixture state: {error}; last saved Run/control/requests: {}",
                    serde_json::to_string(&latest)
                        .expect("serialize the observed fixture snapshot")
                )
            })
            .expect("read terminal fixture state")
    })
    .await
}

/// Observe a saved Human request only after its admitted worker has released scheduling.
/// A request becoming WAITING_HUMAN does not itself establish this distinct Run state.
pub async fn human_waiting(state: &Path, id: &RunId) -> artifactize_core::store::RunView {
    const OBSERVATION_BOUND: Duration = Duration::from_secs(30);
    subscribed(state, async |changes| {
        let result = tokio::time::timeout(crate::support::os::patience(OBSERVATION_BOUND), async {
            loop {
                let view = artifactize_core::store::read_run(state, id).await?;
                if view.run.control.as_ref().is_some_and(|control| {
                    matches!(control.worker, runs::Worker::Admitted { .. })
                        && control.activity == runs::Activity::Waiting
                }) && view
                    .requests
                    .iter()
                    .any(|request| request.status().is_human_wait())
                {
                    return Ok::<_, String>(view);
                }
                changes.next_reconciled().await;
            }
        })
        .await;
        result
            .expect("saved waiting Human Run and admitted worker")
            .expect("read waiting fixture state")
    })
    .await
}

/// Ownership of every submission to a fixture repository, independent of its observers.
pub struct RepoLease {
    state: PathBuf,
    repo: PathBuf,
}
impl RepoLease {
    pub fn new(state: &Path, repo: &Path) -> Self {
        Self {
            state: state.to_owned(),
            repo: repo.to_owned(),
        }
    }
}
impl Drop for RepoLease {
    fn drop(&mut self) {
        release_repo(&self.state, &self.repo);
    }
}

/// Own just the real submission when the rest of a state is deliberately corrupted input.
pub struct RunLease {
    state: PathBuf,
    repo: PathBuf,
    run: RunId,
}
impl RunLease {
    pub fn new(state: &Path, repo: &Path, run: RunId) -> Self {
        Self {
            state: state.to_owned(),
            repo: repo.to_owned(),
            run,
        }
    }
}
impl Drop for RunLease {
    fn drop(&mut self) {
        let cleanup = release_run(&self.state, &self.repo, &self.run);
        if !std::thread::panicking() {
            cleanup.expect("fixture releases its single owned Run");
        }
    }
}

fn release_run(state: &Path, repo: &Path, run: &RunId) -> Result<(), String> {
    std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|error| error.to_string())?;
                runtime.block_on(async {
                    let context =
                        runs::Context::new(repo, Some(state), &crate::support::os::cli())?;
                    release(&context, state, run).await
                })
            })
            .join()
    })
    .unwrap_or_else(|_| Err("owned Run cleanup panicked".into()))
}

#[cfg(test)]
mod observation_tests {
    use super::*;

    #[derive(Clone, Copy)]
    enum Exit {
        Success,
        Error,
        Watchdog,
        Panic,
    }

    #[tokio::test]
    async fn subscription_releases_sqlite_before_return_error_watchdog_and_panic() {
        for exit in [Exit::Success, Exit::Error, Exit::Watchdog, Exit::Panic] {
            let root = crate::support::os::tempdir();
            let state = root.path().join("state");
            let repo = root.path().join("repo");
            std::fs::create_dir(&repo).unwrap();
            let writer = artifactize_core::store::Receipts::open(&state, &repo)
                .await
                .unwrap();
            writer.close().await.unwrap();
            let database = state.join(artifactize_core::store::DATABASE);
            assert!(crate::support::os::database_released(&database).unwrap());
            let result = std::panic::AssertUnwindSafe(subscribed(&state, async |_changes| {
                assert!(
                    !crate::support::os::database_released(&database).unwrap(),
                    "the registered subscription owns a real SQLite reader"
                );
                match exit {
                    Exit::Success => Ok(()),
                    Exit::Error => Err("fixture observation error"),
                    Exit::Watchdog => {
                        tokio::time::timeout(Duration::ZERO, std::future::pending::<()>())
                            .await
                            .map_err(|_| "fixture watchdog")
                    }
                    Exit::Panic => panic!("fixture observation assertion"),
                }
            }))
            .catch_unwind()
            .await;
            match exit {
                Exit::Success => assert_eq!(result.unwrap(), Ok(())),
                Exit::Error => assert_eq!(result.unwrap(), Err("fixture observation error")),
                Exit::Watchdog => assert_eq!(result.unwrap(), Err("fixture watchdog")),
                Exit::Panic => assert!(result.is_err()),
            }
            assert!(
                crate::support::os::database_released(&database).unwrap(),
                "the observation joins its SQLite worker before returning"
            );
            root.close().expect("remove the released fixture state");
        }
    }
}
