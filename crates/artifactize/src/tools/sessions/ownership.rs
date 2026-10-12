//! Durable admission records and invocation leases for a review-owned session.

use std::{
    collections::BTreeMap,
    ffi::OsString,
    io,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
};

use serde::{Deserialize, Serialize};
use tokio::{runtime::Handle, sync::Notify};
use tokio_util::sync::CancellationToken;

use crate::{platform, process};

/// Cleanup must survive the caller's executor shutting down, not just a dropped
/// review future. A single lazy cleanup thread owns all session process I/O and
/// timers; tokens and join handles work across executors. Process exit still uses
/// durable group records (Unix) or kill-on-close Jobs (Windows).
pub(crate) fn executor() -> io::Result<&'static Handle> {
    static EXECUTOR: OnceLock<Result<Handle, String>> = OnceLock::new();
    EXECUTOR
        .get_or_init(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| error.to_string())?;
            let handle = runtime.handle().clone();
            std::thread::Builder::new()
                .name("tool-session-cleanup".into())
                .spawn(move || runtime.block_on(std::future::pending::<()>()))
                .map_err(|error| error.to_string())?;
            Ok(handle)
        })
        .as_ref()
        .map_err(|error| io::Error::other(error.clone()))
}

pub(super) struct Ownership {
    directory: PathBuf,
    pub ended: CancellationToken,
    active: AtomicUsize,
    finished: Notify,
}

impl Ownership {
    pub fn new(directory: &Path) -> Arc<Self> {
        Arc::new(Self {
            directory: directory.into(),
            ended: CancellationToken::new(),
            active: AtomicUsize::new(0),
            finished: Notify::new(),
        })
    }

    pub fn call(self: &Arc<Self>, environment: &BTreeMap<OsString, OsString>) -> Option<Call> {
        self.active.fetch_add(1, Ordering::AcqRel);
        let call = Call {
            owner: self.clone(),
            environment: environment.clone(),
            record: Record::new(&self.directory),
        };
        if self.ended.is_cancelled() {
            None
        } else {
            Some(call)
        }
    }

    pub async fn finish_calls(&self) {
        self.ended.cancel();
        loop {
            let notified = self.finished.notified();
            // Register the waiter before checking the count to avoid lost wakeups.
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.active.load(Ordering::Acquire) == 0 {
                return;
            }
            notified.await;
        }
    }
}

/// The lease lasts until the invocation's independent supervisor has reaped its
/// group. Session cleanup cannot remove shared files while a bound call uses them.
pub(crate) struct Call {
    owner: Arc<Ownership>,
    pub environment: BTreeMap<OsString, OsString>,
    record: Arc<Record>,
}

impl Call {
    pub async fn run(
        self,
        command: process::Command,
        input: Vec<u8>,
        limit: usize,
        cancellation: CancellationToken,
    ) -> Result<process::Output, process::Error> {
        let cancellation = cancellation.child_token();
        let record = self.record.clone();
        let running = process::run_with_input_limit(
            command,
            Some(input),
            limit,
            cancellation.clone(),
            move |child| async move { record.register(child).await },
        );
        tokio::pin!(running);
        tokio::select! {
            biased;
            _ = self.owner.ended.cancelled() => { cancellation.cancel(); running.await },
            result = &mut running => result,
        }
    }
}

impl Drop for Call {
    fn drop(&mut self) {
        // Retire the durable identity before allowing session directory removal.
        self.record.retire();
        self.owner.active.fetch_sub(1, Ordering::AcqRel);
        self.owner.finished.notify_waiters();
    }
}

pub(super) struct Record {
    directory: PathBuf,
    child: Mutex<Option<process::ChildIdentity>>,
}

impl Record {
    pub fn new(directory: &Path) -> Arc<Self> {
        Arc::new(Self {
            directory: directory.into(),
            child: Mutex::new(None),
        })
    }

    pub async fn register(self: Arc<Self>, child: process::ChildIdentity) -> io::Result<()> {
        crate::task::joined(tokio::task::spawn_blocking(move || self.save(child)).await)
            .map_err(io::Error::other)?
    }

    fn save(&self, child: process::ChildIdentity) -> io::Result<()> {
        let path = self.directory.join(format!("process-{}.json", child.pid));
        let pending = path.with_extension("next");
        let mut file = platform::private_options()
            .write(true)
            .create_new(true)
            .open(&pending)?;
        std::io::Write::write_all(
            &mut file,
            &serde_json::to_vec(&child).map_err(io::Error::other)?,
        )?;
        file.sync_all()?;
        std::fs::rename(pending, path)?;
        platform::sync_dir(&self.directory)?;
        *self.child.lock().expect("process record lock") = Some(child);
        Ok(())
    }

    fn retire(&self) {
        if let Some(child) = self.child.lock().expect("process record lock").take() {
            let _ =
                std::fs::remove_file(self.directory.join(format!("process-{}.json", child.pid)));
        }
    }
}

impl Drop for Record {
    fn drop(&mut self) {
        self.retire();
    }
}

#[derive(Serialize, Deserialize)]
struct Owner {
    owner: process::ChildIdentity,
    leader: Option<process::ChildIdentity>,
}

pub(super) fn allocate(root: &Path) -> io::Result<platform::PrivateTempDir> {
    platform::create_private_dir_all(root)?;
    let lock = platform::private_options()
        .read(true)
        .write(true)
        .create(true)
        .open(root.join("lock"))?;
    lock.lock()?;
    // The allocation lock makes an owner-less directory unambiguously abandoned.
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let path = entry.path();
        let record = match std::fs::read(path.join("owner.json")) {
            Ok(bytes) => Some(serde_json::from_slice::<Owner>(&bytes).map_err(io::Error::other)?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        if let Some(record) = record {
            if process::is_alive(Some(record.owner))? {
                continue;
            }
            if let Some(leader) = record.leader {
                platform::kill_saved_group(leader)?;
            }
            for entry in std::fs::read_dir(&path)? {
                let entry = entry?;
                if !entry.file_name().to_string_lossy().starts_with("process-")
                    || entry
                        .path()
                        .extension()
                        .is_none_or(|extension| extension != "json")
                {
                    continue;
                }
                let child = serde_json::from_slice(&std::fs::read(entry.path())?)
                    .map_err(io::Error::other)?;
                platform::kill_saved_group(child)?;
            }
        }
        match platform::remove_abandoned_directory(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    let directory = platform::private_tempdir_in("session-", root)?;
    save_owner(directory.path(), None)?;
    Ok(directory)
}

pub(super) fn save_owner(
    directory: &Path,
    leader: Option<process::ChildIdentity>,
) -> io::Result<()> {
    let owner = process::child_identity(std::process::id())?;
    let bytes = serde_json::to_vec(&Owner { owner, leader }).map_err(io::Error::other)?;
    let path = directory.join("owner.next");
    let mut file = platform::private_options()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)?;
    std::io::Write::write_all(&mut file, &bytes)?;
    file.sync_all()?;
    std::fs::rename(path, directory.join("owner.json"))?;
    platform::sync_dir(directory)
}
