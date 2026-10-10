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
#[serde(try_from = "RunWire", into = "RunWire")]
pub struct Run {
    pub id: RunId,
    pub repo_path: PathBuf,
    /// Optional display metadata; schema 5 Runs written before this remain readable.
    pub repository: crate::repository::Identity,
    pub state_dir: PathBuf,
    pub state: super::RunState,
    pub created_at: Timestamp,
    pub selection: crate::project::selection::Selection,
    pub profile: Option<crate::project::selection::ProfileSelection>,
    pub definitions: super::definitions::Definitions,
    pub recursive: bool,
    pub force: bool,
    pub ignore_gates: bool,
    pub jobs: usize,
    /// Fingerprints computed at once; absent in Runs saved before 0.5.
    pub fingerprint_jobs: Option<usize>,
    pub max_executions: Option<u64>,
    pub executions_started: u64,
    pub wait_timeout_ms: Option<Duration>,
    pub wait_timed_out: bool,
    /// Kinds whose evals only reuse a result; one with nothing to reuse is not executed.
    pub reuse_only: BTreeSet<ProfileKind>,
    pub validation: super::Validation,
    /// Agent backends this Run stopped admitting reviews on, in the order they stopped.
    pub stopped_backends: Vec<StoppedBackend>,
    /// Verdicts the Run took from saved results for evals it has no request for, such as the
    /// dependencies of a partial Run, so readers judge gates on the Run's own evidence.
    pub evidence: BTreeMap<crate::types::EvalId, crate::types::RequestStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunWire {
    pub id: RunId,
    #[serde(with = "crate::platform::path_serde")]
    pub repo_path: PathBuf,
    /// Optional display metadata; schema 5 Runs written before this remain readable.
    #[serde(flatten)]
    pub repository: crate::repository::Identity,
    #[serde(with = "crate::platform::path_serde")]
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
    pub validation: super::Validation,
    pub error: Option<String>,
    /// Agent backends this Run stopped admitting reviews on, in the order they stopped.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stopped_backends: Vec<StoppedBackend>,
    /// Verdicts the Run took from saved results for evals it has no request for, such as the
    /// dependencies of a partial Run, so readers judge gates on the Run's own evidence.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub evidence: BTreeMap<crate::types::EvalId, crate::types::RequestStatus>,
}

impl TryFrom<RunWire> for Run {
    type Error = String;
    fn try_from(wire: RunWire) -> Result<Self, Self::Error> {
        let state = super::RunState::from_parts(wire.status, wire.completed_at, wire.error)?;
        Ok(Self {
            state,
            id: wire.id,
            repo_path: wire.repo_path,
            repository: wire.repository,
            state_dir: wire.state_dir,
            created_at: wire.created_at,
            selection: wire.selection,
            profile: wire.profile,
            definitions: wire.definitions,
            recursive: wire.recursive,
            force: wire.force,
            ignore_gates: wire.ignore_gates,
            jobs: wire.jobs,
            fingerprint_jobs: wire.fingerprint_jobs,
            max_executions: wire.max_executions,
            executions_started: wire.executions_started,
            wait_timeout_ms: wire.wait_timeout_ms,
            wait_timed_out: wire.wait_timed_out,
            reuse_only: wire.reuse_only,
            validation: wire.validation,
            stopped_backends: wire.stopped_backends,
            evidence: wire.evidence,
        })
    }
}
impl From<Run> for RunWire {
    fn from(record: Run) -> Self {
        Self {
            status: record.status(),
            completed_at: record.completed_at(),
            error: record.error().map(str::to_owned),
            id: record.id,
            repo_path: record.repo_path,
            repository: record.repository,
            state_dir: record.state_dir,
            created_at: record.created_at,
            selection: record.selection,
            profile: record.profile,
            definitions: record.definitions,
            recursive: record.recursive,
            force: record.force,
            ignore_gates: record.ignore_gates,
            jobs: record.jobs,
            fingerprint_jobs: record.fingerprint_jobs,
            max_executions: record.max_executions,
            executions_started: record.executions_started,
            wait_timeout_ms: record.wait_timeout_ms,
            wait_timed_out: record.wait_timed_out,
            reuse_only: record.reuse_only,
            validation: record.validation,
            stopped_backends: record.stopped_backends,
            evidence: record.evidence,
        }
    }
}
impl Run {
    pub fn status(&self) -> RunStatus {
        self.state.status()
    }
    pub fn completed_at(&self) -> Option<Timestamp> {
        self.state.completed_at()
    }
    pub fn error(&self) -> Option<&str> {
        self.state.error()
    }
}

/// An Agent backend a Run stopped after an AUTHENTICATION or QUOTA failure, which every
/// later review on it would repeat.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoppedBackend {
    pub backend: crate::config::Backend,
    pub error_code: crate::types::BackendStopCode,
    pub eval_id: crate::types::EvalId,
    pub request_id: RequestId,
    pub error: String,
}

fn default_jobs() -> usize {
    crate::project::DEFAULT_JOBS
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "RequestWire", into = "RequestWire")]
pub struct Request {
    pub state: super::RequestState,
    pub id: RequestId,
    pub run_id: RunId,
    pub eval_id: crate::types::EvalId,
    pub target: crate::types::ArtifactName,
    pub title: String,
    pub profile: crate::config::StoredProfile,
    pub requested_profile: crate::config::StoredProfile,
    pub eval_def_hash: crate::types::DefinitionHash,
    /// The requested execution options, or a reused result's.
    pub options: ExecutionOptions,
    pub execution_id: Option<crate::types::ExecutionId>,
    pub provenance: Option<Provenance>,
    /// Usage spent by this request; a reused request spent none.
    pub usage: Option<Vec<crate::llm::Attempt>>,
    /// The reused execution's original usage, never counted as spent.
    pub reused_usage: Option<Vec<crate::llm::Attempt>>,
    /// The reused result came from the live execution this request waited for, not a
    /// completed record. Saved only; output reports it as `source.kind` ([`crate::query::source`]).
    pub joined: bool,
    /// Who produced a reused result.
    pub producer: Option<Producer>,
    /// The Human claimant who submitted a reused result.
    pub reviewer: Option<crate::types::ReviewerId>,
    /// The remote store, publisher and publication time of a result reused from a mirror.
    pub origin: Option<Origin>,
    /// The Agent review's session id ([`crate::agent::session_id`]), which every request of
    /// the review sends as its prompt-cache identity. Absent when no Agent review ran, as on
    /// reuse, and on requests saved before it existed.
    pub session_id: Option<SessionId>,
    /// Where the saved conversation behind this request's result lives: its own review's,
    /// or for a reused result the producing review's. Absent when none was saved.
    pub session: Option<crate::agent::session::SessionRef>,
    pub human_definition: Option<super::HumanDefinition>,
    pub payload: crate::config::StoredPayload,
    pub references: BTreeMap<String, crate::types::ArtifactName>,
    pub deps: Vec<crate::types::ArtifactName>,
    pub force: bool,
    /// The target Artifact's fingerprint.
    pub fingerprint: Option<Fingerprint>,
    /// The reuse key; absent without a fingerprint on every Artifact the eval depends on.
    pub key: Option<ReuseKey>,
    /// Each Artifact the key covers, with its fingerprint.
    pub fingerprints: std::collections::BTreeMap<crate::types::ArtifactName, Fingerprint>,
    pub created_at: Timestamp,
    pub started_at: Option<Timestamp>,
    pub cwd: PathBuf,
    pub run_dir: Option<PathBuf>,
    pub argv: Option<Vec<String>>,
    pub child: Option<super::ChildIdentity>,
    pub blocked_reason: Option<String>,
    /// Unfulfilled Artifacts and Evals behind a derived dependency verdict.
    pub blocked_by: Vec<super::Blocker>,
    /// What a queued request waits for besides a job; `blocked_reason` says it in words.
    pub queue: Option<QueueReason>,
}

/// What a queued request waits for besides a job slot of its Run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum QueueReason {
    /// A free machine-wide slot of this backend (`limits.json`).
    Slot { backend: crate::config::Backend },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RequestWire {
    pub id: RequestId,
    pub run_id: RunId,
    pub eval_id: crate::types::EvalId,
    pub target: crate::types::ArtifactName,
    pub title: String,
    pub profile: crate::config::StoredProfile,
    pub requested_profile: crate::config::StoredProfile,
    pub eval_def_hash: crate::types::DefinitionHash,
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
    pub reviewer: Option<crate::types::ReviewerId>,
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
    pub human_definition: Option<super::HumanDefinition>,
    pub payload: crate::config::StoredPayload,
    pub references: BTreeMap<String, crate::types::ArtifactName>,
    pub deps: Vec<crate::types::ArtifactName>,
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
    pub fingerprints: std::collections::BTreeMap<crate::types::ArtifactName, Fingerprint>,
    pub status: RequestStatus,
    pub created_at: Timestamp,
    pub started_at: Option<Timestamp>,
    pub completed_at: Option<Timestamp>,
    #[serde(with = "crate::platform::path_serde")]
    pub cwd: PathBuf,
    #[serde(default, with = "crate::platform::path_serde::option")]
    pub run_dir: Option<PathBuf>,
    pub argv: Option<Vec<String>>,
    pub child: Option<super::ChildIdentity>,
    pub result: Option<super::ExecutionResult>,
    pub error: Option<String>,
    pub error_code: Option<crate::types::FailureCode>,
    pub blocked_reason: Option<String>,
    /// Unfulfilled Artifacts and Evals behind a derived dependency verdict.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_by: Vec<super::Blocker>,
    /// Absent from requests saved before it existed, which read as waiting for a job.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue: Option<QueueReason>,
}

impl TryFrom<RequestWire> for Request {
    type Error = String;
    fn try_from(wire: RequestWire) -> Result<Self, Self::Error> {
        let state = super::RequestState::from_parts(
            wire.status,
            wire.result,
            wire.error,
            wire.error_code,
            wire.completed_at,
        )?;
        Ok(Self {
            state,
            id: wire.id,
            run_id: wire.run_id,
            eval_id: wire.eval_id,
            target: wire.target,
            title: wire.title,
            profile: wire.profile,
            requested_profile: wire.requested_profile,
            eval_def_hash: wire.eval_def_hash,
            options: wire.options,
            execution_id: wire.execution_id,
            provenance: wire.provenance,
            usage: wire.usage,
            reused_usage: wire.reused_usage,
            joined: wire.joined,
            producer: wire.producer,
            reviewer: wire.reviewer,
            origin: wire.origin,
            session_id: wire.session_id,
            session: wire.session,
            human_definition: wire.human_definition,
            payload: wire.payload,
            references: wire.references,
            deps: wire.deps,
            force: wire.force,
            fingerprint: wire.fingerprint,
            key: wire.key,
            fingerprints: wire.fingerprints,
            created_at: wire.created_at,
            started_at: wire.started_at,
            cwd: wire.cwd,
            run_dir: wire.run_dir,
            argv: wire.argv,
            child: wire.child,
            blocked_reason: wire.blocked_reason,
            blocked_by: wire.blocked_by,
            queue: wire.queue,
        })
    }
}
impl From<Request> for RequestWire {
    fn from(record: Request) -> Self {
        Self {
            status: record.status(),
            result: record.result().cloned(),
            error: record.error().map(str::to_owned),
            error_code: record.error_code(),
            completed_at: record.completed_at(),
            id: record.id,
            run_id: record.run_id,
            eval_id: record.eval_id,
            target: record.target,
            title: record.title,
            profile: record.profile,
            requested_profile: record.requested_profile,
            eval_def_hash: record.eval_def_hash,
            options: record.options,
            execution_id: record.execution_id,
            provenance: record.provenance,
            usage: record.usage,
            reused_usage: record.reused_usage,
            joined: record.joined,
            producer: record.producer,
            reviewer: record.reviewer,
            origin: record.origin,
            session_id: record.session_id,
            session: record.session,
            human_definition: record.human_definition,
            payload: record.payload,
            references: record.references,
            deps: record.deps,
            force: record.force,
            fingerprint: record.fingerprint,
            key: record.key,
            fingerprints: record.fingerprints,
            created_at: record.created_at,
            started_at: record.started_at,
            cwd: record.cwd,
            run_dir: record.run_dir,
            argv: record.argv,
            child: record.child,
            blocked_reason: record.blocked_reason,
            blocked_by: record.blocked_by,
            queue: record.queue,
        }
    }
}
impl Request {
    pub fn status(&self) -> crate::types::RequestStatus {
        self.state.status()
    }
    pub fn result(&self) -> Option<&super::ExecutionResult> {
        self.state.result()
    }
    pub fn error(&self) -> Option<&str> {
        self.state.error()
    }
    pub fn error_code(&self) -> Option<crate::types::FailureCode> {
        self.state.error_code()
    }
    pub fn completed_at(&self) -> Option<crate::types::Timestamp> {
        self.state.completed_at()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RunView {
    #[serde(flatten)]
    pub run: Run,
    pub requests: Vec<Request>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unreadable: Vec<super::Unreadable>,
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
        let state_id: crate::types::StateId = crate::agent::uuid()?.parse()?;
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
    pub async fn state_id(&self) -> Result<crate::types::StateId, String> {
        self.connection
            .call(|db| -> Result<crate::types::StateId, Error> {
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
        for request in requests {
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
                        run.status(),
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
                            request.status(),
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
        let run = run.clone();
        self.connection
            .call(move |db| -> Result<(), Error> {
                db.execute(
                    "UPDATE runs SET status=?,data=? WHERE id=?",
                    params![run.status(), serde_json::to_string(&run)?, run.id],
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
                    if request.status() == crate::types::RequestStatus::WaitingHuman {
                        continue;
                    }
                    update_request(&transaction, &request)?;
                }
                transaction.execute(
                    "UPDATE runs SET status=?,data=? WHERE id=?",
                    params![run.status(), serde_json::to_string(&run)?, run.id],
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
            request.status(),
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
pub fn read_state_id(state: &Path) -> Result<Option<crate::types::StateId>, String> {
    let state = canonical_target(state).map_err(|e| e.to_string())?;
    check_files(&state)?;
    let database = state.join(DATABASE);
    if !database.try_exists().map_err(|e| e.to_string())? {
        return Ok(None);
    }
    let read = || -> Result<Option<crate::types::StateId>, Error> {
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
        match crate::platform::path_kind(&path) {
            Ok(kind) if kind != crate::platform::FileKind::File => {
                return Err(format!(
                    "State files must be regular files: {}",
                    crate::platform::path_text(&path)
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
) -> Result<std::collections::BTreeMap<crate::types::EvalId, LastRequest>, String> {
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
                    "SELECT data,run_data FROM (
                SELECT q.eval_id,q.data,r.data AS run_data,
                    row_number() OVER (PARTITION BY q.eval_id ORDER BY r.rowid DESC) AS rank
                FROM requests q JOIN runs r ON r.id=q.run_id
                WHERE r.repo=?
            ) WHERE rank=1 ORDER BY eval_id",
                )?;
                statement
                    .query_map([crate::platform::path_text(&repo)], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })?
                    .filter_map(|row| match row {
                        Err(error) => Some(Err(error)),
                        Ok((data, run_data)) => {
                            super::unreadable::evidence::<Run>("run", &run_data)
                                .and_then(|_| {
                                    super::unreadable::evidence::<Request>("request", &data)
                                })
                                .map(|request| {
                                    Ok((
                                        request.eval_id.clone(),
                                        LastRequest {
                                            run_id: request.run_id.clone(),
                                            verdict: request.status(),
                                            fingerprint: request.fingerprint,
                                        },
                                    ))
                                })
                        }
                    })
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
            let run = super::unreadable::decode("run", &id, &data)
                .map_err(|error| Error::Invalid(error.to_string()))?;
            let requests = {
                let mut statement = transaction
                    .prepare("SELECT data FROM requests WHERE run_id=? ORDER BY ordinal")?;
                statement
                    .query_map([&id], |row| row.get::<_, String>(0))?
                    .filter_map(|row| match row {
                        Ok(data) => super::unreadable::evidence("request", &data).map(Ok),
                        Err(error) => Some(Err(Error::Sql(error))),
                    })
                    .collect::<Result<Vec<Request>, Error>>()?
            };
            let unreadable = super::unreadable::in_run(&transaction, &id)?;
            transaction.commit()?;
            Ok(RunView {
                run,
                requests,
                unreadable,
            })
        })
        .await
        .map_err(|e| e.to_string())
}
