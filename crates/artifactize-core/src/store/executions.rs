mod admission;
mod mirror;
mod mutex;
mod wire;
pub use admission::{Capacity, Claim};
use admission::{ProfileIdentity, available, open_read_only, sharing_identity};
pub use wire::{Execution, ExecutionOptions, Origin, Producer, Provenance};

use crate::types::{ExecutionId, ExecutionStatus, ReuseKey};
use std::collections::BTreeMap;

use rusqlite::{OptionalExtension, params};

use super::{
    Receipts, Request, history,
    receipts::{Error, schema_initialized, update_request},
};

/// The latest completed record of a key.
pub(super) fn lookup(
    db: &rusqlite::Connection,
    key: &ReuseKey,
) -> Result<Option<Execution>, Error> {
    let row: Option<(crate::types::ExecutionId, ExecutionStatus, String)> = db
        .query_row(
            &format!(
                "SELECT e.id,e.status,e.data FROM executions e WHERE e.key=? AND {} ORDER BY {} LIMIT 1",
                history::RECORD,
                history::LATEST
            ),
            [key],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((id, indexed, data)) = row else {
        return Ok(None);
    };
    let Some(execution) = super::unreadable::evidence::<Execution>("execution", &data) else {
        return Ok(None);
    };
    if execution.status() != indexed {
        return Err(Error::Invalid(format!(
            "Cached execution {id} has an inconsistent verdict."
        )));
    }
    Ok(Some(execution))
}

pub async fn read_keyed_executions(
    state: &std::path::Path,
    keys: &[ReuseKey],
) -> Result<std::collections::BTreeMap<ReuseKey, Claim>, String> {
    read_claims(
        state,
        keys.iter()
            .map(|key| (key.clone(), key.clone(), None))
            .collect(),
    )
    .await
}

pub(crate) async fn read_profiled_executions(
    state: &std::path::Path,
    keys: Vec<(crate::types::EvalId, ReuseKey, crate::config::StoredProfile)>,
) -> Result<BTreeMap<crate::types::EvalId, Claim>, String> {
    let keys = keys
        .into_iter()
        .map(|(id, key, profile)| {
            sharing_identity(&key, &profile)
                .map(|(key, profile)| (id, key, Some(profile)))
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    read_claims(state, keys).await
}

async fn read_claims<K: Ord + Send + 'static>(
    state: &std::path::Path,
    keys: Vec<(K, ReuseKey, Option<ProfileIdentity>)>,
) -> Result<BTreeMap<K, Claim>, String> {
    if keys.is_empty() {
        return Ok(Default::default());
    }
    let Some(connection) = open_read_only(state).await? else {
        return Ok(Default::default());
    };
    let result = connection
        .call(move |db| -> Result<_, Error> {
            super::wait_when_busy(db)?;
            let transaction = db.transaction()?;
            if !schema_initialized(&transaction)? {
                return Ok(Default::default());
            }
            let mut entries = std::collections::BTreeMap::new();
            for (id, key, profile) in keys {
                if let Some(claim) = available(&transaction, &key, profile.as_ref())? {
                    entries.insert(id, claim);
                }
            }
            transaction.commit()?;
            Ok(entries)
        })
        .await
        .map_err(|e| e.to_string());
    super::connection::finish(connection, result).await
}

/// The latest record per (Eval id, Eval definition hash) under any key, read-only: what a
/// changed key is explained against.
pub async fn read_latest_cached(
    state: &std::path::Path,
    keys: &[(crate::types::EvalId, crate::types::DefinitionHash)],
) -> Result<
    std::collections::BTreeMap<(crate::types::EvalId, crate::types::DefinitionHash), Execution>,
    String,
> {
    if keys.is_empty() {
        return Ok(Default::default());
    }
    let Some(connection) = open_read_only(state).await? else {
        return Ok(Default::default());
    };
    let keys = keys.to_vec();
    let result = connection
        .call(move |db| -> Result<_, Error> {
            super::wait_when_busy(db)?;
            let transaction = db.transaction()?;
            if !schema_initialized(&transaction)? {
                return Ok(Default::default());
            }
            let mut latest = std::collections::BTreeMap::new();
            {
                let mut statement = transaction.prepare(&format!(
                    "SELECT e.data FROM executions e WHERE e.eval_def_hash=? AND json_extract(e.data,'$.provenance.evalId')=? AND {} ORDER BY {} LIMIT 1",
                    history::RECORD,
                    history::LATEST
                ))?;
                for (eval_id, eval_def_hash) in keys {
                    let data: Option<String> = statement
                        .query_row(params![eval_def_hash, eval_id], |row| row.get(0))
                        .optional()?;
                    if let Some(data) = data
                        && let Some(execution) = super::unreadable::evidence("execution", &data)
                    {
                        latest.insert((eval_id, eval_def_hash), execution);
                    }
                }
            }
            transaction.commit()?;
            Ok(latest)
        })
        .await
        .map_err(|e| e.to_string());
    super::connection::finish(connection, result).await
}

impl Receipts {
    /// The key's latest record; a hit updates its last use.
    pub async fn cached_execution(&self, key: &ReuseKey) -> Result<Option<Execution>, String> {
        let key = key.clone();
        self.connection
            .call(move |db| -> Result<_, Error> {
                let execution = lookup(db, &key)?;
                if let Some(execution) = &execution {
                    history::touch(db, &execution.id, crate::broker::now())?;
                }
                Ok(execution)
            })
            .await
            .map_err(|e| e.to_string())
    }

    /// The saved execution eligible for publication; speculative Runs hold publication
    /// until blocked work has been withdrawn. Mirrors never publish again.
    pub async fn published_execution(
        &self,
        execution_id: &ExecutionId,
    ) -> Result<Option<Execution>, String> {
        let execution_id = execution_id.clone();
        self.connection
            .call(move |db| -> Result<_, Error> {
                let data: Option<String> = db
                    .query_row(
                        &format!(
                            "SELECT e.data FROM executions e LEFT JOIN runs r ON r.id=json_extract(e.data,'$.provenance.runId') WHERE e.id=? AND {} AND (r.id IS NULL OR r.status NOT IN ('RUNNING','WAITING_HUMAN') OR coalesce(json_extract(r.data,'$.modes.speculate'),0)=0 OR json_extract(r.data,'$.control.activity')='publishing')",
                            history::RECORD
                        ),
                        [execution_id],
                        |row| row.get(0),
                    )
                    .optional()?;
                Ok(data
                    .and_then(|data| super::unreadable::evidence::<Execution>("execution", &data))
                    .filter(|execution| execution.origin.is_none()))
            })
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn reuse_execution(&self, request: &Request) -> Result<(), String> {
        let request = request.clone();
        self.connection
            .call(move |db| -> Result<(), Error> {
                let transaction = db.transaction()?;
                update_request(&transaction, &request)?;
                if let (Some(execution), Some(at)) = (&request.execution_id, request.completed_at())
                {
                    history::touch(&transaction, execution, at)?;
                }
                transaction.commit()?;
                Ok(())
            })
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn complete_execution(
        &self,
        execution: &Execution,
        request: &Request,
    ) -> Result<(), String> {
        let execution = execution.clone();
        let request = request.clone();
        self.connection
            .call(move |db| -> Result<(), Error> {
                let transaction =
                    db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                settle(&transaction, &execution, &request)?;
                transaction.commit()?;
                Ok(())
            })
            .await
            .map_err(|e| e.to_string())
    }
}

/// Save a finished execution and its request; a GREEN/RED with a key joins the key's history.
/// A claimed or Human execution updates its active row; a forced or unkeyed one is inserted,
/// or replaces the RUNNING row its limited start inserted.
pub(super) fn settle(
    db: &rusqlite::Connection,
    execution: &Execution,
    request: &Request,
) -> Result<(), Error> {
    assert!(
        request.status() == execution.status().into()
            && request.execution_id.as_ref() == Some(&execution.id)
            && request.result() == execution.result()
            && request.error() == execution.error()
            && request.error_code() == execution.error_code(),
        "Execution and request settlement disagree."
    );
    let data = serde_json::to_string(execution)?;
    let record = history::columns(execution, data.len())?;
    let last_used = record.as_ref().and(execution.completed_at());
    let (completed_at, bytes) = record.unzip();
    let claimed = execution.key.is_some() && !request.force;
    if claimed || request.human_definition.is_some() {
        if db.execute(
            "UPDATE executions SET key=?,status=?,completed_at=?,bytes=?,last_used=?,data=? WHERE id=? AND owner_pid=? AND owner_start_time=? AND status IN ('RUNNING','WAITING_HUMAN')",
            params![
                execution.key,
                execution.status(),
                completed_at,
                bytes,
                last_used,
                data,
                execution.id,
                execution.owner_pid(),
                execution.owner_start_time() as i64
            ],
        )? != 1
        {
            return Err(Error::Invalid("Active execution not found.".into()));
        }
    } else {
        db.execute(
            "INSERT INTO executions(id,key,eval_def_hash,status,owner_pid,owner_start_time,backend,completed_at,bytes,last_used,data) VALUES (?,?,?,?,?,?,?,?,?,?,?)
            ON CONFLICT(id) DO UPDATE SET key=excluded.key,status=excluded.status,completed_at=excluded.completed_at,bytes=excluded.bytes,last_used=excluded.last_used,data=excluded.data",
            params![
                execution.id,
                execution.key,
                execution.eval_def_hash,
                execution.status(),
                execution.owner_pid(),
                execution.owner_start_time() as i64,
                execution.backend(),
                completed_at,
                bytes,
                last_used,
                data
            ],
        )?;
    }
    update_request(db, request)
}
