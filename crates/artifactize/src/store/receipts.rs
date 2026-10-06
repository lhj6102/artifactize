use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_rusqlite::Connection;

use super::{ExecutionOptions, Origin, Producer, Provenance, STATE_SCHEMA_VERSION};
use crate::workspace::{canonical_target, outside_workspace, prepare_directory};

pub const DATABASE: &str = "state.sqlite";

/// Named values of the state itself: `id`, a random UUID made when a database is first opened
/// for writing, which identifies the state in Agent session references.
const STATE_META: &str =
    "CREATE TABLE IF NOT EXISTS state_meta(name TEXT PRIMARY KEY, value TEXT NOT NULL);";

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
    pub id: String,
    pub repo_path: PathBuf,
    pub state_dir: PathBuf,
    pub status: String,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub selection: Value,
    #[serde(default)]
    pub profile: Value,
    #[serde(default)]
    pub definitions: Value,
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
    #[serde(default)]
    pub wait_timeout_ms: Option<u32>,
    #[serde(default)]
    pub wait_timed_out: bool,
    pub validation: Value,
    pub error: Option<String>,
    /// Agent backends this Run stopped admitting reviews on, in the order they stopped.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stopped_backends: Vec<StoppedBackend>,
}

/// An Agent backend a Run stopped after an AUTHENTICATION or QUOTA failure, which every
/// later review on it would repeat.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoppedBackend {
    pub backend: String,
    pub error_code: String,
    pub eval_id: String,
    pub request_id: String,
    pub error: String,
}

fn default_jobs() -> usize {
    4
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub id: String,
    pub run_id: String,
    pub eval_id: String,
    pub target: String,
    pub title: String,
    pub profile: Value,
    pub requested_profile: Value,
    pub eval_def_hash: String,
    /// The requested execution options, or a reused result's.
    #[serde(default)]
    pub options: ExecutionOptions,
    pub execution_id: Option<String>,
    pub provenance: Option<Provenance>,
    /// Usage spent by this request; a reused request spent none.
    pub usage: Option<Value>,
    /// The reused execution's original usage, never counted as spent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reused_usage: Option<Value>,
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
    #[serde(default)]
    pub tool_calls: Vec<Value>,
    /// The Agent review's session id ([`crate::agent::session_id`]), which every request of
    /// the review sends as its prompt-cache identity. Absent when no Agent review ran, as on
    /// reuse, and on requests saved before it existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Where the saved conversation behind this request's result lives: its own review's,
    /// or for a reused result the producing review's. Absent when none was saved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<crate::agent::session::SessionRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub human_definition: Option<Value>,
    pub payload: Value,
    pub references: Value,
    pub deps: Vec<String>,
    #[serde(default)]
    pub force: bool,
    /// The target Artifact's fingerprint.
    #[serde(default)]
    pub fingerprint: Option<String>,
    /// The reuse key; absent without a fingerprint on every Artifact the eval depends on.
    #[serde(default)]
    pub key: Option<String>,
    /// Each Artifact the key covers, with its fingerprint.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub fingerprints: std::collections::BTreeMap<String, String>,
    pub status: String,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub cwd: PathBuf,
    pub run_dir: Option<PathBuf>,
    pub argv: Option<Vec<String>>,
    pub child: Option<Value>,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub error_code: Option<String>,
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RunView {
    #[serde(flatten)]
    pub run: Run,
    pub requests: Vec<Request>,
}

#[derive(Clone)]
pub struct Receipts {
    pub(super) connection: Connection,
}

impl Receipts {
    pub async fn open(state: &Path, repo: &Path) -> Result<Self, String> {
        let state = prepare_directory(state, repo).map_err(|e| e.to_string())?;
        check_files(&state)?;
        let connection = Connection::open(state.join(DATABASE))
            .await
            .map_err(|e| e.to_string())?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        let state_id = crate::agent::uuid()?;
        loop {
            let state_id = state_id.clone();
            let initialized = connection.call(move |db| -> Result<(), Error> {
            db.busy_timeout(Duration::from_secs(5))?;
            db.pragma_update(None, "foreign_keys", true)?;
            let version: u32 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
            if version != 0 && version != STATE_SCHEMA_VERSION {
                return Err(Error::Invalid(format!("Unsupported state schema version: {version}")));
            }
            db.pragma_update(None, "journal_mode", "WAL")?;
            let transaction = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            schema_initialized(&transaction)?;
            // Publish the schema and its version together; readers see an empty snapshot until commit.
            transaction.execute_batch("CREATE TABLE IF NOT EXISTS runs(id TEXT PRIMARY KEY, repo TEXT NOT NULL, status TEXT NOT NULL, data TEXT NOT NULL);
                CREATE TABLE IF NOT EXISTS executions(id TEXT PRIMARY KEY, key TEXT, owner_pid INTEGER NOT NULL, owner_start_time INTEGER NOT NULL, status TEXT NOT NULL, data TEXT NOT NULL);
                CREATE TABLE IF NOT EXISTS requests(id TEXT PRIMARY KEY, run_id TEXT NOT NULL REFERENCES runs(id), execution_id TEXT REFERENCES executions(id), status TEXT NOT NULL, data TEXT NOT NULL);
                CREATE INDEX IF NOT EXISTS active_request_execution ON requests(execution_id) WHERE status IN ('QUEUED','RUNNING','WAITING_HUMAN');
                CREATE TABLE IF NOT EXISTS human_claims(request_id TEXT PRIMARY KEY REFERENCES requests(id), reviewer TEXT NOT NULL, claimed_at TEXT NOT NULL);
                CREATE TABLE IF NOT EXISTS mcp_sessions(execution_id TEXT PRIMARY KEY, binding TEXT NOT NULL, max_calls INTEGER, started INTEGER NOT NULL);
                CREATE TABLE IF NOT EXISTS tool_calls(execution_id TEXT NOT NULL REFERENCES mcp_sessions(execution_id), ordinal INTEGER NOT NULL, data TEXT NOT NULL, PRIMARY KEY(execution_id,ordinal));
                CREATE TABLE IF NOT EXISTS run_members(run_id TEXT NOT NULL REFERENCES runs(id), eval_id TEXT NOT NULL, ordinal INTEGER NOT NULL, request_id TEXT NOT NULL REFERENCES requests(id), PRIMARY KEY(run_id, eval_id), UNIQUE(run_id, ordinal));")?;
            transaction.execute_batch(super::cache_entries::SCHEMA)?;
            transaction.execute_batch(super::slots::SCHEMA)?;
            // The state's stable id, which Agent session references name: one per database.
            transaction.execute_batch(STATE_META)?;
            transaction.execute("INSERT OR IGNORE INTO state_meta(name,value) VALUES ('id',?)", [&state_id])?;
            transaction.pragma_update(None, "user_version", STATE_SCHEMA_VERSION)?;
            transaction.commit()?;
            Ok(())
            }).await;
            match initialized {
                Ok(()) => break,
                Err(tokio_rusqlite::Error::Error(Error::Sql(rusqlite::Error::SqliteFailure(
                    error,
                    _,
                )))) if error.code == rusqlite::ErrorCode::DatabaseBusy
                    && tokio::time::Instant::now() < deadline =>
                {
                    // Concurrent first opens can race when enabling WAL despite busy_timeout.
                    tokio::time::sleep(Duration::from_millis(20)).await;
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
        let run = run.clone();
        let requests = requests.to_vec();
        self.connection.call(move |db| -> Result<(), Error> {
            let transaction = db.transaction()?;
            transaction.execute("INSERT INTO runs(id,repo,status,data) VALUES (?,?,?,?)", params![run.id, run.repo_path.to_string_lossy(), run.status, serde_json::to_string(&run)?])?;
            for (ordinal, request) in requests.iter().enumerate() {
                transaction.execute("INSERT INTO requests(id,run_id,status,data) VALUES (?,?,?,?)", params![request.id, run.id, request.status, serde_json::to_string(request)?])?;
                transaction.execute("INSERT INTO run_members(run_id,eval_id,ordinal,request_id) VALUES (?,?,?,?)", params![run.id, request.eval_id, ordinal as i64, request.id])?;
            }
            transaction.commit()?;
            Ok(())
        }).await.map_err(|e| e.to_string())
    }

    pub async fn save_request(&self, request: &Request) -> Result<(), String> {
        let request = request.clone();
        self.connection
            .call(move |db| update_request(db, &request))
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn save_run(&self, run: &Run) -> Result<(), String> {
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
        let run = run.clone();
        let requests = requests.to_vec();
        self.connection
            .call(move |db| -> Result<(), Error> {
                let transaction = db.transaction()?;
                for request in requests {
                    // A concurrent submission may already have settled this saved request.
                    if request.status == "WAITING_HUMAN" {
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
        return Err(Error::Invalid(format!(
            "Unsupported state schema version: {version}"
        )));
    }
    Ok(true)
}

/// Reject non-regular state files, then upgrade an older schema before any read or write.
pub(super) fn check_files(state: &Path) -> Result<(), String> {
    regular_files(state)?;
    super::migrate::upgrade(&state.join(DATABASE)).map_err(|e| e.to_string())
}

/// The state database's schema version, read without upgrading or creating it; `None`
/// without a database. `doctor` reports it.
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
        db.busy_timeout(Duration::from_secs(5))?;
        db.pragma_query_value(None, "user_version", |row| row.get(0))
    };
    read().map(Some).map_err(|e| e.to_string())
}

/// The state's stable id, read without creating or upgrading anything; `None` until a
/// command that writes the state has opened it.
pub fn read_state_id(state: &Path) -> Result<Option<String>, String> {
    let state = canonical_target(state).map_err(|e| e.to_string())?;
    regular_files(&state)?;
    let database = state.join(DATABASE);
    if !database.try_exists().map_err(|e| e.to_string())? {
        return Ok(None);
    }
    let read = || -> Result<Option<String>, rusqlite::Error> {
        let db = rusqlite::Connection::open_with_flags(
            &database,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        db.busy_timeout(Duration::from_secs(5))?;
        let exists: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='state_meta')",
            [],
            |row| row.get(0),
        )?;
        if !exists {
            return Ok(None);
        }
        db.query_row("SELECT value FROM state_meta WHERE name='id'", [], |row| {
            row.get(0)
        })
        .optional()
    };
    read().map_err(|e| e.to_string())
}

fn regular_files(state: &Path) -> Result<(), String> {
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
    pub run_id: String,
    pub verdict: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
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
            db.busy_timeout(Duration::from_secs(5))?;
            let transaction = db.transaction()?;
            if !schema_initialized(&transaction)? {
                return Ok(Default::default());
            }
            let latest = {
                let mut statement = transaction.prepare(
                    "SELECT eval_id,run_id,status,fingerprint FROM (
                SELECT m.eval_id,q.run_id,q.status,json_extract(q.data, '$.fingerprint') AS fingerprint,
                    row_number() OVER (PARTITION BY m.eval_id ORDER BY r.rowid DESC) AS rank
                FROM run_members m JOIN requests q ON q.id=m.request_id JOIN runs r ON r.id=m.run_id
                WHERE r.repo=?
            ) WHERE rank=1 ORDER BY eval_id",
                )?;
                statement
                    .query_map([repo.to_string_lossy()], |row| {
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
    let id = id.to_owned();
    connection.call(move |db| -> Result<RunView, Error> {
        db.busy_timeout(Duration::from_secs(5))?;
        let transaction = db.transaction()?;
        if !schema_initialized(&transaction)? {
            return Err(Error::Invalid("Run not found.".into()));
        }
        let saved: Option<(String, String)> = transaction.query_row("SELECT repo,data FROM runs WHERE id=?", [&id], |row| Ok((row.get(0)?, row.get(1)?))).optional()?;
        let (repo, data) = saved.ok_or_else(|| Error::Invalid("Run not found.".into()))?;
        outside_workspace(Path::new(&repo), &state).map_err(|e| Error::Invalid(e.to_string()))?;
        let run = serde_json::from_str(&data)?;
        let mut requests = {
            let mut statement = transaction.prepare("SELECT q.data FROM requests q JOIN run_members m ON q.id=m.request_id WHERE m.run_id=? ORDER BY m.ordinal")?;
            statement.query_map([&id], |row| row.get::<_, String>(0))?.map(|row| Ok(serde_json::from_str(&row?)?)).collect::<Result<Vec<Request>, Error>>()?
        };
        for request in &mut requests {
            request.tool_calls = super::tool_calls::project(&transaction, request.execution_id.as_deref(), &request.tool_calls)?;
        }
        transaction.commit()?;
        Ok(RunView { run, requests })
    }).await.map_err(|e| e.to_string())
}
