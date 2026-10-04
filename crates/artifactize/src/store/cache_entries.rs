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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub stale_key: String,
    pub eval_def_hash: String,
    pub execution_id: String,
    pub verdict: String,
    pub repo_path: String,
    pub eval_id: String,
    pub bytes: i64,
    pub last_used: String,
    pub completed_at: Option<String>,
    /// The remote store URL of a mirrored entry.
    pub origin: Option<String>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
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

pub async fn list(state: &Path) -> Result<Vec<Entry>, String> {
    let Some(connection) = open(state, false).await? else {
        return Ok(Vec::new());
    };
    connection.call(|db| -> Result<_, Error> {
        let mut statement = db.prepare("SELECT c.stale_key,c.execution_id,e.status,json_extract(e.data,'$.provenance.repoPath'),json_extract(e.data,'$.provenance.evalId'),c.bytes,c.last_used,json_extract(e.data,'$.completedAt'),c.eval_def_hash,json_extract(e.data,'$.origin.store') FROM cache_entries c JOIN executions e ON e.id=c.execution_id WHERE e.status IN ('GREEN','RED') ORDER BY c.stale_key,c.eval_def_hash")?;
        Ok(statement.query_map([], |row| Ok(Entry {
            stale_key: row.get(0)?, execution_id: row.get(1)?, verdict: row.get(2)?,
            repo_path: row.get(3)?, eval_id: row.get(4)?, bytes: row.get(5)?,
            last_used: row.get(6)?, completed_at: row.get(7)?, eval_def_hash: row.get(8)?,
            origin: row.get(9)?,
        }))?.collect::<Result<_, _>>()?)
    }).await.map_err(|e| e.to_string())
}

pub async fn show(
    state: &Path,
    stale_key: &str,
    eval_def_hash: Option<&str>,
) -> Result<Option<Execution>, String> {
    let Some(connection) = open(state, false).await? else {
        return Ok(None);
    };
    let stale_key = stale_key.to_owned();
    let eval_def_hash = eval_def_hash.map(str::to_owned);
    connection
        .call(move |db| -> Result<_, Error> {
            let mut statement = db.prepare("SELECT e.data FROM cache_entries c JOIN executions e ON e.id=c.execution_id WHERE c.stale_key=?1 AND (?2 IS NULL OR c.eval_def_hash=?2) AND e.status IN ('GREEN','RED') LIMIT 2")?;
            let entries = statement.query_map(params![stale_key, eval_def_hash], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
            if entries.len() > 1 {
                return Err(Error::Invalid(format!("Stale key {stale_key} has multiple Eval definitions; specify the eval hash from cache list.")));
            }
            entries.first().map(|data| serde_json::from_str(data)).transpose().map_err(Into::into)
        })
        .await
        .map_err(|e| e.to_string())
}

/// Locally produced GREEN/RED entries (never mirrors) in key order after `after`.
pub async fn local(
    state: &Path,
    after: Option<(String, String)>,
    limit: usize,
) -> Result<Vec<Execution>, String> {
    let Some(connection) = open(state, false).await? else {
        return Ok(Vec::new());
    };
    connection
        .call(move |db| -> Result<_, Error> {
            let (stale_key, eval_def_hash) = after.unwrap_or_default();
            let mut statement = db.prepare("SELECT e.data FROM cache_entries c JOIN executions e ON e.id=c.execution_id WHERE e.status IN ('GREEN','RED') AND json_extract(e.data,'$.origin') IS NULL AND (c.stale_key,c.eval_def_hash)>(?,?) ORDER BY c.stale_key,c.eval_def_hash LIMIT ?")?;
            statement
                .query_map(params![stale_key, eval_def_hash, limit as i64], |row| {
                    row.get::<_, String>(0)
                })?
                .map(|row| Ok(serde_json::from_str(&row?)?))
                .collect()
        })
        .await
        .map_err(|e| e.to_string())
}

pub async fn remove(
    state: &Path,
    stale_key: &str,
    eval_def_hash: Option<&str>,
) -> Result<bool, String> {
    let Some(connection) = open(state, true).await? else {
        return Ok(false);
    };
    let stale_key = stale_key.to_owned();
    let eval_def_hash = eval_def_hash.map(str::to_owned);
    connection.call(move |db| -> Result<bool, Error> {
        let transaction = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let in_use: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM executions WHERE stale_key=?1 AND (?2 IS NULL OR eval_def_hash=?2) AND status IN ('RUNNING','WAITING_HUMAN')) OR EXISTS(SELECT 1 FROM requests q JOIN executions e ON e.id=q.execution_id WHERE e.stale_key=?1 AND (?2 IS NULL OR e.eval_def_hash=?2) AND q.status IN ('QUEUED','RUNNING','WAITING_HUMAN'))",
            params![stale_key, eval_def_hash], |row| row.get(0),
        )?;
        if in_use {
            return Err(Error::Invalid(format!("Stale key {stale_key} is in use by an active execution or waiter.")));
        }
        let count: i64 = transaction.query_row("SELECT count(*) FROM cache_entries WHERE stale_key=?1 AND (?2 IS NULL OR eval_def_hash=?2)", params![stale_key, eval_def_hash], |row| row.get(0))?;
        if count > 1 {
            return Err(Error::Invalid(format!("Stale key {stale_key} has multiple Eval definitions; specify the eval hash from cache list.")));
        }
        let removed = transaction.execute("DELETE FROM cache_entries WHERE stale_key=?1 AND (?2 IS NULL OR eval_def_hash=?2)", params![stale_key, eval_def_hash])?;
        transaction.commit()?;
        Ok(removed != 0)
    }).await.map_err(|e| e.to_string())
}

pub async fn gc(state: &Path) -> Result<GcResult, String> {
    let Some(connection) = open(state, true).await? else {
        return Ok(GcResult::default());
    };
    connection.call(collect).await.map_err(|e| e.to_string())
}

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
        let candidate: Option<(String, String, i64)> = transaction.query_row(
            "SELECT c.stale_key,c.eval_def_hash,c.bytes FROM cache_entries c JOIN executions e ON e.id=c.execution_id
             WHERE e.status IN ('GREEN','RED') AND (? OR c.bytes>?)
             AND NOT EXISTS(SELECT 1 FROM executions a WHERE a.stale_key=c.stale_key AND a.eval_def_hash=c.eval_def_hash AND a.status IN ('RUNNING','WAITING_HUMAN'))
             AND NOT EXISTS(SELECT 1 FROM requests q WHERE q.execution_id=c.execution_id AND q.status IN ('QUEUED','RUNNING','WAITING_HUMAN'))
             ORDER BY c.last_used,c.stale_key,c.eval_def_hash LIMIT 1",
            params![over_capacity, MAX_ENTRY_BYTES as i64],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).optional()?;
        let Some((stale_key, eval_def_hash, bytes)) = candidate else {
            transaction.commit()?;
            return Ok(result);
        };
        transaction.execute(
            "DELETE FROM cache_entries WHERE stale_key=? AND eval_def_hash=?",
            params![stale_key, eval_def_hash],
        )?;
        transaction.commit()?;
        result.removed_entries += 1;
        result.removed_bytes += bytes;
    }
}
