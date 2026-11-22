//! A fingerprint script's scratch directory, held by a lock beside it while its process runs.
//! A process that ended without removing its directory, killed for instance, leaves the lock
//! free, and the next fingerprint of the same state removes what it left.
use std::{
    collections::BTreeSet,
    fs::File,
    io,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, SystemTime},
};

use crate::platform::{self, EntryName, FileKind};

const PREFIX: &str = "fingerprint-";
const LOCK: &str = ".lock";

/// A directory without a lock comes from a build before locks, which may still be running;
/// none of its fingerprint scripts outlives this.
const UNLOCKED_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// A fingerprint's own directory, removed with its lock when dropped.
pub(super) struct Scratch {
    directory: Option<platform::PrivateTempDir>,
    lock: PathBuf,
    /// Held until the directory is gone.
    _held: File,
}

impl Scratch {
    /// Lock a new name in `root`, then make its directory, after removing what ended
    /// processes left there.
    pub(super) fn create(root: &Path) -> io::Result<Self> {
        sweep_once(root);
        let (held, lock) = tempfile::Builder::new()
            .prefix(PREFIX)
            .suffix(LOCK)
            .tempfile_in(root)?
            .keep()
            .map_err(|error| error.error)?;
        let directory = (|| {
            crate::platform::paths::lock(&held)?;
            let name = lock
                .file_name()
                .and_then(|name| crate::platform::os_text(&name))
                .and_then(|name| name.strip_suffix(LOCK))
                .expect("the lock was named with its suffix");
            platform::private_named_dir(root, name)
        })();
        match directory {
            Ok(directory) => Ok(Self {
                directory: Some(directory),
                lock,
                _held: held,
            }),
            Err(error) => {
                let _ = platform::paths::remove_open_file(&lock);
                Err(error)
            }
        }
    }

    pub(super) fn path(&self) -> &Path {
        self.directory.as_ref().expect("open until closed").path()
    }

    /// Remove the directory, saying whether that worked; the lock goes with the value.
    pub(super) fn close(mut self) -> io::Result<()> {
        self.directory.take().expect("closed once").close()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // The directory first: a lock without its directory is only ever a free lock.
        drop(self.directory.take());
        let _ = platform::paths::remove_open_file(&self.lock);
    }
}

/// Sweep `root` the first time this process makes a fingerprint directory in it.
fn sweep_once(root: &Path) {
    static SWEPT: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());
    if SWEPT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(root.to_path_buf())
    {
        // Best effort: what cannot be removed now stays for the next process to try.
        let _ = sweep(root, SystemTime::now(), |file| file.metadata()?.modified());
    }
}

/// Remove from `root` every fingerprint directory and lock whose process ended, and every
/// directory without a lock untouched for [`UNLOCKED_AGE`] before `now`.
fn sweep(
    root: &Path,
    now: SystemTime,
    modified_at: impl Fn(&File) -> io::Result<SystemTime>,
) -> io::Result<()> {
    let directory = platform::open_directory(root)?;
    let mut directories = BTreeSet::new();
    let mut locks = BTreeSet::new();
    for entry in platform::read_dir(&directory)? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some(rest) = name.strip_prefix(PREFIX) else {
            continue;
        };
        // Links and reparse points are never followed, nor removed.
        match (rest.strip_suffix(LOCK), entry.file_type()?) {
            (Some(_), FileKind::File) => {
                locks.insert(name);
            }
            (None, FileKind::Directory) => {
                directories.insert(name);
            }
            _ => {}
        }
    }
    for lock in &locks {
        let path = root.join(lock);
        let Ok(file) = platform::open_no_follow(File::options().read(true).write(true), &path)
        else {
            continue;
        };
        // Held: its process still runs.
        if crate::platform::paths::try_lock(&file).is_err() {
            continue;
        }
        let owned = lock.strip_suffix(LOCK).expect("a lock's name");
        if directories.remove(owned) {
            match platform::remove_tree(&root.join(owned)) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(_) => continue,
            }
        }
        let _ = platform::paths::remove_open_file(&path);
    }
    for name in directories {
        if locks.contains(&format!("{name}{LOCK}")) {
            continue;
        }
        let untouched = EntryName::new(name.as_ref())
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))
            .and_then(|entry| platform::open_entry(&directory, &entry))
            .and_then(|opened| modified_at(&opened))
            .is_ok_and(|modified| {
                now.duration_since(modified)
                    .is_ok_and(|age| age >= UNLOCKED_AGE)
            });
        if untouched {
            let _ = platform::remove_tree(&root.join(name));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn a_held_directory_stays_and_an_ended_one_is_removed() {
        let root = crate::test_os::tempdir();
        let held = Scratch::create(root.path()).unwrap();
        fs::write(held.path().join("output"), "in use").unwrap();
        // What a killed process leaves: a directory and its lock, nobody holding it.
        let ended_directory = root.path().join("fingerprint-killed");
        let ended_lock = root.path().join("fingerprint-killed.lock");
        fs::create_dir_all(ended_directory.join("runtime-x/tmp")).unwrap();
        fs::write(&ended_lock, "").unwrap();
        sweep(root.path(), SystemTime::UNIX_EPOCH, |_| {
            panic!("only directories without a lock need a modification time")
        })
        .unwrap();
        assert!(held.path().join("output").exists());
        assert!(!ended_directory.exists());
        assert!(!ended_lock.exists());
        let lock = held.lock.clone();
        held.close().unwrap();
        assert!(!lock.exists(), "the lock goes with its directory");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_directory_without_a_lock_is_removed_once_untouched_for_a_day() {
        let root = crate::test_os::tempdir();
        let unlocked = root.path().join("fingerprint-before");
        fs::create_dir(&unlocked).unwrap();
        let other = root.path().join("runtime-other");
        fs::create_dir(&other).unwrap();
        let modified = SystemTime::UNIX_EPOCH;
        let modified_at = |_: &File| Ok(modified);
        sweep(
            root.path(),
            modified + UNLOCKED_AGE - Duration::from_secs(1),
            modified_at,
        )
        .unwrap();
        assert!(
            unlocked.exists(),
            "it may belong to an older build still running"
        );
        sweep(root.path(), modified + UNLOCKED_AGE, modified_at).unwrap();
        assert!(!unlocked.exists());
        assert!(other.exists(), "only fingerprint directories are swept");
    }
}
