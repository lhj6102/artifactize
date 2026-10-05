//! The key history: every completed GREEN/RED record per reuse key. The latest by completion
//! time is the one reused.

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

/// The active-key index and the key history, shared by creation and the version 4 migration.
/// `completed_at` is the record's completion time in sortable form.
pub(super) const SCHEMA: &str = "CREATE UNIQUE INDEX IF NOT EXISTS active_key ON executions(key) WHERE key IS NOT NULL AND status IN ('RUNNING','WAITING_HUMAN');
    CREATE TABLE IF NOT EXISTS cache_entries(execution_id TEXT PRIMARY KEY REFERENCES executions(id), key TEXT NOT NULL, eval_def_hash TEXT NOT NULL, completed_at TEXT NOT NULL, bytes INTEGER NOT NULL, last_used TEXT NOT NULL);
    CREATE INDEX IF NOT EXISTS cache_key ON cache_entries(key,completed_at);
    CREATE INDEX IF NOT EXISTS cache_definition ON cache_entries(eval_def_hash);
    CREATE INDEX IF NOT EXISTS cache_lru ON cache_entries(last_used,key);";

/// The latest record of a key first: the newest completion, then the most recently stored.
pub(super) const LATEST: &str = "c.completed_at DESC, c.rowid DESC";

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

#[derive(Debug, Default)]
pub struct GcResult {
    pub removed_entries: i64,
    pub removed_bytes: i64,
    pub remaining_entries: i64,
    pub remaining_bytes: i64,
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
        let mut statement = db.prepare(&format!("SELECT * FROM (SELECT c.key,c.eval_def_hash,c.execution_id,e.status,json_extract(e.data,'$.provenance.repoPath'),json_extract(e.data,'$.provenance.evalId'),json_extract(e.data,'$.fingerprint'),json_extract(e.data,'$.completedAt'),json_extract(e.data,'$.producer.name'),json_extract(e.data,'$.options.variant'),json_extract(e.data,'$.origin.store'),count(*) OVER (PARTITION BY c.key),c.bytes,c.last_used,row_number() OVER (PARTITION BY c.key ORDER BY {LATEST}) AS rank FROM cache_entries c JOIN executions e ON e.id=c.execution_id WHERE e.status IN ('GREEN','RED')) WHERE ?1 OR rank=1 ORDER BY 1,rank"))?;
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
            let mut statement = db.prepare(&format!("SELECT e.data FROM cache_entries c JOIN executions e ON e.id=c.execution_id WHERE c.key=? AND e.status IN ('GREEN','RED') ORDER BY {LATEST} LIMIT ?"))?;
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
            let mut statement = db.prepare(&format!("SELECT data FROM (SELECT c.key,e.data,row_number() OVER (PARTITION BY c.key ORDER BY {LATEST}) AS rank FROM cache_entries c JOIN executions e ON e.id=c.execution_id WHERE e.status IN ('GREEN','RED') AND json_extract(e.data,'$.origin') IS NULL AND c.key>?) WHERE rank=1 ORDER BY key LIMIT ?"))?;
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

/// Remove every record of an unused key, preserving executions and Runs.
pub async fn remove(state: &Path, key: &str) -> Result<bool, String> {
    let Some(connection) = open(state, true).await? else {
        return Ok(false);
    };
    let key = key.to_owned();
    connection.call(move |db| -> Result<bool, Error> {
        let transaction = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let in_use: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM executions WHERE key=?1 AND status IN ('RUNNING','WAITING_HUMAN')) OR EXISTS(SELECT 1 FROM requests q JOIN cache_entries c ON c.execution_id=q.execution_id WHERE c.key=?1 AND q.status IN ('QUEUED','RUNNING','WAITING_HUMAN'))",
            [&key], |row| row.get(0),
        )?;
        if in_use {
            return Err(Error::Invalid(format!("Key {key} is in use by an active execution or waiter.")));
        }
        let removed = transaction.execute("DELETE FROM cache_entries WHERE key=?", [&key])?;
        transaction.commit()?;
        Ok(removed != 0)
    }).await.map_err(|e| e.to_string())
}

/// Append a completed record to its key's history; false when it is over the entry limit.
pub(super) fn append(
    db: &rusqlite::Connection,
    execution: &Execution,
    bytes: usize,
    last_used: &str,
) -> Result<bool, Error> {
    let (Some(key), Some(_), Some(completed_at)) =
        (&execution.key, execution.verdict(), &execution.completed_at)
    else {
        return Ok(false);
    };
    if bytes > MAX_ENTRY_BYTES {
        return Ok(false);
    }
    let completed_at = crate::broker::sortable(completed_at)
        .ok_or_else(|| Error::Invalid(format!("Invalid completion time {completed_at}.")))?;
    Ok(db.execute(
        "INSERT INTO cache_entries(execution_id,key,eval_def_hash,completed_at,bytes,last_used) VALUES (?,?,?,?,?,?) ON CONFLICT(execution_id) DO NOTHING",
        params![execution.id, key, execution.eval_def_hash, completed_at, bytes as i64, last_used],
    )? != 0)
}

/// Evict least-recently-used records above the caps, skipping keys with an active execution
/// and records an in-flight request waits for.
pub(super) fn collect(db: &mut rusqlite::Connection) -> Result<GcResult, Error> {
    let mut result = GcResult::default();
    loop {
        // Each eviction has its own short transaction; execution and receipt rows stay intact.
        let transaction = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        (result.remaining_entries, result.remaining_bytes) = transaction.query_row(
            "SELECT count(*),coalesce(sum(bytes),0) FROM cache_entries",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let over_capacity =
            result.remaining_entries > MAX_ENTRIES || result.remaining_bytes > MAX_BYTES;
        let candidate: Option<(String, i64)> = transaction
            .query_row(
                "SELECT c.execution_id,c.bytes FROM cache_entries c
             WHERE (? OR c.bytes>?)
             AND NOT EXISTS(SELECT 1 FROM executions a WHERE a.key=c.key AND a.status IN ('RUNNING','WAITING_HUMAN'))
             AND NOT EXISTS(SELECT 1 FROM requests q WHERE q.execution_id=c.execution_id AND q.status IN ('QUEUED','RUNNING','WAITING_HUMAN'))
             ORDER BY c.last_used,c.key,c.completed_at LIMIT 1",
                params![over_capacity, MAX_ENTRY_BYTES as i64],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((execution_id, bytes)) = candidate else {
            transaction.commit()?;
            return Ok(result);
        };
        transaction.execute(
            "DELETE FROM cache_entries WHERE execution_id=?",
            [execution_id],
        )?;
        transaction.commit()?;
        result.removed_entries += 1;
        result.removed_bytes += bytes;
    }
}
