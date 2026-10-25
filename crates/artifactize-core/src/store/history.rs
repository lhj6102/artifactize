//! The key history: every completed GREEN/RED execution of a reuse key whose history columns
//! (`completed_at`, `bytes`, `last_used`) are set. The latest by completion time is the one
//! reused; the cache GC and `cache rm` clear the columns and keep the execution.

use std::path::Path;

use rusqlite::{OpenFlags, params};
use serde::Serialize;
use tokio_rusqlite::Connection;

use super::{
    DATABASE, Execution,
    receipts::{Error, NotifyingConnection, schema_initialized},
};

/// Bound the history no checkout reuses now: records a newer one of the same eval and checkout
/// replaced, and the records of checkouts that are gone. Eviction preserves execution audit.
pub const MAX_ENTRIES: i64 = 10_000;
/// Bound those records' JSON payload bytes independently of the record-count ceiling.
pub const MAX_BYTES: i64 = 1024 * 1024 * 1024;
/// Refuse one oversized reusable record rather than letting it monopolize cache storage.
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
    pub key: crate::types::ReuseKey,
    pub eval_def_hash: crate::types::DefinitionHash,
    pub execution_id: crate::types::ExecutionId,
    pub verdict: crate::types::ExecutionStatus,
    #[serde(with = "crate::platform::path_serde")]
    pub repo_path: std::path::PathBuf,
    pub eval_id: crate::types::EvalId,
    /// The target Artifact's fingerprint.
    pub fingerprint: Option<crate::types::Fingerprint>,
    pub completed_at: Option<crate::types::Timestamp>,
    /// `user@host` of the machine that produced the result.
    pub producer: Option<crate::types::ProducerId>,
    /// The selected profile variant, when the result came from one.
    pub variant: Option<crate::config::ProfileVariantName>,
    /// The remote store URL of a mirrored record.
    pub origin: Option<crate::types::StoreUrl>,
    /// How many records the key holds.
    pub records: i64,
    pub bytes: i64,
    pub last_used: crate::types::Timestamp,
}

async fn open(state: &Path, writable: bool) -> Result<Option<NotifyingConnection>, String> {
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
    let connection = NotifyingConnection::new(connection, &state);
    let initialized = connection
        .call(|db| -> Result<bool, Error> {
            super::wait_when_busy(db)?;
            let transaction = db.transaction()?;
            schema_initialized(&transaction)
        })
        .await
        .map_err(|e| e.to_string());
    match initialized {
        Ok(true) => Ok(Some(connection)),
        result => connection.finish(result.map(|_| None)).await,
    }
}

/// The latest record of every key, or with `history` every record, latest first per key.
pub async fn list(state: &Path, history: bool) -> Result<Vec<Entry>, String> {
    let Some(connection) = open(state, false).await? else {
        return Ok(Vec::new());
    };
    let result = connection
        .call(move |db| -> Result<_, Error> {
            let mut statement = db.prepare(&format!(
                "SELECT * FROM (SELECT e.key,e.eval_def_hash,e.id,e.status,json_extract(e.data,'$.provenance.repoPath'),json_extract(e.data,'$.provenance.evalId'),json_extract(e.data,'$.fingerprint'),json_extract(e.data,'$.completedAt'),json_extract(e.data,'$.origin.store'),count(*) OVER (PARTITION BY e.key),e.bytes,e.last_used,row_number() OVER (PARTITION BY e.key ORDER BY {LATEST}) AS rank,e.data FROM executions e WHERE {RECORD}) WHERE ?1 OR rank=1 ORDER BY 1,rank"
            ))?;
            Ok(statement
                .query_map([history], |row| {
                    let id: crate::types::ExecutionId = row.get(2)?;
                    let data: String = row.get(13)?;
                    let Some(execution) =
                        super::unreadable::evidence::<Execution>("execution", &data)
                    else {
                        return Ok(None);
                    };
                    Ok(Some(Entry {
                        key: row.get(0)?,
                        eval_def_hash: row.get(1)?,
                        execution_id: id,
                        verdict: row.get(3)?,
                        repo_path: row.get::<_, String>(4)?.into(),
                        eval_id: row.get(5)?,
                        fingerprint: row.get(6)?,
                        completed_at: row.get(7)?,
                        producer: execution.producer.map(|producer| producer.name),
                        variant: execution.options.variant,
                        origin: row.get(8)?,
                        records: row.get(9)?,
                        bytes: row.get(10)?,
                        last_used: row.get(11)?,
                    }))
                })?
                .filter_map(|row| row.transpose())
                .collect::<Result<_, _>>()?)
        })
        .await
        .map_err(|e| e.to_string());
    connection.finish(result).await
}

/// A key's records, latest first; without `history` only the latest.
pub async fn show(state: &Path, key: &str, history: bool) -> Result<Vec<Execution>, String> {
    let Some(connection) = open(state, false).await? else {
        return Ok(Vec::new());
    };
    let Ok(key) = key.parse::<crate::types::ReuseKey>() else {
        return connection.finish(Ok(Vec::new())).await;
    };
    let result = connection
        .call(move |db| -> Result<_, Error> {
            let mut statement = db.prepare(&format!(
                "SELECT e.data FROM executions e WHERE e.key=? AND {RECORD} ORDER BY {LATEST} LIMIT ?"
            ))?;
            statement
                .query_map(params![key, if history { -1 } else { 1 }], |row| {
                    row.get::<_, String>(0)
                })?
                .filter_map(|row| match row {
                    Ok(data) => super::unreadable::evidence("execution", &data).map(Ok),
                    Err(error) => Some(Err(Error::Sql(error))),
                })
                .collect()
        })
        .await
        .map_err(|e| e.to_string());
    connection.finish(result).await
}

/// The latest locally produced record (never a mirror) of each key after `after`, in key order.
pub async fn local(
    state: &Path,
    after: Option<crate::types::ReuseKey>,
    limit: usize,
) -> Result<Vec<Execution>, String> {
    let Some(connection) = open(state, false).await? else {
        return Ok(Vec::new());
    };
    let result = connection
        .call(move |db| -> Result<_, Error> {
            let mut statement = db.prepare(&format!(
                "SELECT data FROM (SELECT e.key,e.data,row_number() OVER (PARTITION BY e.key ORDER BY {LATEST}) AS rank FROM executions e WHERE {RECORD} AND json_extract(e.data,'$.origin') IS NULL AND e.key>?) WHERE rank=1 ORDER BY key LIMIT ?"
            ))?;
            statement
                .query_map(
                    params![
                        after.as_ref().map_or("", crate::types::ReuseKey::as_str),
                        limit as i64
                    ],
                    |row| row.get::<_, String>(0),
                )?
                .filter_map(|row| match row {
                    Ok(data) => super::unreadable::evidence("execution", &data).map(Ok),
                    Err(error) => Some(Err(Error::Sql(error))),
                })
                .collect()
        })
        .await
        .map_err(|e| e.to_string());
    connection.finish(result).await
}

/// Remove every record of an unused key from its history, preserving executions and Runs.
pub async fn remove(state: &Path, key: &str) -> Result<bool, String> {
    let Some(connection) = open(state, true).await? else {
        return Ok(false);
    };
    let Ok(key) = key.parse::<crate::types::ReuseKey>() else {
        return connection.finish(Ok(false)).await;
    };
    let result = connection
        .call(move |db| -> Result<bool, Error> {
            let transaction =
                db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let in_use: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM executions WHERE key=?1 AND status IN ('RUNNING','WAITING_HUMAN')) OR EXISTS(SELECT 1 FROM requests q JOIN executions e ON e.id=q.execution_id WHERE e.key=?1 AND e.completed_at IS NOT NULL AND q.status IN ('QUEUED','RUNNING','WAITING_HUMAN'))",
                [&key],
                |row| row.get(0),
            )?;
            if in_use {
                return Err(Error::Invalid(format!(
                    "Key {key} is in use by an active execution or waiter."
                )));
            }
            let removed = transaction.execute(
                "UPDATE executions SET completed_at=NULL,bytes=NULL,last_used=NULL WHERE key=? AND completed_at IS NOT NULL",
                [&key],
            )?;
            transaction.commit()?;
            Ok(removed != 0)
        })
        .await
        .map_err(|e| e.to_string());
    connection.finish(result).await
}

/// The history columns of a completed record: its sortable completion time and size, or
/// `None` when it does not join its key's history (no key, no verdict, or over the entry
/// limit).
pub(super) fn columns(
    execution: &Execution,
    bytes: usize,
) -> Result<Option<(crate::types::Timestamp, i64)>, Error> {
    let (Some(_), Some(_), Some(completed_at)) = (
        &execution.key,
        execution.verdict(),
        &execution.completed_at(),
    ) else {
        return Ok(None);
    };
    if bytes > MAX_ENTRY_BYTES {
        return Ok(None);
    }
    Ok(Some((*completed_at, bytes as i64)))
}

/// Mark a reused record as used now.
pub(super) fn touch(
    db: &rusqlite::Connection,
    execution_id: &crate::types::ExecutionId,
    at: crate::types::Timestamp,
) -> Result<(), Error> {
    db.execute(
        "UPDATE executions SET last_used=? WHERE id=? AND completed_at IS NOT NULL",
        params![at, execution_id],
    )?;
    Ok(())
}

/// The caps [`collect`] keeps the history no checkout reuses now within.
#[derive(Debug, Clone, Copy)]
struct Caps {
    entries: i64,
    bytes: i64,
}

const CAPS: Caps = Caps {
    entries: MAX_ENTRIES,
    bytes: MAX_BYTES,
};

/// Collect the key history once a Run has finished. The newest record of each eval in each
/// checkout that still exists is what that checkout's next verify reuses, so it stays whatever
/// the caps: one large project evicts neither another project's results nor its own. The
/// other records stay within [`MAX_ENTRIES`] and [`MAX_BYTES`], the least recently used
/// leaving first and Human sign-offs last. Records of a key with an active execution, and
/// records an in-flight request waits for, stay as well.
pub(crate) async fn collect(state: &Path) -> Result<(), String> {
    let Some(connection) = open(state, true).await? else {
        return Ok(());
    };
    let result = connection
        .call(|db| collect_within(db, CAPS))
        .await
        .map_err(|e| e.to_string());
    connection.finish(result).await
}

/// A record as collection weighs it.
struct Record {
    id: crate::types::ExecutionId,
    checkout: Option<String>,
    eval: Option<String>,
    human: bool,
    bytes: i64,
    last_used: String,
    held: bool,
}

fn collect_within(db: &mut rusqlite::Connection, caps: Caps) -> Result<(), Error> {
    let transaction = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    // Latest first: the first record seen of an eval in a checkout is the one it reuses.
    let records = transaction
        .prepare(&format!(
            "SELECT e.id,json_extract(e.data,'$.provenance.repoPath'),json_extract(e.data,'$.provenance.evalId'),
             coalesce(json_extract(e.data,'$.profile.kind')='human',0),e.bytes,e.last_used,
             EXISTS(SELECT 1 FROM executions a WHERE a.key=e.key AND a.status IN ('RUNNING','WAITING_HUMAN'))
             OR EXISTS(SELECT 1 FROM requests q WHERE q.execution_id=e.id AND q.status IN ('QUEUED','RUNNING','WAITING_HUMAN'))
             FROM executions e WHERE {RECORD} ORDER BY {LATEST}"
        ))?
        .query_map([], |row| {
            Ok(Record {
                id: row.get(0)?,
                checkout: row.get(1)?,
                eval: row.get(2)?,
                human: row.get(3)?,
                bytes: row.get(4)?,
                last_used: row.get(5)?,
                held: row.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut reused = std::collections::BTreeSet::new();
    let mut present = std::collections::BTreeMap::new();
    let mut history = Vec::new();
    let mut evicted = Vec::new();
    for record in records {
        let newest = match (&record.checkout, &record.eval) {
            (Some(checkout), Some(eval)) => {
                reused.insert((checkout.clone(), eval.clone()))
                    && *present
                        .entry(checkout.clone())
                        .or_insert_with(|| Path::new(checkout).is_dir())
            }
            _ => false,
        };
        if record.held {
            continue;
        }
        if record.bytes > MAX_ENTRY_BYTES as i64 {
            evicted.push(record.id);
        } else if !newest {
            history.push(record);
        }
    }
    // Human sign-offs leave last; among the rest the least recently used leave first.
    history.sort_by(|a, b| {
        b.human
            .cmp(&a.human)
            .then_with(|| b.last_used.cmp(&a.last_used))
    });
    let (mut entries, mut bytes) = (0, 0);
    for record in history {
        if entries < caps.entries && bytes + record.bytes <= caps.bytes {
            entries += 1;
            bytes += record.bytes;
        } else {
            evicted.push(record.id);
        }
    }
    {
        // Execution and receipt rows stay; only their place in the history goes.
        let mut evict = transaction.prepare(
            "UPDATE executions SET completed_at=NULL,bytes=NULL,last_used=NULL WHERE id=?",
        )?;
        for id in &evicted {
            evict.execute([id])?;
        }
    }
    Ok(transaction.commit()?)
}

#[cfg(test)]
mod tests;
