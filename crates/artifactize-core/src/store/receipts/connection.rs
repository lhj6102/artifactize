//! Opening, versioning and joining state database connections.
use super::Error;
use crate::{
    store::{self as store, STATE_SCHEMA_VERSION},
    workspace::{canonical_target, prepare_directory},
};
use rusqlite::OptionalExtension;
use std::{path::Path, time::Duration};
use tokio_rusqlite::Connection;

pub const DATABASE: &str = "state.sqlite";
/// Yield between WAL initialization races instead of spinning, still within the
/// shared SQLite contention deadline used by the outer initialization loop.
const INITIALIZATION_RETRY_INTERVAL: Duration = Duration::from_millis(20);

/// The five tables of a state and the indexes its lookups use.
///
/// - `runs`: one row per Run, its changing state without its definitions.
/// - `run_definitions`: one row per Run with definitions: the Artifacts, evals and relations it
///   was started with. They are fixed for the Run's life, written once with the Run and never
///   updated, so a change of the Run never rewrites them.
/// - `requests`: one row per eval of a Run, at its `ordinal` in the Run's selection order;
///   `claimed_by` and `claimed_at` hold the Human claim of a waiting request.
/// - `executions`: one row per execution. `backend` names the Agent backend it runs on, and
///   a RUNNING execution with a backend holds one of that backend's machine-wide slots. A
///   completed GREEN/RED execution with a key is a record of the key's history while
///   `completed_at` (sortable), `bytes` and `last_used` are set; the cache GC and `cache rm`
///   clear them, and the execution stays.
/// - `state_meta`: named values of the state itself: `id`, a random UUID made when the
///   database is created, which identifies the state in Agent session references.
///
/// `runs`, `requests` and `executions` carry a `revision` that [`REVISIONS`] raises on every
/// update of the row, so a reader learns which rows changed by comparing numbers.
const SCHEMA: &str =
    "CREATE TABLE runs(id TEXT PRIMARY KEY, repo TEXT NOT NULL, status TEXT NOT NULL, data TEXT NOT NULL, request_key TEXT, revision INTEGER NOT NULL DEFAULT 0);
    CREATE UNIQUE INDEX run_request_key ON runs(repo,request_key) WHERE request_key IS NOT NULL;
    CREATE TABLE run_definitions(run_id TEXT PRIMARY KEY REFERENCES runs(id), data TEXT NOT NULL);
    CREATE TABLE requests(id TEXT PRIMARY KEY, run_id TEXT NOT NULL REFERENCES runs(id), eval_id TEXT NOT NULL, ordinal INTEGER NOT NULL, execution_id TEXT REFERENCES executions(id), status TEXT NOT NULL, claimed_by TEXT, claimed_at TEXT, data TEXT NOT NULL, revision INTEGER NOT NULL DEFAULT 0, UNIQUE(run_id, eval_id), UNIQUE(run_id, ordinal));
    CREATE TABLE executions(id TEXT PRIMARY KEY, key TEXT, eval_def_hash TEXT NOT NULL, status TEXT NOT NULL, owner_pid INTEGER NOT NULL, owner_start_time INTEGER NOT NULL, backend TEXT, profile_identity TEXT, completed_at TEXT, bytes INTEGER, last_used TEXT, data TEXT NOT NULL, revision INTEGER NOT NULL DEFAULT 0);
    CREATE TABLE state_meta(name TEXT PRIMARY KEY, value TEXT NOT NULL);
    CREATE INDEX active_request_execution ON requests(execution_id) WHERE status IN ('QUEUED','RUNNING','WAITING_HUMAN');
    CREATE UNIQUE INDEX active_key ON executions(key,profile_identity) WHERE key IS NOT NULL AND status IN ('RUNNING','WAITING_HUMAN');
    CREATE INDEX key_history ON executions(key, completed_at);
    CREATE INDEX definition_history ON executions(eval_def_hash, completed_at);
    CREATE INDEX running_backend ON executions(backend) WHERE status='RUNNING';";

/// Raise a row's `revision` on every update that does not set it. Writers need not know about
/// it: the trigger's own update leaves `revision` changed, so it never fires again. The indexes
/// let readers count, list and compare revisions without reading a record's data:
/// `request_status` counts a Run's requests by status, `request_marks` and `execution_marks`
/// hold the revisions of a Run's requests and their executions, `request_claim` the Human
/// claims, and `request_execution` names the Runs an execution's change touches.
pub(in crate::store) const REVISIONS: &str =
    "CREATE TRIGGER run_revision AFTER UPDATE ON runs FOR EACH ROW WHEN NEW.revision=OLD.revision BEGIN UPDATE runs SET revision=OLD.revision+1 WHERE rowid=NEW.rowid; END;
    CREATE TRIGGER request_revision AFTER UPDATE ON requests FOR EACH ROW WHEN NEW.revision=OLD.revision BEGIN UPDATE requests SET revision=OLD.revision+1 WHERE rowid=NEW.rowid; END;
    CREATE TRIGGER execution_revision AFTER UPDATE ON executions FOR EACH ROW WHEN NEW.revision=OLD.revision BEGIN UPDATE executions SET revision=OLD.revision+1 WHERE rowid=NEW.rowid; END;
    CREATE INDEX request_status ON requests(status,run_id);
    CREATE INDEX request_marks ON requests(run_id,ordinal,id,execution_id,revision);
    CREATE INDEX request_execution ON requests(execution_id,run_id);
    CREATE INDEX request_claim ON requests(execution_id,id,revision) WHERE claimed_by IS NOT NULL;
    CREATE INDEX execution_marks ON executions(id,revision);";

/// Versions before the supported additive upgrade boundary are refused.
pub const EARLIER_STATE: &str = "This state was written by an earlier artifactize. Start a new state (set ARTIFACTIZE_STATE_HOME or move the old one away). Only schema 6 and later supported versions can be upgraded.";

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

#[derive(Clone)]
pub struct Receipts {
    pub(in crate::store) connection: NotifyingConnection,
}

/// A test writer closes its SQLite worker before the caller's temporary state is removed.
#[cfg(any(test, feature = "test-support"))]
pub struct OwnedReceipts(Receipts);

#[cfg(any(test, feature = "test-support"))]
impl OwnedReceipts {
    pub async fn open(state: &Path, repo: &Path) -> Result<Self, String> {
        Receipts::open(state, repo).await.map(Self)
    }
}

#[cfg(any(test, feature = "test-support"))]
impl std::ops::Deref for OwnedReceipts {
    type Target = Receipts;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Drop for OwnedReceipts {
    fn drop(&mut self) {
        let connection = self.0.connection.inner.clone();
        // Drop can run on a current-thread Tokio test runtime, including during unwinding.
        // A separate joined thread avoids nesting runtimes or detaching SQLite teardown.
        let closed = std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .build()
                .expect("create the test writer's teardown runtime")
                .block_on(connection.close())
        })
        .join();
        if !std::thread::panicking() {
            closed
                .expect("join the test writer's teardown")
                .expect("close the test writer's SQLite connection");
        }
    }
}

#[cfg(test)]
mod ownership_tests {
    use super::*;

    #[tokio::test]
    async fn failed_writer_initialization_releases_sqlite_before_returning_the_schema_error() {
        let root = crate::test_os::tempdir();
        let state = root.path().join("state");
        let repo = root.path().join("repo");
        std::fs::create_dir(&state).unwrap();
        std::fs::create_dir(&repo).unwrap();
        let database = state.join(DATABASE);
        let connection = rusqlite::Connection::open(&database).unwrap();
        connection
            .execute_batch("CREATE TABLE unrelated(value TEXT)")
            .unwrap();
        drop(connection);
        let error = match OwnedReceipts::open(&state, &repo).await {
            Ok(_) => panic!("an unversioned nonempty database is not our schema"),
            Err(error) => error,
        };
        assert!(
            error.contains("Unsupported state schema version: 0."),
            "{error}"
        );
        assert!(crate::test_os::database_released(&database).unwrap());
        root.close().unwrap();
    }

    #[tokio::test]
    async fn explicit_writer_close_releases_sqlite_before_return_and_closes_clones() {
        let root = crate::test_os::tempdir();
        let state = root.path().join("state");
        let repo = root.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        let writer = OwnedReceipts::open(&state, &repo).await.unwrap();
        let retained = Receipts::clone(&writer);
        let database = state.join(DATABASE);
        assert!(!crate::test_os::database_released(&database).unwrap());
        Receipts::clone(&writer).close().await.unwrap();
        assert!(crate::test_os::database_released(&database).unwrap());
        assert_eq!(retained.state_id().await.unwrap_err(), "ConnectionClosed");
        root.close().unwrap();
    }

    #[tokio::test]
    async fn owned_writer_closes_before_temp_state_removal_on_success_and_unwind() {
        for unwind in [false, true] {
            let root = crate::test_os::tempdir();
            let state = root.path().join("state");
            let repo = root.path().join("repo");
            std::fs::create_dir(&repo).unwrap();
            let writer = OwnedReceipts::open(&state, &repo).await.unwrap();
            assert!(writer.state_id().await.is_ok());
            // Even another writer handle cannot retain the worker after fixture teardown.
            let retained = Receipts::clone(&writer);
            if unwind {
                let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                    let _writer = writer;
                    panic!("fixture assertion failure");
                }));
                assert!(failed.is_err());
            } else {
                drop(writer);
            }
            let error = retained.state_id().await.unwrap_err();
            assert_eq!(error, "ConnectionClosed");
            root.close().unwrap();
        }
    }
}

/// Notify at the DB-worker completion boundary, even if the awaiting caller was cancelled.
/// total_changes is conservative (rolled-back changes can invalidate harmlessly). Only a
/// finished autocommit edge can publish; no event is an audit record or proof of success.
#[derive(Clone)]
pub(crate) struct NotifyingConnection {
    inner: Connection,
    publisher: crate::changes::Publisher,
    /// The connection notes which Runs its writes touch ([`store::touched`]).
    noting: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl NotifyingConnection {
    pub(in crate::store) async fn finish<T>(self, result: Result<T, String>) -> Result<T, String> {
        store::connection::finish(self.inner, result).await
    }
    pub(crate) fn new(inner: Connection, state: &Path) -> Self {
        Self {
            inner,
            publisher: crate::changes::Publisher::new(state),
            noting: Default::default(),
        }
    }
    pub(crate) async fn call<F, R, E>(&self, function: F) -> Result<R, tokio_rusqlite::Error<E>>
    where
        F: FnOnce(&mut rusqlite::Connection) -> Result<R, E> + Send + 'static,
        R: Send + 'static,
        E: Send + 'static,
    {
        let publisher = self.publisher.clone();
        let noting = self.noting.clone();
        self.inner
            .call(move |db| {
                if !noting.load(std::sync::atomic::Ordering::Relaxed)
                    && db.is_autocommit()
                    && store::touched::install(db).unwrap_or(false)
                {
                    noting.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                let before = db.total_changes();
                let result = function(db);
                if db.is_autocommit() && db.total_changes() != before {
                    for change in store::touched::take(db) {
                        publisher.notify(change);
                    }
                }
                result
            })
            .await
    }
}

impl Receipts {
    /// Close the shared SQLite connection before releasing its state files. Any cloned
    /// receipts also become closed; call this only after all operations on them finish.
    pub async fn close(self) -> Result<(), String> {
        self.connection.finish(Ok(())).await
    }

    pub async fn open(state: &Path, repo: &Path) -> Result<Self, String> {
        let state = prepare_directory(state, repo).map_err(|e| e.to_string())?;
        check_files(&state)?;
        let state_id: crate::types::StateId = crate::agent::uuid()?.parse()?;
        let connection = Connection::open(state.join(DATABASE))
            .await
            .map_err(|e| e.to_string())?;
        let connection = NotifyingConnection::new(connection, &state);
        // WAL initialization retries share the database contention budget.
        let deadline = tokio::time::Instant::now() + store::SQLITE_BUSY_TIMEOUT;
        loop {
            let state_id = state_id.clone();
            let initialized = connection
                .call(move |db| -> Result<(), Error> {
                    store::wait_when_busy(db)?;
                    db.pragma_update(None, "foreign_keys", true)?;
                    let version: u32 =
                        db.pragma_query_value(None, "user_version", |row| row.get(0))?;
                    if version != 0
                        && !(store::migrations::FIRST_VERSION..=STATE_SCHEMA_VERSION)
                            .contains(&version)
                    {
                        return Err(Error::Invalid(schema_error(version)));
                    }
                    db.pragma_update(None, "journal_mode", "WAL")?;
                    let transaction =
                        db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                    // Publish the schema, the state's id and the version together; readers see
                    // an empty snapshot until commit.
                    store::migrations::apply(&transaction)?;
                    if !schema_initialized(&transaction)? {
                        transaction.execute_batch(SCHEMA)?;
                        transaction.execute_batch(store::migrations::WORKSPACES)?;
                        transaction.execute_batch(REVISIONS)?;
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
                    // Concurrent first opens can race when enabling WAL despite the busy handler.
                    tokio::time::sleep(INITIALIZATION_RETRY_INTERVAL).await;
                }
                Err(error) => return connection.finish(Err(error.to_string())).await,
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
}

pub(in crate::store) fn schema_initialized(
    transaction: &rusqlite::Transaction<'_>,
) -> Result<bool, Error> {
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
pub(in crate::store) fn check_files(state: &Path) -> Result<(), String> {
    match state_schema(state)? {
        Some(version)
            if (store::migrations::FIRST_VERSION..STATE_SCHEMA_VERSION).contains(&version) =>
        {
            store::migrations::open_existing(state)
        }
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
        store::wait_when_busy(&db)?;
        db.pragma_query_value(None, "user_version", |row| row.get(0))
    };
    let version = read().map_err(|e| e.to_string())?;
    if (store::migrations::FIRST_VERSION..STATE_SCHEMA_VERSION).contains(&version) {
        store::migrations::open_existing(&state)?;
        return read().map(Some).map_err(|e| e.to_string());
    }
    Ok(Some(version))
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
        store::wait_when_busy(&db)?;
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
    regular_files_with_retry(state, || {
        std::thread::sleep(store::TRANSIENT_ACCESS_BACKOFF);
    })
}

/// The probe and retry path is shared with the seam test; only the between-attempt effect
/// differs, so a fixture can release a pending-delete handle exactly after a failed probe.
pub(in crate::store) fn regular_files_with_retry(
    state: &Path,
    mut before_retry: impl FnMut(),
) -> Result<(), String> {
    for suffix in ["", "-wal", "-shm"] {
        let path = state.join(format!("{DATABASE}{suffix}"));
        match store::retry_transient_access(|| crate::platform::path_kind(&path), &mut before_retry)
        {
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
