use crate::types::{ExecutionStatus, Fingerprint, RequestId, ReuseKey, RunId};
use std::{collections::BTreeMap, path::PathBuf};

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    Receipts, Request, history,
    receipts::{Error, schema_initialized, update_request},
};
use crate::{config::Profile, process, runtime::Verdict};

pub enum Claim {
    Owned,
    BudgetExhausted,
    /// Every machine-wide slot of the execution's backend is held.
    Full,
    Reuse(Box<Execution>),
    Wait(crate::types::ExecutionId),
    WaitHuman(crate::types::ExecutionId),
}

/// The machine-wide limit (`limits.json`) of the backend an execution would start on.
pub struct Capacity {
    pub limit: u32,
    /// Whether the Run stopped admitting the backend. It is asked while the claim holds the
    /// write lock, so the stop of a review that records it before it completes is always seen
    /// once that review's slot is free.
    pub stopped: Box<dyn FnOnce() -> bool + Send>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provenance {
    pub repo_path: PathBuf,
    pub run_id: RunId,
    pub request_id: RequestId,
    pub eval_id: String,
    pub eval_def_hash: String,
    pub completed_at: Option<String>,
    /// For Agent results: the SHA-256 of each tool's `executionPaths` when the review started,
    /// by tool name and declared path.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub execution_paths: crate::tools::pins::Pins,
}

/// One execution and, once GREEN or RED with a key, one record of its key's history. A local
/// Agent execution holds one of its backend's machine-wide slots while RUNNING.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Execution {
    pub id: crate::types::ExecutionId,
    /// The reuse key; absent without a fingerprint on every Artifact the eval depends on.
    #[serde(default)]
    pub key: Option<ReuseKey>,
    /// The target Artifact's fingerprint.
    pub fingerprint: Option<Fingerprint>,
    /// Each Artifact the key covers, the target included, with its fingerprint.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fingerprints: BTreeMap<String, Fingerprint>,
    /// Kinds of exactly the Artifacts covered by a new-format reuse key.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub artifact_kinds: BTreeMap<String, crate::config::ArtifactKind>,
    pub eval_def_hash: String,
    pub owner_pid: u32,
    pub owner_start_time: u64,
    pub status: ExecutionStatus,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub error_code: Option<String>,
    pub profile: crate::config::StoredProfile,
    /// How this result was produced; never part of the key.
    #[serde(default)]
    pub options: ExecutionOptions,
    pub usage: Option<Vec<crate::llm::Attempt>>,
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
    #[serde(with = "crate::config::validation::milliseconds")]
    pub timeout_ms: Option<std::time::Duration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tool_calls: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    /// The selected `profileVariants` entry; absent for the declared profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
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
            Profile::Human {} | Profile::Dependency { .. } => {}
        }
        options
    }
}

/// Who produced an execution: display metadata, never authentication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Producer {
    /// `user@host`.
    pub name: String,
    /// The producing artifactize version.
    pub version: String,
    /// Where an Agent review's saved conversation lives. It travels with remote records,
    /// which older clients read without it; the conversation itself never leaves the machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<crate::agent::session::SessionRef>,
}

/// Keep printable producer display metadata bounded in saved records and reuse messages.
const MAX_PRODUCER_CHARS: usize = 200;

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
            name: name
                .chars()
                .filter(|c| !c.is_control())
                .take(MAX_PRODUCER_CHARS)
                .collect(),
            version: env!("CARGO_PKG_VERSION").into(),
            session: None,
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
    /// The backend whose machine-wide slot this execution holds while RUNNING: a local Agent
    /// review's. A mirrored record ran elsewhere.
    fn backend(&self) -> Option<&str> {
        self.options
            .backend
            .as_deref()
            .filter(|_| self.origin.is_none())
    }

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
        match self.status {
            ExecutionStatus::Green => Some(Verdict::Green),
            ExecutionStatus::Red => Some(Verdict::Red),
            _ => None,
        }
    }
}

/// The latest completed record of a key.
pub(super) fn lookup(
    db: &rusqlite::Connection,
    key: &ReuseKey,
) -> Result<Option<Execution>, Error> {
    let data: Option<String> = db
        .query_row(
            &format!(
                "SELECT e.data FROM executions e WHERE e.key=? AND {} ORDER BY {} LIMIT 1",
                history::RECORD,
                history::LATEST
            ),
            [key],
            |row| row.get(0),
        )
        .optional()?;
    data.map(|data| serde_json::from_str(&data))
        .transpose()
        .map_err(Into::into)
}

/// End a RUNNING execution whose owner process is gone, which frees its key and its slot.
fn owner_died(
    db: &rusqlite::Connection,
    id: &crate::types::ExecutionId,
    at: &str,
) -> Result<(), Error> {
    db.execute(
        "UPDATE executions SET status='ERROR',data=json_set(data,'$.status','ERROR','$.error','Execution owner died.','$.errorCode','OWNER_DIED','$.completedAt',?1,'$.provenance.completedAt',?1) WHERE id=?2 AND status='RUNNING'",
        params![at, id],
    )?;
    Ok(())
}

/// How many slots of `backend` the RUNNING executions of live owners hold; the executions of
/// dead owners end, freeing theirs.
fn held_slots(db: &rusqlite::Connection, backend: &str, at: &str) -> Result<u32, Error> {
    let owners = {
        let mut statement = db.prepare(
            "SELECT id,owner_pid,owner_start_time FROM executions WHERE backend=? AND status='RUNNING'",
        )?;
        statement
            .query_map([backend], |row| {
                Ok((
                    row.get::<_, crate::types::ExecutionId>(0)?,
                    process::ChildIdentity {
                        pid: row.get(1)?,
                        start_time: row.get::<_, i64>(2)? as u64,
                    },
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    let mut held = 0;
    for (id, owner) in owners {
        if process::is_alive(owner).map_err(|e| Error::Invalid(e.to_string()))? {
            held += 1;
        } else {
            owner_died(db, &id, at)?;
        }
    }
    Ok(held)
}

fn active_owner(
    db: &rusqlite::Connection,
    key: &ReuseKey,
) -> Result<
    Option<(
        crate::types::ExecutionId,
        process::ChildIdentity,
        ExecutionStatus,
    )>,
    Error,
> {
    db.query_row(
        "SELECT id,owner_pid,owner_start_time,status FROM executions WHERE key=? AND status IN ('RUNNING','WAITING_HUMAN')",
        [key],
        |row| {
            Ok((
                row.get(0)?,
                process::ChildIdentity {
                    pid: row.get(1)?,
                    start_time: row.get::<_, i64>(2)? as u64,
                },
                row.get(3)?,
            ))
        },
    )
    .optional()
    .map_err(Into::into)
}

fn available_to_waiter(
    db: &rusqlite::Connection,
    key: &ReuseKey,
    waiting_for: Option<&crate::types::ExecutionId>,
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

fn available(db: &rusqlite::Connection, key: &ReuseKey) -> Result<Option<Claim>, Error> {
    if let Some(execution) = lookup(db, key)? {
        return Ok(Some(Claim::Reuse(Box::new(execution))));
    }
    if let Some((id, owner, status)) = active_owner(db, key)? {
        if status == ExecutionStatus::WaitingHuman {
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
    keys: &[ReuseKey],
) -> Result<std::collections::BTreeMap<ReuseKey, Claim>, String> {
    if keys.is_empty() {
        return Ok(Default::default());
    }
    let Some(connection) = open_read_only(state).await? else {
        return Ok(Default::default());
    };
    let keys = keys.to_vec();
    connection
        .call(move |db| -> Result<_, Error> {
            db.busy_timeout(super::SQLITE_BUSY_TIMEOUT)?;
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
            db.busy_timeout(super::SQLITE_BUSY_TIMEOUT)?;
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
        let key: ReuseKey = key.parse()?;
        self.connection
            .call(move |db| -> Result<_, Error> {
                let execution = lookup(db, &key)?;
                if let Some(execution) = &execution {
                    history::touch(db, &execution.id, &crate::broker::now())?;
                }
                Ok(execution)
            })
            .await
            .map_err(|e| e.to_string())
    }

    /// The saved execution when its own settle added it to its key's history; never a mirror.
    pub async fn published_execution(
        &self,
        execution_id: &str,
    ) -> Result<Option<Execution>, String> {
        let execution_id: crate::types::ExecutionId = execution_id.parse()?;
        self.connection
            .call(move |db| -> Result<_, Error> {
                let data: Option<String> = db
                    .query_row(
                        &format!(
                            "SELECT e.data FROM executions e WHERE e.id=? AND {}",
                            history::RECORD
                        ),
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

    /// Claim an execution for its request. A `keyed` claim (a key, not forced) first reuses
    /// the key's latest record or waits for the key's live execution. Starting needs
    /// `allow_start` and, on a backend with a machine-wide limit, a free slot: the slots the
    /// backend's RUNNING executions hold are counted and this execution inserted RUNNING in
    /// one write transaction. An unkeyed or forced execution without a limit is saved when it
    /// completes.
    pub async fn claim_execution(
        &self,
        execution: &Execution,
        keyed: bool,
        waiting_for: Option<&crate::types::ExecutionId>,
        allow_start: bool,
        capacity: Option<Capacity>,
    ) -> Result<Claim, String> {
        execution.validate()?;
        let execution = execution.clone();
        let waiting_for = waiting_for.cloned();
        self.connection
            .call(move |db| -> Result<Claim, Error> {
                let key = execution.key.as_ref().filter(|_| keyed);
                if let Some(key) = key {
                    let transaction = db.transaction()?;
                    if let Some(claim) =
                        available_to_waiter(&transaction, key, waiting_for.as_ref())?
                    {
                        return Ok(claim);
                    }
                }
                if !allow_start {
                    return Ok(Claim::BudgetExhausted);
                }
                let capacity = capacity.zip(execution.backend());
                if key.is_none() && capacity.is_none() {
                    return Ok(Claim::Owned);
                }
                let transaction =
                    db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                if let Some(key) = key
                    && let Some(claim) =
                        available_to_waiter(&transaction, key, waiting_for.as_ref())?
                {
                    return Ok(claim);
                }
                if let Some((capacity, backend)) = capacity {
                    if (capacity.stopped)() {
                        return Ok(Claim::BudgetExhausted);
                    }
                    if held_slots(&transaction, backend, &execution.started_at)? >= capacity.limit {
                        transaction.commit()?;
                        return Ok(Claim::Full);
                    }
                }
                if let Some(key) = key
                    && let Some((id, _, _)) = active_owner(&transaction, key)?
                {
                    owner_died(&transaction, &id, &execution.started_at)?;
                }
                let inserted = transaction.execute(
                    "INSERT INTO executions(id,key,eval_def_hash,status,owner_pid,owner_start_time,backend,data) VALUES (?,?,?,?,?,?,?,?) ON CONFLICT(key) WHERE key IS NOT NULL AND status IN ('RUNNING','WAITING_HUMAN') DO NOTHING",
                    params![
                        execution.id,
                        key,
                        execution.eval_def_hash,
                        execution.status,
                        execution.owner_pid,
                        execution.owner_start_time as i64,
                        execution.backend(),
                        serde_json::to_string(&execution)?
                    ],
                )?;
                let claim = match key {
                    Some(key) if inserted == 0 => Claim::Wait(
                        active_owner(&transaction, key)?
                            .expect("conflicting active key")
                            .0,
                    ),
                    _ => Claim::Owned,
                };
                transaction.commit()?;
                Ok(claim)
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
                if let (Some(execution), Some(at)) = (&request.execution_id, &request.completed_at)
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
                let published = settle(&transaction, &execution, &request)?;
                transaction.commit()?;
                if published {
                    history::collect_after(db);
                }
                Ok(())
            })
            .await
            .map_err(|e| e.to_string())
    }

    /// Add a remote result to its key's history as a self-contained execution when it
    /// completed after the key's latest local record, which it then replaces as the one reused;
    /// `None` when the local record is as new. The mirrored record settles a local Human wait
    /// for the key.
    pub async fn mirror_execution(
        &self,
        execution: &Execution,
    ) -> Result<Option<Execution>, String> {
        execution.validate()?;
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
                if data.len() > history::MAX_ENTRY_BYTES {
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
                    "INSERT INTO executions(id,key,eval_def_hash,status,owner_pid,owner_start_time,data) VALUES (?,?,?,?,?,?,?) ON CONFLICT(id) DO NOTHING",
                    params![
                        execution.id,
                        key,
                        execution.eval_def_hash,
                        execution.status,
                        execution.owner_pid,
                        execution.owner_start_time as i64,
                        data
                    ],
                )?;
                let data: String = transaction
                    .query_row(
                        "SELECT data FROM executions WHERE id=? AND key=? AND status=?",
                        params![execution.id, key, execution.status],
                        |row| row.get(0),
                    )
                    .optional()?
                    .ok_or_else(|| {
                        Error::Invalid(
                            "Mirrored execution ID conflicts with another execution.".into(),
                        )
                    })?;
                let mirrored: Execution = serde_json::from_str(&data)?;
                if let Some((completed_at, bytes)) = history::columns(&mirrored, data.len())? {
                    transaction.execute(
                        "UPDATE executions SET completed_at=?,bytes=?,last_used=? WHERE id=? AND completed_at IS NULL",
                        params![completed_at, bytes, crate::broker::now(), mirrored.id],
                    )?;
                }
                super::human::settle_waiting(&transaction, &mirrored)?;
                transaction.commit()?;
                history::collect_after(db);
                Ok(Some(mirrored))
            })
            .await
            .map_err(|e| e.to_string())
    }
}

/// Save a finished execution and its request; a GREEN/RED with a key joins the key's history.
/// A claimed or Human execution updates its active row; a forced or unkeyed one is inserted,
/// or replaces the RUNNING row its limited start inserted. True when it joined the history.
pub(super) fn settle(
    db: &rusqlite::Connection,
    execution: &Execution,
    request: &Request,
) -> Result<bool, Error> {
    execution.validate().map_err(Error::Invalid)?;
    request.validate().map_err(Error::Invalid)?;
    if request.status != execution.status.into()
        || request.execution_id.as_ref() != Some(&execution.id)
        || request.result != execution.result
        || request.error != execution.error
        || request.error_code != execution.error_code
    {
        return Err(Error::Invalid(
            "Execution and request settlement disagree.".into(),
        ));
    }
    let data = serde_json::to_string(execution)?;
    let record = history::columns(execution, data.len())?;
    let last_used = record.as_ref().and(execution.completed_at.as_deref());
    let (completed_at, bytes) = record.clone().unzip();
    let claimed = execution.key.is_some() && !request.force;
    if claimed || request.human_definition.is_some() {
        if db.execute(
            "UPDATE executions SET key=?,status=?,completed_at=?,bytes=?,last_used=?,data=? WHERE id=? AND owner_pid=? AND owner_start_time=? AND status IN ('RUNNING','WAITING_HUMAN')",
            params![
                execution.key,
                execution.status,
                completed_at,
                bytes,
                last_used,
                data,
                execution.id,
                execution.owner_pid,
                execution.owner_start_time as i64
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
                execution.status,
                execution.owner_pid,
                execution.owner_start_time as i64,
                execution.backend(),
                completed_at,
                bytes,
                last_used,
                data
            ],
        )?;
    }
    update_request(db, request)?;
    Ok(record.is_some())
}
