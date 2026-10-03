use std::path::PathBuf;

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    Receipts, Request,
    receipts::{Error, update_request},
};
use crate::{process, runtime::Verdict};

pub enum Claim {
    Owned,
    BudgetExhausted,
    Reuse(Box<Execution>),
    Wait(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provenance {
    pub repo_path: PathBuf,
    pub run_id: String,
    pub request_id: String,
    pub eval_id: String,
    pub completed_at: Option<String>,
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
    #[serde(default)]
    pub tool_calls: Vec<Value>,
    pub provenance: Provenance,
    pub started_at: String,
    pub completed_at: Option<String>,
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

fn active_owner(
    db: &rusqlite::Connection,
    identity: &str,
) -> Result<Option<(String, process::ChildIdentity)>, Error> {
    db.query_row(
        "SELECT id,owner_pid,owner_start_time FROM executions WHERE identity=? AND status IN ('RUNNING','WAITING_HUMAN')",
        [identity],
        |row| Ok((row.get(0)?, process::ChildIdentity { pid: row.get(1)?, start_time: row.get::<_, i64>(2)? as u64 })),
    ).optional().map_err(Into::into)
}

fn available(db: &rusqlite::Connection, identity: &str) -> Result<Option<Claim>, Error> {
    if let Some(execution) = lookup(db, identity)? {
        return Ok(Some(Claim::Reuse(Box::new(execution))));
    }
    if let Some((id, owner)) = active_owner(db, identity)?
        && process::is_alive(owner).map_err(|e| Error::Invalid(e.to_string()))?
    {
        return Ok(Some(Claim::Wait(id)));
    }
    Ok(None)
}

/// Read completed entries or live owners without creating or changing the database.
pub async fn read_identity_executions(
    state: &std::path::Path,
    identities: &[String],
) -> Result<std::collections::BTreeMap<String, Claim>, String> {
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
                if let Some(execution) = available(&transaction, &identity)? {
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

    pub async fn claim_execution(
        &self,
        execution: &Execution,
        allow_start: bool,
    ) -> Result<Claim, String> {
        let execution = execution.clone();
        self.connection.call(move |db| -> Result<Claim, Error> {
            let Some(identity) = &execution.identity else {
                return Ok(if allow_start { Claim::Owned } else { Claim::BudgetExhausted });
            };
            if let Some(claim) = available(db, identity)? {
                return Ok(claim);
            }
            if !allow_start {
                return Ok(Claim::BudgetExhausted);
            }
            let transaction = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            if let Some(claim) = available(&transaction, identity)? {
                return Ok(claim);
            }
            if let Some((id, _)) = active_owner(&transaction, identity)? {
                transaction.execute(
                    "UPDATE executions SET status='ERROR',data=json_set(data,'$.status','ERROR','$.error','Execution owner died.','$.errorCode','OWNER_DIED','$.completedAt',?,'$.provenance.completedAt',?) WHERE id=? AND status IN ('RUNNING','WAITING_HUMAN')",
                    params![execution.started_at, execution.started_at, id],
                )?;
            }
            let inserted = transaction.execute(
                "INSERT INTO executions(id,identity,owner_pid,owner_start_time,status,data) VALUES (?,?,?,?,?,?) ON CONFLICT(identity) WHERE identity IS NOT NULL AND status IN ('RUNNING','WAITING_HUMAN') DO NOTHING",
                params![execution.id, identity, execution.owner_pid, execution.owner_start_time as i64, execution.status, serde_json::to_string(&execution)?],
            )?;
            let claim = if inserted == 1 {
                Claim::Owned
            } else {
                Claim::Wait(active_owner(&transaction, identity)?.expect("conflicting active identity").0)
            };
            transaction.commit()?;
            Ok(claim)
        }).await.map_err(|e| e.to_string())
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
            if execution.identity.is_some() {
                if transaction.execute(
                    "UPDATE executions SET status=?,data=? WHERE id=? AND owner_pid=? AND owner_start_time=? AND status IN ('RUNNING','WAITING_HUMAN')",
                    params![execution.status, data, execution.id, execution.owner_pid, execution.owner_start_time as i64],
                )? != 1 {
                    return Err(Error::Invalid("Active execution not found.".into()));
                }
            } else {
                transaction.execute("INSERT INTO executions(id,identity,owner_pid,owner_start_time,status,data) VALUES (?,?,?,?,?,?)", params![execution.id, execution.identity, execution.owner_pid, execution.owner_start_time as i64, execution.status, data])?;
            }
            if execution.verdict().is_some() && let Some(identity) = &execution.identity {
                transaction.execute("INSERT INTO cache_entries(identity,execution_id,bytes,last_used) VALUES (?,?,?,?) ON CONFLICT(identity) DO NOTHING", params![identity, execution.id, data.len() as i64, execution.completed_at])?;
            }
            update_request(&transaction, &request)?;
            transaction.commit()?;
            Ok(())
        }).await.map_err(|e| e.to_string())
    }
}
