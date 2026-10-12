//! Durable admission journals. Removal requires confirmed process cleanup.
use super::ownership::Cleanup;
use crate::{platform, process};
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

pub(super) struct Record {
    directory: PathBuf,
    child: Mutex<Admission>,
    kind: RecordKind,
}

enum Admission {
    Empty,
    Owned(process::ChildIdentity),
    Cleaned(process::ChildIdentity),
}
enum RecordKind {
    Start,
    Invocation,
}

impl Record {
    pub fn new(directory: &Path) -> Arc<Self> {
        Self::create(directory, RecordKind::Invocation)
    }
    pub fn start(directory: &Path) -> Arc<Self> {
        Self::create(directory, RecordKind::Start)
    }

    fn create(directory: &Path, kind: RecordKind) -> Arc<Self> {
        Arc::new(Self {
            directory: directory.into(),
            child: Mutex::new(Admission::Empty),
            kind,
        })
    }

    pub async fn register(self: Arc<Self>, child: process::ChildIdentity) -> io::Result<()> {
        crate::task::joined(tokio::task::spawn_blocking(move || self.save(child)).await)
            .map_err(io::Error::other)?
    }

    fn save(&self, child: process::ChildIdentity) -> io::Result<()> {
        // Set the in-memory identity before I/O. A failed/late registration cannot
        // admit code, but any successfully persisted record remains conservative.
        *self.child.lock().expect("process record lock") = Admission::Owned(child);
        if matches!(self.kind, RecordKind::Start) {
            return save_owner(&self.directory, Some(child));
        }
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
        platform::sync_dir(&self.directory)
    }

    pub fn after_run<T>(&self, result: &Result<T, process::Error>) {
        if result
            .as_ref()
            .err()
            .is_none_or(process::Error::cleanup_confirmed)
        {
            self.confirm();
        }
    }

    pub fn confirm(&self) {
        let mut state = self.child.lock().expect("process record lock");
        if let Admission::Owned(child) = *state {
            *state = Admission::Cleaned(child);
        }
    }

    pub fn finish(&self) -> Cleanup {
        let mut state = self.child.lock().expect("process record lock");
        let child = match *state {
            Admission::Empty => return Cleanup::default(),
            Admission::Owned(child) => {
                return Cleanup::Incomplete(format!(
                    "Process {} cleanup was not confirmed; recovery records retained.",
                    child.pid
                ));
            }
            Admission::Cleaned(child) => child,
        };
        let retired = match self.kind {
            RecordKind::Start => save_owner(&self.directory, None),
            RecordKind::Invocation => {
                std::fs::remove_file(self.directory.join(format!("process-{}.json", child.pid)))
                    .or_else(|error| {
                        if error.kind() == io::ErrorKind::NotFound {
                            Ok(())
                        } else {
                            Err(error)
                        }
                    })
                    .and_then(|()| platform::sync_dir(&self.directory))
            }
        };
        match retired {
            Ok(()) => {
                *state = Admission::Empty;
                Cleanup::default()
            }
            Err(error) => Cleanup::error(error),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Owner {
    owner: process::ChildIdentity,
    leader: Option<process::ChildIdentity>,
}

pub(super) fn allocate(root: &Path) -> io::Result<(platform::PrivateTempDir, std::fs::File)> {
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
        let path = entry.path();
        // Through the platform's no-follow classification: a link is not a directory.
        if !matches!(platform::path_kind(&path)?, platform::FileKind::Directory) {
            continue;
        }
        let record = match std::fs::read(path.join("owner.json")) {
            Ok(bytes) => Some(serde_json::from_slice::<Owner>(&bytes).map_err(io::Error::other)?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        if let Some(record) = record {
            if process::is_alive(Some(record.owner))? {
                match platform::private_options()
                    .read(true)
                    .write(true)
                    .open(path.join("lease"))
                {
                    Ok(lease) => match lease.try_lock() {
                        Err(std::fs::TryLockError::WouldBlock) => continue,
                        Err(std::fs::TryLockError::Error(error)) => return Err(error),
                        Ok(()) => {}
                    },
                    // Old session directories had no lease. Their live owner is
                    // still authoritative; dead/zombie owners are always swept.
                    Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                    Err(error) => return Err(error),
                }
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
    let lease = platform::private_options()
        .read(true)
        .write(true)
        .create_new(true)
        .open(directory.path().join("lease"))?;
    lease.lock()?;
    save_owner(directory.path(), None)?;
    Ok((directory, lease))
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
