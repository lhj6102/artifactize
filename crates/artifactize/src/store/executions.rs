use std::{collections::BTreeMap, path::PathBuf};

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    Receipts, Request,
    receipts::{Error, schema_initialized, update_request},
};
use crate::{config::Profile, process, runtime::Verdict};

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
    /// For Agent results: the SHA-256 of each tool's `executionPaths` when the review started,
    /// by tool name and declared path.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub execution_paths: crate::tools::pins::Pins,
}

/// One execution and, once GREEN or RED with a key, one record of its key's history.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Execution {
    pub id: String,
    /// The reuse key; absent without a fingerprint on every Artifact the eval depends on.
    #[serde(default)]
    pub key: Option<String>,
    /// The target Artifact's fingerprint.
    pub fingerprint: Option<String>,
    /// Each Artifact the key covers, the target included, with its fingerprint.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fingerprints: BTreeMap<String, String>,
    pub eval_def_hash: String,
    pub owner_pid: u32,
    pub owner_start_time: u64,
    pub status: String,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub error_code: Option<String>,
    pub profile: Value,
    /// How this result was produced; never part of the key.
    #[serde(default)]
    pub options: ExecutionOptions,
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

/// The execution options of a result: stored next to it, never part of its key, so results
/// from different profiles for the same key reuse each other.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tool_calls: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    /// The selected `profileVariants` entry; absent for the declared profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    /// An Agent eval's declared `resultCheck.timeoutMs`: a limit, like the profile's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_check_timeout_ms: Option<u32>,
}

impl ExecutionOptions {
    /// The declared options of an effective profile, as declared (absent means the default).
    pub fn new(profile: &Profile, variant: Option<&str>) -> Self {
        let mut options = Self {
            variant: variant.map(str::to_owned),
            ..Self::default()
        };
        match profile {
            Profile::Agent {
                backend,
                model,
                reasoning,
                timeout_ms,
                max_tool_calls,
                max_tokens,
            } => {
                options.backend = serde_json::to_value(backend)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned));
                options.model = Some(model.clone());
                options.reasoning = reasoning.clone();
                options.timeout_ms = *timeout_ms;
                options.max_tool_calls = *max_tool_calls;
                options.max_tokens = *max_tokens;
            }
            Profile::Runtime { timeout_ms, .. } => options.timeout_ms = *timeout_ms,
            Profile::Human {} => {}
        }
        options
    }

    /// These options with the eval's declared `resultCheck` limit.
    pub fn with_result_check(mut self, check: Option<&crate::config::ResultCheck>) -> Self {
        self.result_check_timeout_ms = check.and_then(|check| check.timeout_ms);
        self
    }
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
        // Windows sets neither, only USERNAME.
        let user = ["USER", "LOGNAME", "USERNAME"]
            .into_iter()
            .take(if cfg!(windows) { 3 } else { 2 })
            .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()));
        let host = crate::platform::host_name();
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
    /// Whether this record completed after `other`, comparing `completedAt` as instants.
    pub fn completed_after(&self, other: &Execution) -> bool {
        let at = |execution: &Execution| {
            execution
                .completed_at
                .as_deref()
                .and_then(crate::broker::sortable)
        };
        at(self) > at(other)
    }

    pub fn verdict(&self) -> Option<Verdict> {
        match self.status.as_str() {
            "GREEN" => Some(Verdict::Green),
            "RED" => Some(Verdict::Red),
            _ => None,
        }
    }
}

/// The latest completed record of a key.
pub(super) fn lookup(db: &rusqlite::Connection, key: &str) -> Result<Option<Execution>, Error> {
    let data: Option<String> = db
        .query_row(
            &format!("SELECT e.data FROM cache_entries c JOIN executions e ON e.id=c.execution_id WHERE c.key=? AND e.status IN ('GREEN','RED') ORDER BY {} LIMIT 1", super::cache_entries::LATEST),
            [key],
            |row| row.get(0),
        )
        .optional()?;
    data.map(|data| serde_json::from_str(&data))
        .transpose()
        .map_err(Into::into)
}

fn active_owner(
    db: &rusqlite::Connection,
    key: &str,
) -> Result<Option<(String, process::ChildIdentity, String)>, Error> {
    db.query_row(
        "SELECT id,owner_pid,owner_start_time,status FROM executions WHERE key=? AND status IN ('RUNNING','WAITING_HUMAN')",
        [key],
        |row| Ok((row.get(0)?, process::ChildIdentity { pid: row.get(1)?, start_time: row.get::<_, i64>(2)? as u64 }, row.get(3)?)),
    ).optional().map_err(Into::into)
}

fn available_to_waiter(
    db: &rusqlite::Connection,
    key: &str,
    waiting_for: Option<&str>,
) -> Result<Option<Claim>, Error> {
    if let Some(id) = waiting_for {
        let data: Option<String> = db
            .query_row(
                "SELECT data FROM executions WHERE id=? AND key=? AND status IN ('GREEN','RED')",
                params![id, key],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(data) = data {
            return Ok(Some(Claim::Reuse(Box::new(serde_json::from_str(&data)?))));
        }
    }
    available(db, key)
}

fn available(db: &rusqlite::Connection, key: &str) -> Result<Option<Claim>, Error> {
    if let Some(execution) = lookup(db, key)? {
        return Ok(Some(Claim::Reuse(Box::new(execution))));
    }
    if let Some((id, owner, status)) = active_owner(db, key)? {
        if status == "WAITING_HUMAN" {
            return Ok(Some(Claim::WaitHuman(id)));
        }
        if process::is_alive(owner).map_err(|e| Error::Invalid(e.to_string()))? {
            return Ok(Some(Claim::Wait(id)));
        }
    }
    Ok(None)
}

async fn open_read_only(
    state: &std::path::Path,
) -> Result<Option<tokio_rusqlite::Connection>, String> {
    let state = crate::workspace::canonical_target(state).map_err(|e| e.to_string())?;
    super::receipts::check_files(&state)?;
    if !state
        .join(super::DATABASE)
        .try_exists()
        .map_err(|e| e.to_string())?
    {
        return Ok(None);
    }
    tokio_rusqlite::Connection::open_with_flags(
        state.join(super::DATABASE),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .await
    .map(Some)
    .map_err(|e| e.to_string())
}

/// Read the latest completed record or the live owner of each key without creating or
/// changing the database.
pub async fn read_keyed_executions(
    state: &std::path::Path,
    keys: &[String],
) -> Result<std::collections::BTreeMap<String, Claim>, String> {
    if keys.is_empty() {
        return Ok(Default::default());
    }
    let Some(connection) = open_read_only(state).await? else {
        return Ok(Default::default());
    };
    let keys = keys.to_vec();
    connection
        .call(move |db| -> Result<_, Error> {
            db.busy_timeout(std::time::Duration::from_secs(5))?;
            let transaction = db.transaction()?;
            if !schema_initialized(&transaction)? {
                return Ok(Default::default());
            }
            let mut entries = std::collections::BTreeMap::new();
            for key in keys {
                if let Some(claim) = available(&transaction, &key)? {
                    entries.insert(key, claim);
                }
            }
            transaction.commit()?;
            Ok(entries)
        })
        .await
        .map_err(|e| e.to_string())
}

/// The latest record per (Eval id, Eval definition hash) under any key, read-only: what a
/// changed key is explained against.
pub async fn read_latest_cached(
    state: &std::path::Path,
    keys: &[(String, String)],
) -> Result<std::collections::BTreeMap<(String, String), Execution>, String> {
    if keys.is_empty() {
        return Ok(Default::default());
    }
    let Some(connection) = open_read_only(state).await? else {
        return Ok(Default::default());
    };
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
                let mut statement = transaction.prepare(&format!(
                    "SELECT e.data FROM cache_entries c JOIN executions e ON e.id=c.execution_id WHERE c.eval_def_hash=? AND json_extract(e.data,'$.provenance.evalId')=? AND e.status IN ('GREEN','RED') ORDER BY {} LIMIT 1",
                    super::cache_entries::LATEST
                ))?;
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
    /// The key's latest record; a hit updates its last use.
    pub async fn cached_execution(&self, key: &str) -> Result<Option<Execution>, String> {
        let key = key.to_owned();
        self.connection
            .call(move |db| -> Result<_, Error> {
                let execution = lookup(db, &key)?;
                if let Some(execution) = &execution {
                    db.execute(
                        "UPDATE cache_entries SET last_used=? WHERE execution_id=?",
                        params![crate::broker::now(), execution.id],
                    )?;
                }
                Ok(execution)
            })
            .await
            .map_err(|e| e.to_string())
    }

    /// The saved execution when its own settle appended it to its key's history; never a
    /// mirror.
    pub async fn published_execution(
        &self,
        execution_id: &str,
    ) -> Result<Option<Execution>, String> {
        let execution_id = execution_id.to_owned();
        self.connection
            .call(move |db| -> Result<_, Error> {
                let data: Option<String> = db
                    .query_row(
                        "SELECT e.data FROM cache_entries c JOIN executions e ON e.id=c.execution_id WHERE c.execution_id=? AND e.status IN ('GREEN','RED')",
                        [execution_id],
                        |row| row.get(0),
                    )
                    .optional()?;
                Ok(data
                    .map(|data| serde_json::from_str::<Execution>(&data))
                    .transpose()?
                    .filter(|execution| execution.origin.is_none()))
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
            let Some(key) = &execution.key else {
                return Ok(if allow_start { Claim::Owned } else { Claim::BudgetExhausted });
            };
            {
                let transaction = db.transaction()?;
                if let Some(claim) = available_to_waiter(&transaction, key, waiting_for.as_deref())? {
                    return Ok(claim);
                }
            }
            if !allow_start {
                return Ok(Claim::BudgetExhausted);
            }
            let transaction = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            if let Some(claim) = available_to_waiter(&transaction, key, waiting_for.as_deref())? {
                return Ok(claim);
            }
            if let Some((id, _, _)) = active_owner(&transaction, key)? {
                transaction.execute(
                    "UPDATE executions SET status='ERROR',data=json_set(data,'$.status','ERROR','$.error','Execution owner died.','$.errorCode','OWNER_DIED','$.completedAt',?,'$.provenance.completedAt',?) WHERE id=? AND status='RUNNING'",
                    params![execution.started_at, execution.started_at, id],
                )?;
            }
            let inserted = transaction.execute(
                "INSERT INTO executions(id,key,owner_pid,owner_start_time,status,data) VALUES (?,?,?,?,?,?) ON CONFLICT(key) WHERE key IS NOT NULL AND status IN ('RUNNING','WAITING_HUMAN') DO NOTHING",
                params![execution.id, key, execution.owner_pid, execution.owner_start_time as i64, execution.status, serde_json::to_string(&execution)?],
            )?;
            let claim = if inserted == 1 {
                Claim::Owned
            } else {
                Claim::Wait(active_owner(&transaction, key)?.expect("conflicting active key").0)
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
                    "UPDATE cache_entries SET last_used=? WHERE execution_id=?",
                    params![request.completed_at, request.execution_id],
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

    /// Append a remote result to its key's history as a self-contained execution when it
    /// completed after the key's latest local record, which it then replaces as the one reused;
    /// `None` when the local record is as new. The mirrored record settles a local Human wait
    /// for the key.
    pub async fn mirror_execution(
        &self,
        execution: &Execution,
    ) -> Result<Option<Execution>, String> {
        let execution = execution.clone();
        self.connection
            .call(move |db| -> Result<Option<Execution>, Error> {
                let (Some(key), Some(_), Some(_)) =
                    (&execution.key, &execution.origin, execution.verdict())
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
                if lookup(&transaction, key)?
                    .is_some_and(|local| !execution.completed_after(&local))
                {
                    transaction.commit()?;
                    return Ok(None);
                }
                // A mirror kept after `cache rm` or GC is reused for the same remote execution.
                transaction.execute(
                    "INSERT INTO executions(id,key,owner_pid,owner_start_time,status,data) VALUES (?,?,?,?,?,?) ON CONFLICT(id) DO NOTHING",
                    params![execution.id, key, execution.owner_pid, execution.owner_start_time as i64, execution.status, data],
                )?;
                let data: String = transaction.query_row(
                    "SELECT data FROM executions WHERE id=? AND key=? AND status=?",
                    params![execution.id, key, execution.status],
                    |row| row.get(0),
                ).optional()?.ok_or_else(|| Error::Invalid("Mirrored execution ID conflicts with another execution.".into()))?;
                let mirrored: Execution = serde_json::from_str(&data)?;
                super::cache_entries::append(&transaction, &mirrored, data.len(), &crate::broker::now())?;
                super::human::settle_waiting(&transaction, &mirrored)?;
                transaction.commit()?;
                if let Err(error) = super::cache_entries::collect(db) {
                    use std::io::Write;
                    let _ = writeln!(std::io::stderr().lock(), "Cache GC failed: {error}");
                }
                Ok(Some(mirrored))
            })
            .await
            .map_err(|e| e.to_string())
    }
}

/// Save a finished execution and its request; a GREEN/RED with a key is appended to the key's
/// history. A claimed execution updates its active row; a forced or unkeyed one is inserted.
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
    let claimed = execution.key.is_some() && !request.force;
    if claimed || request.human_definition.is_some() {
        if db.execute(
            "UPDATE executions SET status=?,data=? WHERE id=? AND owner_pid=? AND owner_start_time=? AND status IN ('RUNNING','WAITING_HUMAN')",
            params![execution.status, data, execution.id, execution.owner_pid, execution.owner_start_time as i64],
        )? != 1
        {
            return Err(Error::Invalid("Active execution not found.".into()));
        }
    } else {
        db.execute(
            "INSERT INTO executions(id,key,owner_pid,owner_start_time,status,data) VALUES (?,?,?,?,?,?)",
            params![execution.id, execution.key, execution.owner_pid, execution.owner_start_time as i64, execution.status, data],
        )?;
    }
    let published = super::cache_entries::append(
        db,
        &execution,
        data.len(),
        execution.completed_at.as_deref().unwrap_or_default(),
    )?;
    update_request(db, &request)?;
    Ok(published)
}
