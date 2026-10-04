use std::path::PathBuf;

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    Receipts, Request,
    receipts::{Error, schema_initialized, update_request},
};
use crate::{process, runtime::Verdict};

pub enum Claim {
    Owned,
    BudgetExhausted,
    Reuse(Box<Execution>),
    Wait(String),
    WaitHuman(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provenance {
    pub repo_path: PathBuf,
    pub run_id: String,
    pub request_id: String,
    pub eval_id: String,
    pub eval_def_hash: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Execution {
    pub id: String,
    pub identity: Option<String>,
    pub eval_def_hash: String,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer: Option<Producer>,
    /// The Human claimant who submitted this result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer: Option<String>,
    /// Set only on executions mirrored from a remote review store.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Origin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<crate::cache::Manifest>,
}

/// Who produced an execution: display metadata, never authentication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Producer {
    /// `user@host`.
    pub name: String,
    /// The producing artifactize version.
    pub version: String,
}

impl Producer {
    pub fn current() -> Self {
        let user = ["USER", "LOGNAME"]
            .into_iter()
            .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()));
        let host = std::fs::read_to_string("/proc/sys/kernel/hostname").ok();
        let name = format!(
            "{}@{}",
            user.as_deref().unwrap_or("unknown"),
            host.as_deref()
                .map(str::trim)
                .filter(|host| !host.is_empty())
                .unwrap_or("unknown")
        );
        Self {
            name: name.chars().filter(|c| !c.is_control()).take(200).collect(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }
}

/// The remote store, authenticated publisher and server clock of a mirrored execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Origin {
    pub store: String,
    pub publisher: String,
    pub published_at: String,
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
    eval_def_hash: &str,
) -> Result<Option<Execution>, Error> {
    let data: Option<String> = db.query_row(
        "SELECT e.data FROM cache_entries c JOIN executions e ON e.id=c.execution_id WHERE c.identity=? AND c.eval_def_hash=? AND e.status IN ('GREEN','RED')",
        params![identity, eval_def_hash], |row| row.get(0),
    ).optional()?;
    data.map(|data| serde_json::from_str(&data))
        .transpose()
        .map_err(Into::into)
}

fn active_owner(
    db: &rusqlite::Connection,
    identity: &str,
    eval_def_hash: &str,
) -> Result<Option<(String, process::ChildIdentity, String)>, Error> {
    db.query_row(
        "SELECT id,owner_pid,owner_start_time,status FROM executions WHERE identity=? AND eval_def_hash=? AND status IN ('RUNNING','WAITING_HUMAN')",
        params![identity, eval_def_hash],
        |row| Ok((row.get(0)?, process::ChildIdentity { pid: row.get(1)?, start_time: row.get::<_, i64>(2)? as u64 }, row.get(3)?)),
    ).optional().map_err(Into::into)
}

fn available_to_waiter(
    db: &rusqlite::Connection,
    identity: &str,
    eval_def_hash: &str,
    waiting_for: Option<&str>,
) -> Result<Option<Claim>, Error> {
    if let Some(id) = waiting_for {
        let data: Option<String> = db.query_row(
            "SELECT data FROM executions WHERE id=? AND identity=? AND eval_def_hash=? AND status IN ('GREEN','RED')",
            params![id, identity, eval_def_hash], |row| row.get(0),
        ).optional()?;
        if let Some(data) = data {
            return Ok(Some(Claim::Reuse(Box::new(serde_json::from_str(&data)?))));
        }
    }
    available(db, identity, eval_def_hash)
}

fn available(
    db: &rusqlite::Connection,
    identity: &str,
    eval_def_hash: &str,
) -> Result<Option<Claim>, Error> {
    if let Some(execution) = lookup(db, identity, eval_def_hash)? {
        return Ok(Some(Claim::Reuse(Box::new(execution))));
    }
    if let Some((id, owner, status)) = active_owner(db, identity, eval_def_hash)? {
        if status == "WAITING_HUMAN" {
            return Ok(Some(Claim::WaitHuman(id)));
        }
        if process::is_alive(owner).map_err(|e| Error::Invalid(e.to_string()))? {
            return Ok(Some(Claim::Wait(id)));
        }
    }
    Ok(None)
}

/// Read completed entries or live owners without creating or changing the database.
pub async fn read_identity_executions(
    state: &std::path::Path,
    keys: &[(String, String)],
) -> Result<std::collections::BTreeMap<(String, String), Claim>, String> {
    let state = crate::workspace::canonical_target(state).map_err(|e| e.to_string())?;
    super::receipts::check_files(&state)?;
    if keys.is_empty()
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
    let keys = keys.to_vec();
    connection
        .call(move |db| -> Result<_, Error> {
            db.busy_timeout(std::time::Duration::from_secs(5))?;
            let transaction = db.transaction()?;
            if !schema_initialized(&transaction)? {
                return Ok(Default::default());
            }
            let mut entries = std::collections::BTreeMap::new();
            for (identity, eval_def_hash) in keys {
                if let Some(execution) = available(&transaction, &identity, &eval_def_hash)? {
                    entries.insert((identity, eval_def_hash), execution);
                }
            }
            transaction.commit()?;
            Ok(entries)
        })
        .await
        .map_err(|e| e.to_string())
}

/// The most recently published result per (Eval id, Eval definition hash), read-only.
pub async fn read_latest_cached(
    state: &std::path::Path,
    keys: &[(String, String)],
) -> Result<std::collections::BTreeMap<(String, String), Execution>, String> {
    let state = crate::workspace::canonical_target(state).map_err(|e| e.to_string())?;
    super::receipts::check_files(&state)?;
    if keys.is_empty()
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
    let keys = keys.to_vec();
    connection
        .call(move |db| -> Result<_, Error> {
            db.busy_timeout(std::time::Duration::from_secs(5))?;
            let transaction = db.transaction()?;
            if !schema_initialized(&transaction)? {
                return Ok(Default::default());
            }
            let mut latest = std::collections::BTreeMap::new();
            {
                let mut statement = transaction.prepare(
                    "SELECT e.data FROM cache_entries c JOIN executions e ON e.id=c.execution_id WHERE c.eval_def_hash=? AND json_extract(e.data,'$.provenance.evalId')=? AND e.status IN ('GREEN','RED') ORDER BY c.rowid DESC LIMIT 1",
                )?;
                for (eval_id, eval_def_hash) in keys {
                    let data: Option<String> = statement
                        .query_row(params![eval_def_hash, eval_id], |row| row.get(0))
                        .optional()?;
                    if let Some(data) = data {
                        latest.insert((eval_id, eval_def_hash), serde_json::from_str(&data)?);
                    }
                }
            }
            transaction.commit()?;
            Ok(latest)
        })
        .await
        .map_err(|e| e.to_string())
}

impl Receipts {
    pub async fn cached_execution(
        &self,
        identity: &str,
        eval_def_hash: &str,
    ) -> Result<Option<Execution>, String> {
        let identity = identity.to_owned();
        let eval_def_hash = eval_def_hash.to_owned();
        self.connection
            .call(move |db| -> Result<_, Error> {
                let execution = lookup(db, &identity, &eval_def_hash)?;
                if let Some(execution) = &execution {
                    db.execute(
                        "UPDATE cache_entries SET last_used=? WHERE identity=? AND eval_def_hash=? AND execution_id=?",
                        params![crate::broker::now(), identity, eval_def_hash, execution.id],
                    )?;
                }
                Ok(execution)
            })
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn claim_execution(
        &self,
        execution: &Execution,
        waiting_for: Option<&str>,
        allow_start: bool,
    ) -> Result<Claim, String> {
        let execution = execution.clone();
        let waiting_for = waiting_for.map(str::to_owned);
        self.connection.call(move |db| -> Result<Claim, Error> {
            let Some(identity) = &execution.identity else {
                return Ok(if allow_start { Claim::Owned } else { Claim::BudgetExhausted });
            };
            {
                let transaction = db.transaction()?;
                if let Some(claim) = available_to_waiter(&transaction, identity, &execution.eval_def_hash, waiting_for.as_deref())? {
                    return Ok(claim);
                }
            }
            if !allow_start {
                return Ok(Claim::BudgetExhausted);
            }
            let transaction = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            if let Some(claim) = available_to_waiter(&transaction, identity, &execution.eval_def_hash, waiting_for.as_deref())? {
                return Ok(claim);
            }
            if let Some((id, _, _)) = active_owner(&transaction, identity, &execution.eval_def_hash)? {
                transaction.execute(
                    "UPDATE executions SET status='ERROR',data=json_set(data,'$.status','ERROR','$.error','Execution owner died.','$.errorCode','OWNER_DIED','$.completedAt',?,'$.provenance.completedAt',?) WHERE id=? AND status='RUNNING'",
                    params![execution.started_at, execution.started_at, id],
                )?;
            }
            let inserted = transaction.execute(
                "INSERT INTO executions(id,identity,eval_def_hash,owner_pid,owner_start_time,status,data) VALUES (?,?,?,?,?,?,?) ON CONFLICT(identity,eval_def_hash) WHERE identity IS NOT NULL AND status IN ('RUNNING','WAITING_HUMAN') DO NOTHING",
                params![execution.id, identity, execution.eval_def_hash, execution.owner_pid, execution.owner_start_time as i64, execution.status, serde_json::to_string(&execution)?],
            )?;
            let claim = if inserted == 1 {
                Claim::Owned
            } else {
                Claim::Wait(active_owner(&transaction, identity, &execution.eval_def_hash)?.expect("conflicting active identity").0)
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
                    "UPDATE cache_entries SET last_used=? WHERE identity=? AND eval_def_hash=? AND execution_id=?",
                    params![request.completed_at, request.identity, request.eval_def_hash, request.execution_id],
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
        self.connection
            .call(move |db| -> Result<(), Error> {
                let transaction =
                    db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                let published = settle(&transaction, &execution, &request)?;
                transaction.commit()?;
                if published && let Err(error) = super::cache_entries::collect(db) {
                    use std::io::Write;
                    let _ = writeln!(std::io::stderr().lock(), "Cache GC failed: {error}");
                }
                Ok(())
            })
            .await
            .map_err(|e| e.to_string())
    }

    /// Cache a remote result as a self-contained execution; an existing local entry wins.
    pub async fn mirror_execution(&self, execution: &Execution) -> Result<Execution, String> {
        let execution = execution.clone();
        self.connection
            .call(move |db| -> Result<Execution, Error> {
                let (Some(stale_key), Some(_), Some(_)) =
                    (&execution.identity, &execution.origin, execution.verdict())
                else {
                    return Err(Error::Invalid(
                        "Only completed remote GREEN/RED results can be mirrored.".into(),
                    ));
                };
                let data = serde_json::to_string(&execution)?;
                if data.len() > super::cache_entries::MAX_ENTRY_BYTES {
                    return Err(Error::Invalid(
                        "Remote result exceeds the cache entry limit.".into(),
                    ));
                }
                let transaction =
                    db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                if let Some(existing) = lookup(&transaction, stale_key, &execution.eval_def_hash)? {
                    return Ok(existing);
                }
                // A mirror kept after `cache rm` or GC is reused for the same remote execution.
                transaction.execute(
                    "INSERT INTO executions(id,identity,eval_def_hash,owner_pid,owner_start_time,status,data) VALUES (?,?,?,?,?,?,?) ON CONFLICT(id) DO NOTHING",
                    params![execution.id, stale_key, execution.eval_def_hash, execution.owner_pid, execution.owner_start_time as i64, execution.status, data],
                )?;
                let data: String = transaction.query_row(
                    "SELECT data FROM executions WHERE id=? AND identity=? AND eval_def_hash=? AND status=?",
                    params![execution.id, stale_key, execution.eval_def_hash, execution.status],
                    |row| row.get(0),
                ).optional()?.ok_or_else(|| Error::Invalid("Mirrored execution ID conflicts with another execution.".into()))?;
                transaction.execute(
                    "INSERT INTO cache_entries(identity,eval_def_hash,execution_id,bytes,last_used) VALUES (?,?,?,?,?)",
                    params![stale_key, execution.eval_def_hash, execution.id, data.len() as i64, crate::broker::now()],
                )?;
                transaction.commit()?;
                if let Err(error) = super::cache_entries::collect(db) {
                    use std::io::Write;
                    let _ = writeln!(std::io::stderr().lock(), "Cache GC failed: {error}");
                }
                Ok(serde_json::from_str(&data)?)
            })
            .await
            .map_err(|e| e.to_string())
    }
}

pub(super) fn settle(
    db: &rusqlite::Connection,
    execution: &Execution,
    request: &Request,
) -> Result<bool, Error> {
    let mut execution = execution.clone();
    let mut request = request.clone();
    execution.tool_calls =
        super::tool_calls::project(db, Some(&execution.id), &execution.tool_calls)?;
    request.tool_calls = execution.tool_calls.clone();
    let data = serde_json::to_string(&execution)?;
    if execution.identity.is_some() || request.human_definition.is_some() {
        if db.execute(
                    "UPDATE executions SET status=?,data=? WHERE id=? AND owner_pid=? AND owner_start_time=? AND status IN ('RUNNING','WAITING_HUMAN')",
                    params![execution.status, data, execution.id, execution.owner_pid, execution.owner_start_time as i64],
                )? != 1 {
                    return Err(Error::Invalid("Active execution not found.".into()));
                }
    } else {
        db.execute("INSERT INTO executions(id,identity,eval_def_hash,owner_pid,owner_start_time,status,data) VALUES (?,?,?,?,?,?,?)", params![execution.id, execution.identity, execution.eval_def_hash, execution.owner_pid, execution.owner_start_time as i64, execution.status, data])?;
    }
    let published = if execution.verdict().is_some()
        && data.len() <= super::cache_entries::MAX_ENTRY_BYTES
        && let Some(identity) = &execution.identity
    {
        db.execute("INSERT INTO cache_entries(identity,eval_def_hash,execution_id,bytes,last_used) VALUES (?,?,?,?,?) ON CONFLICT(identity,eval_def_hash) DO NOTHING", params![identity, execution.eval_def_hash, execution.id, data.len() as i64, execution.completed_at])? != 0
    } else {
        false
    };
    update_request(db, &request)?;
    Ok(published)
}
