//! Fixture ownership is separate from the public lifetime of a Run observer.
#![expect(
    dead_code,
    reason = "each CLI test binary uses a different fixture seam"
)]

use artifactize_core::{runs, types::RunId};
use std::{
    io::{self, BufRead},
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
    process::{Child, Output},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

/// Own only the caller process. Disposing a `run show --wait` observer must not cancel its Run.
pub struct Watcher(Option<Child>);
impl Watcher {
    pub fn new(child: Child) -> Self {
        Self(Some(child))
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
            let status = self.wait();
            if status.is_err() {
                crate::support::os::kill(self.id());
                let _ = self.wait();
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
            if child.try_wait().ok().flatten().is_none() {
                crate::support::os::kill(child.id());
            }
            let _ = child.wait();
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
        // A one-byte reader cannot swallow later stderr from the watcher.
        std::io::BufReader::with_capacity(1, owned.child.stderr.as_mut().expect("piped stderr"))
            .read_line(&mut owned.announcement)
            .unwrap();
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
    pub fn announcement(&self) -> &str {
        &self.announcement
    }
    pub fn run(&self) -> RunId {
        self.run
            .clone()
            .expect("valid submission announced its Run")
    }
    pub fn wait_with_output(mut self) -> io::Result<Output> {
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
        let cleanup = std::thread::scope(|scope| {
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
                            &crate::support::os::cli(),
                        )?;
                        release(&context, &self.state, run).await
                    })
                })
                .join()
        })
        .unwrap_or_else(|_| Err("owned Run cleanup panicked".into()));
        // Reap the watcher even if Run cleanup fails; the field guard is also unwind-safe.
        if let Some(child) = &mut self.child.0 {
            if child.try_wait().ok().flatten().is_none() {
                crate::support::os::kill(child.id());
            }
            let _ = child.wait();
        }
        if !std::thread::panicking() {
            cleanup.expect("fixture releases its exact Run and worker before temporary state");
        }
    }
}

/// Release only Runs in this fixture's repository/state, independently of any observers.
/// Submit-only fixtures own execution here; dropping an individual watcher still does not.
pub fn release_repo(state: &Path, repo: &Path) {
    if !state.join(artifactize_core::store::DATABASE).is_file() {
        return;
    }
    let cleanup = std::thread::scope(|scope| {
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
                    loop {
                        let page = artifactize_core::store::read_runs(
                            state,
                            Some(repo),
                            PAGE_SIZE,
                            offset,
                        )
                        .await?;
                        let count = page.len();
                        for run in page {
                            release(&context, state, &run.id).await?;
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
    .unwrap_or_else(|_| Err("fixture repository cleanup panicked".into()));
    if !std::thread::panicking() {
        cleanup.expect("fixture releases all its submitted Runs before state");
    }
}

async fn release(context: &runs::Context, state: &Path, id: &RunId) -> Result<(), String> {
    // A watchdog bounds broken fixture cleanup, never the execution's lifetime.
    const CLEANUP_BOUND: Duration = Duration::from_secs(30);
    tokio::time::timeout(crate::support::os::patience(CLEANUP_BOUND), async {
        let mut changes = artifactize_core::changes::Subscription::new(state).await;
        if !runs::status(context, id).await?.state.is_terminal() {
            runs::cancel(context, id).await?;
            runs::wait(context, id, None, CancellationToken::new()).await?;
        }
        // Terminal state is saved before the durable worker closes its last
        // connection. Its exact identity must also exit before TempDir drops.
        let view = runs::view(context, id).await?;
        if let Some(control) = view.run.control {
            let owner = match control.worker {
                runs::Worker::Starting { launcher } => Some(launcher),
                runs::Worker::Admitted { owner } => Some(owner),
                runs::Worker::Legacy => None,
            };
            if let Some(owner) = owner {
                while crate::support::os::running(owner.pid.get()) {
                    changes.next_reconciled().await;
                }
            }
        }
        Ok::<_, String>(())
    })
    .await
    .map_err(|_| "owned Run cleanup timed out".to_owned())?
}
