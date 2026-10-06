//! The key history: every completed GREEN/RED execution of a reuse key whose history columns
//! (`completed_at`, `bytes`, `last_used`) are set. The latest by completion time is the one
//! reused; the cache GC and `cache rm` clear the columns and keep the execution.

use std::{path::Path, time::Duration};

use rusqlite::{OpenFlags, OptionalExtension, params};
use serde::Serialize;
use tokio_rusqlite::Connection;

use super::{
    DATABASE, Execution,
    receipts::{Error, schema_initialized},
};

pub const MAX_ENTRIES: i64 = 10_000;
pub const MAX_BYTES: i64 = 1024 * 1024 * 1024;
pub const MAX_ENTRY_BYTES: usize = 16 * 1024 * 1024;

/// A record of the key history, of executions `e`.
pub(super) const RECORD: &str =
    "e.completed_at IS NOT NULL AND e.status IN ('GREEN','RED') AND e.key IS NOT NULL";

/// The latest record of a key first: the newest completion, then the most recently stored.
pub(super) const LATEST: &str = "e.completed_at DESC, e.rowid DESC";

/// One record of a key's history, or with `cache list` the latest record of each key.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub key: String,
    pub eval_def_hash: String,
    pub execution_id: String,
    pub verdict: String,
    pub repo_path: String,
    pub eval_id: String,
    /// The target Artifact's fingerprint.
    pub fingerprint: Option<String>,
    pub completed_at: Option<String>,
    /// `user@host` of the machine that produced the result.
    pub producer: Option<String>,
    /// The selected profile variant, when the result came from one.
    pub variant: Option<String>,
    /// The remote store URL of a mirrored record.
    pub origin: Option<String>,
    /// How many records the key holds.
    pub records: i64,
    pub bytes: i64,
    pub last_used: String,
}

async fn open(state: &Path, writable: bool) -> Result<Option<Connection>, String> {
    let state = crate::workspace::canonical_target(state).map_err(|e| e.to_string())?;
    super::receipts::check_files(&state)?;
    let path = state.join(DATABASE);
    if !path.try_exists().map_err(|e| e.to_string())? {
        return Ok(None);
    }
    let connection = Connection::open_with_flags(
        path,
        if writable {
            OpenFlags::SQLITE_OPEN_READ_WRITE
        } else {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        },
    )
    .await
    .map_err(|e| e.to_string())?;
    let initialized = connection
        .call(|db| -> Result<bool, Error> {
            db.busy_timeout(Duration::from_secs(5))?;
            let transaction = db.transaction()?;
            schema_initialized(&transaction)
        })
        .await
        .map_err(|e| e.to_string())?;
    Ok(initialized.then_some(connection))
}

/// The latest record of every key, or with `history` every record, latest first per key.
pub async fn list(state: &Path, history: bool) -> Result<Vec<Entry>, String> {
    let Some(connection) = open(state, false).await? else {
        return Ok(Vec::new());
    };
    connection.call(move |db| -> Result<_, Error> {
        let mut statement = db.prepare(&format!("SELECT * FROM (SELECT e.key,e.eval_def_hash,e.id,e.status,json_extract(e.data,'$.provenance.repoPath'),json_extract(e.data,'$.provenance.evalId'),json_extract(e.data,'$.fingerprint'),json_extract(e.data,'$.completedAt'),json_extract(e.data,'$.producer.name'),json_extract(e.data,'$.options.variant'),json_extract(e.data,'$.origin.store'),count(*) OVER (PARTITION BY e.key),e.bytes,e.last_used,row_number() OVER (PARTITION BY e.key ORDER BY {LATEST}) AS rank FROM executions e WHERE {RECORD}) WHERE ?1 OR rank=1 ORDER BY 1,rank"))?;
        Ok(statement.query_map([history], |row| Ok(Entry {
            key: row.get(0)?, eval_def_hash: row.get(1)?, execution_id: row.get(2)?,
            verdict: row.get(3)?, repo_path: row.get(4)?, eval_id: row.get(5)?,
            fingerprint: row.get(6)?, completed_at: row.get(7)?, producer: row.get(8)?,
            variant: row.get(9)?, origin: row.get(10)?, records: row.get(11)?,
            bytes: row.get(12)?, last_used: row.get(13)?,
        }))?.collect::<Result<_, _>>()?)
    }).await.map_err(|e| e.to_string())
}

/// A key's records, latest first; without `history` only the latest.
pub async fn show(state: &Path, key: &str, history: bool) -> Result<Vec<Execution>, String> {
    let Some(connection) = open(state, false).await? else {
        return Ok(Vec::new());
    };
    let key = key.to_owned();
    connection
        .call(move |db| -> Result<_, Error> {
            let mut statement = db.prepare(&format!(
                "SELECT e.data FROM executions e WHERE e.key=? AND {RECORD} ORDER BY {LATEST} LIMIT ?"
            ))?;
            statement
                .query_map(params![key, if history { -1 } else { 1 }], |row| {
                    row.get::<_, String>(0)
                })?
                .map(|row| Ok(serde_json::from_str(&row?)?))
                .collect()
        })
        .await
        .map_err(|e| e.to_string())
}

/// The latest locally produced record (never a mirror) of each key after `after`, in key order.
pub async fn local(
    state: &Path,
    after: Option<String>,
    limit: usize,
) -> Result<Vec<Execution>, String> {
    let Some(connection) = open(state, false).await? else {
        return Ok(Vec::new());
    };
    connection
        .call(move |db| -> Result<_, Error> {
            let mut statement = db.prepare(&format!("SELECT data FROM (SELECT e.key,e.data,row_number() OVER (PARTITION BY e.key ORDER BY {LATEST}) AS rank FROM executions e WHERE {RECORD} AND json_extract(e.data,'$.origin') IS NULL AND e.key>?) WHERE rank=1 ORDER BY key LIMIT ?"))?;
            statement
                .query_map(params![after.unwrap_or_default(), limit as i64], |row| {
                    row.get::<_, String>(0)
                })?
                .map(|row| Ok(serde_json::from_str(&row?)?))
                .collect()
        })
        .await
        .map_err(|e| e.to_string())
}

/// Remove every record of an unused key from its history, preserving executions and Runs.
pub async fn remove(state: &Path, key: &str) -> Result<bool, String> {
    let Some(connection) = open(state, true).await? else {
        return Ok(false);
    };
    let key = key.to_owned();
    connection.call(move |db| -> Result<bool, Error> {
        let transaction = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let in_use: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM executions WHERE key=?1 AND status IN ('RUNNING','WAITING_HUMAN')) OR EXISTS(SELECT 1 FROM requests q JOIN executions e ON e.id=q.execution_id WHERE e.key=?1 AND e.completed_at IS NOT NULL AND q.status IN ('QUEUED','RUNNING','WAITING_HUMAN'))",
            [&key], |row| row.get(0),
        )?;
        if in_use {
            return Err(Error::Invalid(format!("Key {key} is in use by an active execution or waiter.")));
        }
        let removed = transaction.execute(
            "UPDATE executions SET completed_at=NULL,bytes=NULL,last_used=NULL WHERE key=? AND completed_at IS NOT NULL",
            [&key],
        )?;
        transaction.commit()?;
        Ok(removed != 0)
    }).await.map_err(|e| e.to_string())
}

/// The history columns of a completed record: its sortable completion time and size, or
/// `None` when it does not join its key's history (no key, no verdict, or over the entry
/// limit).
pub(super) fn columns(execution: &Execution, bytes: usize) -> Result<Option<(String, i64)>, Error> {
    let (Some(_), Some(_), Some(completed_at)) =
        (&execution.key, execution.verdict(), &execution.completed_at)
    else {
        return Ok(None);
    };
    if bytes > MAX_ENTRY_BYTES {
        return Ok(None);
    }
    let completed_at = crate::broker::sortable(completed_at)
        .ok_or_else(|| Error::Invalid(format!("Invalid completion time {completed_at}.")))?;
    Ok(Some((completed_at, bytes as i64)))
}

/// Mark a reused record as used now.
pub(super) fn touch(db: &rusqlite::Connection, execution_id: &str, at: &str) -> Result<(), Error> {
    db.execute(
        "UPDATE executions SET last_used=? WHERE id=? AND completed_at IS NOT NULL",
        params![at, execution_id],
    )?;
    Ok(())
}

/// Report a failed collection; the completed execution it followed is saved already.
pub(super) fn collect_after(db: &mut rusqlite::Connection) {
    if let Err(error) = collect(db) {
        use std::io::Write;
        let _ = writeln!(std::io::stderr().lock(), "Cache GC failed: {error}");
    }
}

/// Drop least-recently-used records from the history while it is above its caps, and records
/// over the entry limit, skipping keys with an active execution and records an in-flight
/// request waits for.
fn collect(db: &mut rusqlite::Connection) -> Result<(), Error> {
    loop {
        // Each eviction has its own short transaction; execution and receipt rows stay.
        let transaction = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let (entries, bytes): (i64, i64) = transaction.query_row(
            "SELECT count(*),coalesce(sum(bytes),0) FROM executions WHERE completed_at IS NOT NULL",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let over_capacity = entries > MAX_ENTRIES || bytes > MAX_BYTES;
        let candidate: Option<String> = transaction
            .query_row(
                "SELECT e.id FROM executions e WHERE e.completed_at IS NOT NULL AND (? OR e.bytes>?)
             AND NOT EXISTS(SELECT 1 FROM executions a WHERE a.key=e.key AND a.status IN ('RUNNING','WAITING_HUMAN'))
             AND NOT EXISTS(SELECT 1 FROM requests q WHERE q.execution_id=e.id AND q.status IN ('QUEUED','RUNNING','WAITING_HUMAN'))
             ORDER BY e.last_used,e.key,e.completed_at LIMIT 1",
                params![over_capacity, MAX_ENTRY_BYTES as i64],
                |row| row.get(0),
            )
            .optional()?;
        let Some(execution_id) = candidate else {
            return Ok(transaction.commit()?);
        };
        transaction.execute(
            "UPDATE executions SET completed_at=NULL,bytes=NULL,last_used=NULL WHERE id=?",
            [execution_id],
        )?;
        transaction.commit()?;
    }
}
