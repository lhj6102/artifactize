use std::path::PathBuf;

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    Receipts, Request,
    receipts::{Error, update_request},
};
use crate::runtime::Verdict;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provenance {
    pub repo_path: PathBuf,
    pub run_id: String,
    pub request_id: String,
    pub eval_id: String,
    pub completed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Execution {
    pub id: String,
    pub identity: Option<String>,
    pub owner_pid: u32,
    pub owner_start_time: u64,
    pub status: String,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub error_code: Option<String>,
    pub profile: Value,
    pub usage: Option<Value>,
    pub provenance: Provenance,
    pub started_at: String,
    pub completed_at: String,
}

impl Execution {
    pub fn verdict(&self) -> Option<Verdict> {
        match self.status.as_str() {
            "GREEN" => Some(Verdict::Green),
            "RED" => Some(Verdict::Red),
            _ => None,
        }
    }
}

pub(super) fn lookup(
    db: &rusqlite::Connection,
    identity: &str,
) -> Result<Option<Execution>, Error> {
    let data: Option<String> = db.query_row(
        "SELECT e.data FROM cache_entries c JOIN executions e ON e.id=c.execution_id WHERE c.identity=? AND e.status IN ('GREEN','RED')",
        [identity], |row| row.get(0),
    ).optional()?;
    data.map(|data| serde_json::from_str(&data))
        .transpose()
        .map_err(Into::into)
}

/// Read completed entries without creating a database or touching access times.
pub async fn read_cached_executions(
    state: &std::path::Path,
    identities: &[String],
) -> Result<std::collections::BTreeMap<String, Execution>, String> {
    let state = crate::workspace::canonical_target(state).map_err(|e| e.to_string())?;
    super::receipts::check_files(&state)?;
    if identities.is_empty()
        || !state
            .join(super::DATABASE)
            .try_exists()
            .map_err(|e| e.to_string())?
    {
        return Ok(Default::default());
    }
    let connection = tokio_rusqlite::Connection::open_with_flags(
        state.join(super::DATABASE),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .await
    .map_err(|e| e.to_string())?;
    let identities = identities.to_vec();
    connection
        .call(move |db| -> Result<_, Error> {
            db.busy_timeout(std::time::Duration::from_secs(5))?;
            let transaction = db.transaction()?;
            let version: u32 =
                transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
            if version != super::STATE_SCHEMA_VERSION {
                return Err(Error::Invalid(format!(
                    "Unsupported state schema version: {version}"
                )));
            }
            let mut entries = std::collections::BTreeMap::new();
            for identity in identities {
                if let Some(execution) = lookup(&transaction, &identity)? {
                    entries.insert(identity, execution);
                }
            }
            transaction.commit()?;
            Ok(entries)
        })
        .await
        .map_err(|e| e.to_string())
}

impl Receipts {
    pub async fn cached_execution(&self, identity: &str) -> Result<Option<Execution>, String> {
        let identity = identity.to_owned();
        self.connection
            .call(move |db| lookup(db, &identity))
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn reuse_execution(&self, request: &Request) -> Result<(), String> {
        let request = request.clone();
        self.connection
            .call(move |db| -> Result<(), Error> {
                let transaction = db.transaction()?;
                update_request(&transaction, &request)?;
                transaction.execute(
                    "UPDATE cache_entries SET last_used=? WHERE identity=?",
                    params![request.completed_at, request.identity],
                )?;
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
        self.connection.call(move |db| -> Result<(), Error> {
            let data = serde_json::to_string(&execution)?;
            let transaction = db.transaction()?;
            transaction.execute("INSERT INTO executions(id,identity,owner_pid,owner_start_time,status,data) VALUES (?,?,?,?,?,?)", params![execution.id, execution.identity, execution.owner_pid, execution.owner_start_time as i64, execution.status, data])?;
            if execution.verdict().is_some() && let Some(identity) = &execution.identity {
                transaction.execute("INSERT INTO cache_entries(identity,execution_id,bytes,last_used) VALUES (?,?,?,?) ON CONFLICT(identity) DO NOTHING", params![identity, execution.id, data.len() as i64, execution.completed_at])?;
            }
            update_request(&transaction, &request)?;
            transaction.commit()?;
            Ok(())
        }).await.map_err(|e| e.to_string())
    }
}
