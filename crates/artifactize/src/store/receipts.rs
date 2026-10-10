use crate::types::{
    Fingerprint, RequestId, RequestStatus, ReuseKey, RunId, RunStatus, SessionId, Timestamp,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::Duration,
};

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_rusqlite::Connection;

use super::{ExecutionOptions, Origin, Producer, Provenance, STATE_SCHEMA_VERSION};
use crate::{
    config::ProfileKind,
    workspace::{canonical_target, outside_workspace, prepare_directory},
};

pub const DATABASE: &str = "state.sqlite";
/// Yield between WAL initialization races instead of spinning, still within the
/// shared SQLite contention deadline used by the outer initialization loop.
const INITIALIZATION_RETRY_INTERVAL: Duration = Duration::from_millis(20);

/// The four tables of a state and the indexes its lookups use.
///
/// - `runs`: one row per Run.
/// - `requests`: one row per eval of a Run, at its `ordinal` in the Run's selection order;
///   `claimed_by` and `claimed_at` hold the Human claim of a waiting request.
/// - `executions`: one row per execution. `backend` names the Agent backend it runs on, and
///   a RUNNING execution with a backend holds one of that backend's machine-wide slots. A
///   completed GREEN/RED execution with a key is a record of the key's history while
///   `completed_at` (sortable), `bytes` and `last_used` are set; the cache GC and `cache rm`
///   clear them, and the execution stays.
/// - `state_meta`: named values of the state itself: `id`, a random UUID made when the
///   database is created, which identifies the state in Agent session references.
const SCHEMA: &str =
    "CREATE TABLE runs(id TEXT PRIMARY KEY, repo TEXT NOT NULL, status TEXT NOT NULL, data TEXT NOT NULL);
    CREATE TABLE requests(id TEXT PRIMARY KEY, run_id TEXT NOT NULL REFERENCES runs(id), eval_id TEXT NOT NULL, ordinal INTEGER NOT NULL, execution_id TEXT REFERENCES executions(id), status TEXT NOT NULL, claimed_by TEXT, claimed_at TEXT, data TEXT NOT NULL, UNIQUE(run_id, eval_id), UNIQUE(run_id, ordinal));
    CREATE TABLE executions(id TEXT PRIMARY KEY, key TEXT, eval_def_hash TEXT NOT NULL, status TEXT NOT NULL, owner_pid INTEGER NOT NULL, owner_start_time INTEGER NOT NULL, backend TEXT, completed_at TEXT, bytes INTEGER, last_used TEXT, data TEXT NOT NULL);
    CREATE TABLE state_meta(name TEXT PRIMARY KEY, value TEXT NOT NULL);
    CREATE INDEX active_request_execution ON requests(execution_id) WHERE status IN ('QUEUED','RUNNING','WAITING_HUMAN');
    CREATE UNIQUE INDEX active_key ON executions(key) WHERE key IS NOT NULL AND status IN ('RUNNING','WAITING_HUMAN');
    CREATE INDEX key_history ON executions(key, completed_at);
    CREATE INDEX definition_history ON executions(eval_def_hash, completed_at);
    CREATE INDEX running_backend ON executions(backend) WHERE status='RUNNING';";

/// Why a state written by an earlier artifactize is refused: no version migrates.
pub const EARLIER_STATE: &str = "This state was written by an earlier artifactize. Start a new state (set ARTIFACTIZE_STATE_HOME or move the old one away). artifactize does not migrate it.";

/// Why a state database of schema `version`, neither this one nor uninitialized, is refused.
pub fn schema_error(version: u32) -> String {
    match version {
        0 => "Unsupported state schema version: 0.".into(),
        version if version < STATE_SCHEMA_VERSION => EARLIER_STATE.into(),
        version => {
            format!("Unsupported state schema version: {version}; a newer artifactize wrote it.")
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Invalid(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: RunId,
    pub repo_path: PathBuf,
    /// Optional display metadata; schema 5 Runs written before this remain readable.
    #[serde(flatten)]
    pub repository: crate::repository::Identity,
    pub state_dir: PathBuf,
    pub status: RunStatus,
    pub created_at: Timestamp,
    pub completed_at: Option<Timestamp>,
    pub selection: crate::project::selection::Selection,
    #[serde(default)]
    pub profile: Option<crate::project::selection::ProfileSelection>,
    #[serde(default)]
    pub definitions: super::definitions::Definitions,
    #[serde(default)]
    pub recursive: bool,
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub ignore_gates: bool,
    #[serde(default = "default_jobs")]
    pub jobs: usize,
    /// Fingerprints computed at once; absent in Runs saved before 0.5.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint_jobs: Option<usize>,
    #[serde(default)]
    pub max_executions: Option<u64>,
    #[serde(default)]
    pub executions_started: u64,
    #[serde(default, with = "super::wait_timeout")]
    pub wait_timeout_ms: Option<Duration>,
    #[serde(default)]
    pub wait_timed_out: bool,
    /// Kinds whose evals only reuse a result; one with nothing to reuse is not executed.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub reuse_only: BTreeSet<ProfileKind>,
    pub validation: Value,
    pub error: Option<String>,
    /// Agent backends this Run stopped admitting reviews on, in the order they stopped.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stopped_backends: Vec<StoppedBackend>,
    /// Verdicts the Run took from saved results for evals it has no request for, such as the
    /// dependencies of a partial Run, so readers judge gates on the Run's own evidence.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub evidence: BTreeMap<String, crate::types::RequestStatus>,
}

/// An Agent backend a Run stopped after an AUTHENTICATION or QUOTA failure, which every
/// later review on it would repeat.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoppedBackend {
    pub backend: String,
    pub error_code: String,
    pub eval_id: String,
    pub request_id: RequestId,
    pub error: String,
}

fn default_jobs() -> usize {
    crate::project::DEFAULT_JOBS
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub id: RequestId,
    pub run_id: RunId,
    pub eval_id: String,
    pub target: String,
    pub title: String,
    pub profile: crate::config::StoredProfile,
    pub requested_profile: crate::config::StoredProfile,
    pub eval_def_hash: String,
    /// The requested execution options, or a reused result's.
    #[serde(default)]
    pub options: ExecutionOptions,
    pub execution_id: Option<crate::types::ExecutionId>,
    pub provenance: Option<Provenance>,
    /// Usage spent by this request; a reused request spent none.
    pub usage: Option<Vec<crate::llm::Attempt>>,
    /// The reused execution's original usage, never counted as spent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reused_usage: Option<Vec<crate::llm::Attempt>>,
    /// The reused result came from the live execution this request waited for, not a
    /// completed record. Saved only; output reports it as `source.kind` ([`crate::query::source`]).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub joined: bool,
    /// Who produced a reused result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer: Option<Producer>,
    /// The Human claimant who submitted a reused result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer: Option<String>,
    /// The remote store, publisher and publication time of a result reused from a mirror.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Origin>,
    /// The Agent review's session id ([`crate::agent::session_id`]), which every request of
    /// the review sends as its prompt-cache identity. Absent when no Agent review ran, as on
    /// reuse, and on requests saved before it existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<SessionId>,
    /// Where the saved conversation behind this request's result lives: its own review's,
    /// or for a reused result the producing review's. Absent when none was saved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<crate::agent::session::SessionRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub human_definition: Option<Value>,
    pub payload: crate::config::StoredPayload,
    pub references: Value,
    pub deps: Vec<String>,
    #[serde(default)]
    pub force: bool,
    /// The target Artifact's fingerprint.
    #[serde(default)]
    pub fingerprint: Option<Fingerprint>,
    /// The reuse key; absent without a fingerprint on every Artifact the eval depends on.
    #[serde(default)]
    pub key: Option<ReuseKey>,
    /// Each Artifact the key covers, with its fingerprint.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub fingerprints: std::collections::BTreeMap<String, Fingerprint>,
    pub status: RequestStatus,
    pub created_at: Timestamp,
    pub started_at: Option<Timestamp>,
    pub completed_at: Option<Timestamp>,
    pub cwd: PathBuf,
    pub run_dir: Option<PathBuf>,
    pub argv: Option<Vec<String>>,
    pub child: Option<Value>,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub error_code: Option<String>,
    pub blocked_reason: Option<String>,
    /// Unfulfilled Artifacts and Evals behind a derived dependency verdict.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_by: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RunView {
    #[serde(flatten)]
    pub run: Run,
    pub requests: Vec<Request>,
}

#[derive(Clone)]
pub struct Receipts {
    pub(super) connection: NotifyingConnection,
}

/// Notify at the DB-worker completion boundary, even if the awaiting caller was cancelled.
/// total_changes is conservative (rolled-back changes can invalidate harmlessly). Only a
/// finished autocommit edge can publish; no event is an audit record or proof of success.
#[derive(Clone)]
pub(crate) struct NotifyingConnection {
    inner: Connection,
    publisher: crate::changes::Publisher,
}
impl NotifyingConnection {
    pub(crate) fn new(inner: Connection, state: &Path) -> Self {
        Self {
            inner,
            publisher: crate::changes::Publisher::new(state),
        }
    }
    pub(crate) async fn call<F, R, E>(&self, function: F) -> Result<R, tokio_rusqlite::Error<E>>
    where
        F: FnOnce(&mut rusqlite::Connection) -> Result<R, E> + Send + 'static,
        R: Send + 'static,
        E: Send + 'static,
    {
        let publisher = self.publisher.clone();
        self.inner
            .call(move |db| {
                let before = db.total_changes();
                let result = function(db);
                if db.is_autocommit() && db.total_changes() != before {
                    publisher.notify(crate::changes::Change::StateInvalidated);
                }
                result
            })
            .await
    }
}

impl Receipts {
    pub async fn open(state: &Path, repo: &Path) -> Result<Self, String> {
        let state = prepare_directory(state, repo).map_err(|e| e.to_string())?;
        check_files(&state)?;
        let connection = Connection::open(state.join(DATABASE))
            .await
            .map_err(|e| e.to_string())?;
        let connection = NotifyingConnection::new(connection, &state);
        // WAL initialization retries share the database contention budget.
        let deadline = tokio::time::Instant::now() + super::SQLITE_BUSY_TIMEOUT;
        let state_id = crate::agent::uuid()?;
        loop {
            let state_id = state_id.clone();
            let initialized = connection
                .call(move |db| -> Result<(), Error> {
                    db.busy_timeout(super::SQLITE_BUSY_TIMEOUT)?;
                    db.pragma_update(None, "foreign_keys", true)?;
                    let version: u32 =
                        db.pragma_query_value(None, "user_version", |row| row.get(0))?;
                    if version != 0 && version != STATE_SCHEMA_VERSION {
                        return Err(Error::Invalid(schema_error(version)));
                    }
                    db.pragma_update(None, "journal_mode", "WAL")?;
                    let transaction =
                        db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                    // Publish the schema, the state's id and the version together; readers see
                    // an empty snapshot until commit.
                    if !schema_initialized(&transaction)? {
                        transaction.execute_batch(SCHEMA)?;
                        transaction.execute(
                            "INSERT INTO state_meta(name,value) VALUES ('id',?)",
                            [&state_id],
                        )?;
                        transaction.pragma_update(None, "user_version", STATE_SCHEMA_VERSION)?;
                    }
                    transaction.commit()?;
                    Ok(())
                })
                .await;
            match initialized {
                Ok(()) => break,
                Err(tokio_rusqlite::Error::Error(Error::Sql(rusqlite::Error::SqliteFailure(
                    error,
                    _,
                )))) if error.code == rusqlite::ErrorCode::DatabaseBusy
                    && tokio::time::Instant::now() < deadline =>
                {
                    // Concurrent first opens can race when enabling WAL despite busy_timeout.
                    tokio::time::sleep(INITIALIZATION_RETRY_INTERVAL).await;
                }
                Err(error) => return Err(error.to_string()),
            }
        }
        Ok(Self { connection })
    }

    /// This state's stable id ([`read_state_id`]).
    pub async fn state_id(&self) -> Result<String, String> {
        self.connection
            .call(|db| -> Result<String, Error> {
                Ok(
                    db.query_row("SELECT value FROM state_meta WHERE name='id'", [], |row| {
                        row.get(0)
                    })?,
                )
            })
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn create_run(&self, run: &Run, requests: &[Request]) -> Result<(), String> {
        run.validate()?;
        for request in requests {
            request.validate()?;
            if request.run_id != run.id {
                return Err("Request belongs to another Run.".into());
            }
        }
        let run = run.clone();
        let requests = requests.to_vec();
        self.connection
            .call(move |db| -> Result<(), Error> {
                let transaction = db.transaction()?;
                transaction.execute(
                    "INSERT INTO runs(id,repo,status,data) VALUES (?,?,?,?)",
                    params![
                        run.id,
                        crate::platform::path_text(&run.repo_path),
                        run.status,
                        serde_json::to_string(&run)?
                    ],
                )?;
                for (ordinal, request) in requests.iter().enumerate() {
                    transaction.execute(
                        "INSERT INTO requests(id,run_id,eval_id,ordinal,status,data) VALUES (?,?,?,?,?,?)",
                        params![
                            request.id,
                            run.id,
                            request.eval_id,
                            ordinal as i64,
                            request.status,
                            serde_json::to_string(request)?
                        ],
                    )?;
                }
                transaction.commit()?;
                Ok(())
            })
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn save_request(&self, request: &Request) -> Result<(), String> {
        let request = request.clone();
        self.connection
            .call(move |db| update_request(db, &request))
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn save_run(&self, run: &Run) -> Result<(), String> {
        run.validate()?;
        let run = run.clone();
        self.connection
            .call(move |db| -> Result<(), Error> {
                db.execute(
                    "UPDATE runs SET status=?,data=? WHERE id=?",
                    params![run.status, serde_json::to_string(&run)?, run.id],
                )?;
                Ok(())
            })
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn finish(&self, run: &Run, requests: &[Request]) -> Result<(), String> {
        run.validate()?;
        let run = run.clone();
        let requests = requests.to_vec();
        self.connection
            .call(move |db| -> Result<(), Error> {
                let transaction = db.transaction()?;
                for request in requests {
                    // A concurrent submission may already have settled this saved request.
                    if request.status == crate::types::RequestStatus::WaitingHuman {
                        continue;
                    }
                    update_request(&transaction, &request)?;
                }
                transaction.execute(
                    "UPDATE runs SET status=?,data=? WHERE id=?",
                    params![run.status, serde_json::to_string(&run)?, run.id],
                )?;
                transaction.commit()?;
                Ok(())
            })
            .await
            .map_err(|e| e.to_string())
    }
}

pub(super) fn update_request(db: &rusqlite::Connection, request: &Request) -> Result<(), Error> {
    request.validate().map_err(Error::Invalid)?;
    if db.execute(
        "UPDATE requests SET status=?,data=?,execution_id=? WHERE id=?",
        params![
            request.status,
            serde_json::to_string(request)?,
            request.execution_id,
            request.id
        ],
    )? != 1
    {
        return Err(Error::Invalid("Review request not found.".into()));
    }
    Ok(())
}

pub(super) fn schema_initialized(transaction: &rusqlite::Transaction<'_>) -> Result<bool, Error> {
    let version: u32 = transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
    // Check both in one snapshot: initialization may commit between separate reads.
    if version == 0
        && !transaction.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master)", [], |row| {
            row.get::<_, bool>(0)
        })?
    {
        return Ok(false);
    }
    if version != STATE_SCHEMA_VERSION {
        return Err(Error::Invalid(schema_error(version)));
    }
    Ok(true)
}

/// Reject non-regular state files and a database of another schema before any read or write.
pub(super) fn check_files(state: &Path) -> Result<(), String> {
    match state_schema(state)? {
        Some(version) if version != 0 && version != STATE_SCHEMA_VERSION => {
            Err(schema_error(version))
        }
        _ => Ok(()),
    }
}

/// The state database's schema version, read without creating it; `None` without a database.
/// `doctor` reports it.
pub fn state_schema(state: &Path) -> Result<Option<u32>, String> {
    let state = canonical_target(state).map_err(|e| e.to_string())?;
    regular_files(&state)?;
    let database = state.join(DATABASE);
    if !database.try_exists().map_err(|e| e.to_string())? {
        return Ok(None);
    }
    let read = || -> Result<u32, rusqlite::Error> {
        let db = rusqlite::Connection::open_with_flags(
            &database,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        db.busy_timeout(super::SQLITE_BUSY_TIMEOUT)?;
        db.pragma_query_value(None, "user_version", |row| row.get(0))
    };
    read().map(Some).map_err(|e| e.to_string())
}

/// The state's stable id, read without creating anything; `None` until a command that writes
/// the state has opened it.
pub fn read_state_id(state: &Path) -> Result<Option<String>, String> {
    let state = canonical_target(state).map_err(|e| e.to_string())?;
    check_files(&state)?;
    let database = state.join(DATABASE);
    if !database.try_exists().map_err(|e| e.to_string())? {
        return Ok(None);
    }
    let read = || -> Result<Option<String>, Error> {
        let mut db = rusqlite::Connection::open_with_flags(
            &database,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        db.busy_timeout(super::SQLITE_BUSY_TIMEOUT)?;
        let transaction = db.transaction()?;
        if !schema_initialized(&transaction)? {
            return Ok(None);
        }
        Ok(transaction
            .query_row("SELECT value FROM state_meta WHERE name='id'", [], |row| {
                row.get(0)
            })
            .optional()?)
    };
    read().map_err(|e| e.to_string())
}

pub(crate) fn regular_files(state: &Path) -> Result<(), String> {
    for suffix in ["", "-wal", "-shm"] {
        let path = state.join(format!("{DATABASE}{suffix}"));
        match path.symlink_metadata() {
            Ok(metadata) if !metadata.is_file() => {
                return Err(format!(
                    "State files must be regular files: {}",
                    path.display()
                ));
            }
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(error.to_string());
            }
            _ => {}
        }
    }
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LastRequest {
    pub run_id: RunId,
    pub verdict: RequestStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<Fingerprint>,
}

/// Last attempts are audit pointers, never evidence for a new current-input query.
pub async fn read_latest_requests(
    state: &Path,
    repo: &Path,
) -> Result<std::collections::BTreeMap<String, LastRequest>, String> {
    let state = canonical_target(state).map_err(|e| e.to_string())?;
    outside_workspace(repo, &state).map_err(|e| e.to_string())?;
    check_files(&state)?;
    if !state
        .join(DATABASE)
        .try_exists()
        .map_err(|e| e.to_string())?
    {
        return Ok(Default::default());
    }
    let connection = Connection::open_with_flags(
        state.join(DATABASE),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .await
    .map_err(|e| e.to_string())?;
    let repo = repo.to_path_buf();
    connection
        .call(move |db| -> Result<_, Error> {
            db.busy_timeout(super::SQLITE_BUSY_TIMEOUT)?;
            let transaction = db.transaction()?;
            if !schema_initialized(&transaction)? {
                return Ok(Default::default());
            }
            let latest = {
                let mut statement = transaction.prepare(
                    "SELECT eval_id,run_id,status,fingerprint FROM (
                SELECT q.eval_id,q.run_id,q.status,json_extract(q.data, '$.fingerprint') AS fingerprint,
                    row_number() OVER (PARTITION BY q.eval_id ORDER BY r.rowid DESC) AS rank
                FROM requests q JOIN runs r ON r.id=q.run_id
                WHERE r.repo=?
            ) WHERE rank=1 ORDER BY eval_id",
                )?;
                statement
                    .query_map([crate::platform::path_text(&repo)], |row| {
                        Ok((
                            row.get(0)?,
                            LastRequest {
                                run_id: row.get(1)?,
                                verdict: row.get(2)?,
                                fingerprint: row.get(3)?,
                            },
                        ))
                    })?
                    .collect::<Result<_, _>>()?
            };
            transaction.commit()?;
            Ok(latest)
        })
        .await
        .map_err(|e| e.to_string())
}

/// Existing store only: no discovery, schema creation, or execution.
pub async fn read_run(state: &Path, id: &str) -> Result<RunView, String> {
    let state = canonical_target(state).map_err(|e| e.to_string())?;
    check_files(&state)?;
    let connection = Connection::open_with_flags(
        state.join(DATABASE),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .await
    .map_err(|e| e.to_string())?;
    let id: RunId = id.parse()?;
    connection
        .call(move |db| -> Result<RunView, Error> {
            db.busy_timeout(super::SQLITE_BUSY_TIMEOUT)?;
            let transaction = db.transaction()?;
            if !schema_initialized(&transaction)? {
                return Err(Error::Invalid("Run not found.".into()));
            }
            let saved: Option<(String, String)> = transaction
                .query_row("SELECT repo,data FROM runs WHERE id=?", [&id], |row| {
                    Ok((row.get(0)?, row.get(1)?))
                })
                .optional()?;
            let (repo, data) = saved.ok_or_else(|| Error::Invalid("Run not found.".into()))?;
            outside_workspace(Path::new(&repo), &state)
                .map_err(|e| Error::Invalid(e.to_string()))?;
            let run = serde_json::from_str(&data)?;
            let requests = {
                let mut statement = transaction
                    .prepare("SELECT data FROM requests WHERE run_id=? ORDER BY ordinal")?;
                statement
                    .query_map([&id], |row| row.get::<_, String>(0))?
                    .map(|row| Ok(serde_json::from_str(&row?)?))
                    .collect::<Result<Vec<Request>, Error>>()?
            };
            transaction.commit()?;
            Ok(RunView { run, requests })
        })
        .await
        .map_err(|e| e.to_string())
}
