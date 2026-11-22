//! Durable admission journals. Removal requires confirmed process cleanup.
use super::ownership::Cleanup;
use crate::{platform, process};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    ffi::OsStr,
    fs::File,
    io::{self, BufReader},
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
        // The staged file is still open here.
        platform::paths::rename_open_file(&pending, &path)?;
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
            RecordKind::Invocation => crate::platform::paths::remove_open_file(
                &self.directory.join(format!("process-{}.json", child.pid)),
            )
            .or_else(|error| {
                if error.kind() == io::ErrorKind::NotFound {
                    Ok(())
                } else {
                    Err(error)
                }
            })
            .and_then(|()| platform::sync_dir(&self.directory)),
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

fn read_record<T: DeserializeOwned>(directory: &File, name: &OsStr) -> io::Result<T> {
    let file = platform::paths::open_regular_entry(directory, name)?;
    serde_json::from_reader(BufReader::new(file)).map_err(io::Error::other)
}

pub(super) fn allocate(root: &Path) -> io::Result<(platform::PrivateTempDir, File)> {
    platform::create_private_dir_all(root)?;
    let lock = platform::open_no_follow(
        platform::private_options()
            .read(true)
            .write(true)
            .create(true),
        &root.join("lock"),
    )?;
    crate::platform::paths::lock(&lock)?;
    // The allocation lock makes an owner-less directory unambiguously abandoned.
    let sessions = platform::open_directory(root)?;
    for entry in platform::read_dir(&sessions)? {
        let entry = entry?;
        if entry.file_type()? != platform::FileKind::Directory {
            continue;
        }
        let name = entry.file_name();
        let directory = platform::paths::open_named(&sessions, &name)?;
        let path = root.join(&name);
        let record = match read_record::<Owner>(&directory, OsStr::new("owner.json")) {
            Ok(record) => Some(record),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        if let Some(record) = record {
            if process::is_alive(Some(record.owner))? {
                match platform::open_no_follow(
                    platform::private_options().read(true).write(true),
                    &path.join("lease"),
                ) {
                    Ok(lease) => match crate::platform::paths::try_lock(&lease) {
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
            for entry in platform::read_dir(&directory)? {
                let entry = entry?;
                let name = entry.file_name();
                if !name.as_encoded_bytes().starts_with(b"process-")
                    || Path::new(&name).extension() != Some(OsStr::new("json"))
                {
                    continue;
                }
                let child = read_record(&directory, name.as_ref())?;
                platform::kill_saved_group(child)?;
            }
        }
        drop(directory);
        match platform::remove_tree(&path) {
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
    crate::platform::paths::lock(&lease)?;
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
    let mut file =
        platform::open_no_follow(platform::private_options().write(true).create(true), &path)?;
    // Clear a leftover pending record only after the no-follow open has checked it.
    file.set_len(0)?;
    std::io::Write::write_all(&mut file, &bytes)?;
    file.sync_all()?;
    platform::paths::rename_open_file(&path, &directory.join("owner.json"))?;
    platform::sync_dir(directory)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_os;
    use std::fs;

    #[test]
    fn recovery_refuses_linked_owner_and_process_records() {
        let fixture = test_os::tempdir();
        let root = fixture.path().join("sessions");
        let abandoned = root.join("abandoned");
        platform::create_private_dir_all(&abandoned).unwrap();
        let owner = process::child_identity(std::process::id()).unwrap();
        let owner_record = abandoned.join("owner.json");
        let outside = fixture.path().join("outside.json");
        fs::write(
            &outside,
            serde_json::to_vec(&Owner {
                owner,
                leader: None,
            })
            .unwrap(),
        )
        .unwrap();
        test_os::file_link(&outside, &owner_record);
        let Err(error) = allocate(&root) else {
            panic!("a linked owner journal was accepted");
        };
        assert!(artifactize_tools::files::is_link_refusal(&error), "{error}");
        assert!(abandoned.is_dir(), "a refused record is retained");

        test_os::remove_file_link(&owner_record);
        save_owner(&abandoned, None).unwrap();
        // No process holds this lease, so recovery proceeds even though its owner is live.
        fs::write(abandoned.join("lease"), "").unwrap();
        let old_identity = process::ChildIdentity {
            start_time: owner.start_time ^ 1,
            ..owner
        };
        fs::write(&outside, serde_json::to_vec(&old_identity).unwrap()).unwrap();
        test_os::file_link(&outside, &abandoned.join("process-old.json"));
        let Err(error) = allocate(&root) else {
            panic!("a linked process journal was accepted");
        };
        assert!(artifactize_tools::files::is_link_refusal(&error), "{error}");
        assert!(abandoned.is_dir(), "a refused record is retained");
        assert!(outside.is_file(), "recovery leaves the link target alone");
    }
}
