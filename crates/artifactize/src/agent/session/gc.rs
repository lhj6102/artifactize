//! Size-bounded collection of the session store, with hysteresis: once the conversations
//! exceed `maxBytes`, the oldest (by last write) are deleted until they take `targetBytes` or
//! less. A session whose request is still RUNNING, or that a send holds, is never deleted.

use std::{
    collections::BTreeSet,
    fs::{self, File},
    path::Path,
    time::SystemTime,
};

use rusqlite::OpenFlags;
use serde::Serialize;

use super::directory;
use crate::{limits::AgentSessions, store::DATABASE, types::SessionId};

/// What one collection found and did.
#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    /// The session store's size before the collection.
    pub bytes: u64,
    /// Its size after it, or with a dry run, after the deletions it reports.
    pub remaining: u64,
    /// The sessions deleted, or with a dry run those that would be, oldest first.
    pub removed: Vec<String>,
}

/// The session store's size.
#[derive(Debug, Default, Clone, Copy, Serialize)]
pub struct Usage {
    pub sessions: usize,
    pub bytes: u64,
}

struct Entry {
    id: SessionId,
    bytes: u64,
    written: SystemTime,
}

/// The saved conversations, read without following links; a missing store is empty.
fn entries(state: &Path) -> Result<Vec<Entry>, String> {
    let directory = directory(state);
    let handle = match crate::platform::open_no_follow(File::options().read(true), &directory) {
        Ok(handle) => handle,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(format!(
                "{}: {error}",
                crate::platform::path_text(&directory)
            ));
        }
    };
    let listing = crate::platform::read_dir(&handle).map_err(|e| e.to_string())?;
    let mut entries = Vec::new();
    for entry in listing {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let Some(id) = name.to_str().and_then(|name| name.strip_suffix(".jsonl")) else {
            continue;
        };
        let Ok(id) = id.parse::<SessionId>() else {
            // Not a session path; never collect arbitrary files in this directory.
            continue;
        };
        if crate::platform::entry_kind(&handle, &name).map_err(|e| e.to_string())?
            != crate::platform::FileKind::File
        {
            continue;
        }
        let name = crate::platform::EntryName::new(&name).ok_or("Invalid session entry name.")?;
        let file = crate::platform::open_entry(&handle, &name).map_err(|e| e.to_string())?;
        let metadata = file.metadata().map_err(|e| e.to_string())?;
        if !metadata.is_file() {
            continue;
        }
        entries.push(Entry {
            id,
            bytes: metadata.len(),
            written: metadata.modified().map_err(|e| e.to_string())?,
        });
    }
    Ok(entries)
}

/// Pure ordering shared by collection and the explicit-time contract test.
fn order(entries: &mut [Entry]) {
    entries.sort_by(|a, b| a.written.cmp(&b.written).then_with(|| a.id.cmp(&b.id)));
}

/// How many conversations the store holds and their bytes.
pub fn usage(state: &Path) -> Result<Usage, String> {
    let entries = entries(state)?;
    Ok(Usage {
        sessions: entries.len(),
        bytes: entries.iter().map(|entry| entry.bytes).sum(),
    })
}

/// The sessions of requests that are still RUNNING, read without a writer lock.
fn running(state: &Path) -> Result<BTreeSet<SessionId>, String> {
    let database = state.join(DATABASE);
    if !database.try_exists().map_err(|e| e.to_string())? {
        return Ok(BTreeSet::new());
    }
    let read = || -> Result<BTreeSet<SessionId>, rusqlite::Error> {
        let db =
            rusqlite::Connection::open_with_flags(&database, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        db.busy_timeout(crate::store::SQLITE_BUSY_TIMEOUT)?;
        let initialized: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='requests')",
            [],
            |row| row.get(0),
        )?;
        if !initialized {
            return Ok(BTreeSet::new());
        }
        let mut statement = db.prepare(
            "SELECT json_extract(data,'$.sessionId') FROM requests WHERE status='RUNNING' AND json_extract(data,'$.sessionId') IS NOT NULL",
        )?;
        statement
            .query_map([], |row| row.get::<_, SessionId>(0))?
            .collect()
    };
    read().map_err(|e| format!("Cannot read running Agent reviews: {e}"))
}

/// Collect the store under `bounds` (see the module). The sessions that are listed first and
/// then found RUNNING were saved after their request turned RUNNING, so a review that starts
/// during a collection is either seen as running or not listed at all.
pub fn collect(state: &Path, bounds: AgentSessions, dry_run: bool) -> Result<Collection, String> {
    let publisher = if dry_run {
        crate::changes::Publisher::default()
    } else {
        crate::changes::Publisher::new(state)
    };
    let mut entries = entries(state)?;
    let bytes = entries.iter().map(|entry| entry.bytes).sum();
    let mut collection = Collection {
        bytes,
        remaining: bytes,
        removed: Vec::new(),
    };
    if bytes <= bounds.max_bytes {
        return Ok(collection);
    }
    let running = running(state)?;
    order(&mut entries);
    let directory = directory(state);
    for entry in entries {
        if collection.remaining <= bounds.target_bytes {
            break;
        }
        if running.contains(&entry.id) {
            continue;
        }
        if !dry_run {
            let lock = directory.join(format!("{}.lock", entry.id));
            // A session a send holds stays; its lock goes with it.
            let held = match crate::platform::open_no_follow(
                File::options().read(true).write(true),
                &lock,
            ) {
                Ok(file) => match file.try_lock() {
                    Ok(()) => Some(file),
                    Err(std::fs::TryLockError::WouldBlock) => continue,
                    Err(std::fs::TryLockError::Error(error)) => return Err(error.to_string()),
                },
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(error.to_string()),
            };
            match fs::remove_file(directory.join(format!("{}.jsonl", entry.id))) {
                Ok(()) => {
                    publisher.notify(crate::changes::Change::SessionInvalidated(entry.id.clone()))
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!("Cannot remove Agent session {}: {error}", entry.id));
                }
            }
            if held.is_some() {
                let _ = fs::remove_file(&lock);
            }
        }
        collection.remaining = collection.remaining.saturating_sub(entry.bytes);
        collection.removed.push(entry.id.into());
    }
    Ok(collection)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;

    /// Sessions with equal explicit write times; identity provides deterministic tie order.
    fn store(sizes: &[(&str, usize)]) -> tempfile::TempDir {
        let state = crate::test_os::tempdir();
        let sessions = directory(state.path());
        fs::create_dir_all(&sessions).unwrap();
        for (id, bytes) in sizes {
            let path = sessions.join(format!("{id}.jsonl"));
            fs::write(&path, vec![b'x'; *bytes]).unwrap();
            crate::test_os::set_modified(&path, UNIX_EPOCH + Duration::from_secs(1_000));
        }
        state
    }

    #[test]
    fn ordering_uses_explicit_times_then_identity() {
        let entry = |id: &str, secs| Entry {
            id: id.parse().unwrap(),
            bytes: 1,
            written: UNIX_EPOCH + Duration::from_secs(secs),
        };
        let mut entries = [entry("a", 20), entry("c", 10), entry("b", 10)];
        order(&mut entries);
        assert_eq!(entries.map(|entry| entry.id.to_string()), ["b", "c", "a"]);
    }

    fn bounds(max_bytes: u64, target_bytes: u64) -> AgentSessions {
        AgentSessions {
            enabled: true,
            max_bytes,
            target_bytes,
        }
    }

    #[test]
    fn collects_oldest_first_down_to_the_target_only_past_the_maximum() {
        let state = store(&[("a", 100), ("b", 100), ("c", 100), ("d", 100)]);
        // At the maximum, nothing is collected.
        let collection = collect(state.path(), bounds(400, 150), false).unwrap();
        assert_eq!(collection.removed, Vec::<String>::new());
        assert_eq!(usage(state.path()).unwrap().sessions, 4);

        let dry = collect(state.path(), bounds(399, 150), true).unwrap();
        assert_eq!(dry.removed, ["a", "b", "c"]);
        assert_eq!((dry.bytes, dry.remaining), (400, 100));
        assert_eq!(usage(state.path()).unwrap().sessions, 4);

        // A held lock keeps its session.
        let lock = directory(state.path()).join("b.lock");
        let held = File::create(&lock).unwrap();
        held.lock().unwrap();
        let collection = collect(state.path(), bounds(399, 200), false).unwrap();
        assert_eq!(collection.removed, ["a", "c"]);
        assert_eq!(collection.remaining, 200);
        drop(held);
        let left: BTreeSet<_> = fs::read_dir(directory(state.path()))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(
            left,
            ["b.jsonl", "b.lock", "d.jsonl"]
                .map(str::to_owned)
                .into_iter()
                .collect()
        );
    }

    #[test]
    fn invalid_session_filenames_are_not_collected() {
        let state = store(&[("valid", 100), ("..", 100), ("bad name", 100)]);
        assert_eq!(usage(state.path()).unwrap().sessions, 1);
        let collection = collect(state.path(), bounds(1, 0), false).unwrap();
        assert_eq!(collection.removed, ["valid"]);
        assert!(directory(state.path()).join("...jsonl").is_file());
        assert!(directory(state.path()).join("bad name.jsonl").is_file());
    }

    #[test]
    fn running_session_ids_are_validated_at_the_sql_edge() {
        let state = store(&[("active", 100), ("finished", 100)]);
        let db = rusqlite::Connection::open(state.path().join(DATABASE)).unwrap();
        db.execute_batch(
            "CREATE TABLE requests(status TEXT, data TEXT);
            INSERT INTO requests VALUES('RUNNING','{\"sessionId\":\"active\"}');",
        )
        .unwrap();
        let collection = collect(state.path(), bounds(1, 0), false).unwrap();
        assert_eq!(collection.removed, ["finished"]);
        db.execute_batch("UPDATE requests SET data='{\"sessionId\":\"../invalid\"}';")
            .unwrap();
        assert!(running(state.path()).is_err());
        assert!(directory(state.path()).join("active.jsonl").is_file());
    }

    #[test]
    fn a_missing_store_is_empty() {
        let state = crate::test_os::tempdir();
        let collection = collect(state.path(), bounds(1, 0), false).unwrap();
        assert_eq!((collection.bytes, collection.removed.len()), (0, 0));
    }
}
