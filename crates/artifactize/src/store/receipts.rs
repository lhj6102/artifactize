use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_rusqlite::Connection;

use super::{STATE_SCHEMA_VERSION, canonical_target, outside_workspace};

pub const DATABASE: &str = "receipts.sqlite";

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
    pub recursive: bool,
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub ignore_gates: bool,
    pub validation: Value,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub id: String,
    pub run_id: String,
    pub critic_id: String,
    pub target: String,
    pub title: String,
    pub profile: Value,
    pub payload: Value,
    pub references: Value,
    pub deps: Vec<String>,
    #[serde(default)]
    pub force: bool,
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
    connection: Connection,
}

impl Receipts {
    pub async fn open(state: &Path, repo: &Path) -> Result<Self, String> {
        let state = super::prepare_directory(state, repo).map_err(|e| e.to_string())?;
        check_files(&state)?;
        let connection = Connection::open(state.join(DATABASE))
            .await
            .map_err(|e| e.to_string())?;
        let repo = repo.to_string_lossy().into_owned();
        connection.call(move |db| -> Result<(), Error> {
            db.busy_timeout(Duration::from_secs(5))?;
            db.pragma_update(None, "foreign_keys", true)?;
            let version: u32 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
            if version != 0 && version != STATE_SCHEMA_VERSION {
                return Err(Error::Invalid(format!("Unsupported receipts schema version: {version}")));
            }
            if version == STATE_SCHEMA_VERSION {
                check_repo(db, Some(&repo))?;
            }
            db.pragma_update(None, "journal_mode", "WAL")?;
            let transaction = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            transaction.execute_batch("CREATE TABLE IF NOT EXISTS metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL);
                CREATE TABLE IF NOT EXISTS runs(id TEXT PRIMARY KEY, status TEXT NOT NULL, data TEXT NOT NULL);
                CREATE TABLE IF NOT EXISTS requests(id TEXT PRIMARY KEY, run_id TEXT NOT NULL REFERENCES runs(id), status TEXT NOT NULL, data TEXT NOT NULL);
                CREATE TABLE IF NOT EXISTS run_members(run_id TEXT NOT NULL REFERENCES runs(id), critic_id TEXT NOT NULL, ordinal INTEGER NOT NULL, request_id TEXT NOT NULL REFERENCES requests(id), PRIMARY KEY(run_id, critic_id), UNIQUE(run_id, ordinal));")?;
            transaction.execute("INSERT OR IGNORE INTO metadata(key,value) VALUES ('repo_path',?)", [&repo])?;
            check_repo(&transaction, Some(&repo))?;
            transaction.pragma_update(None, "user_version", STATE_SCHEMA_VERSION)?;
            transaction.commit()?;
            Ok(())
        }).await.map_err(|e| e.to_string())?;
        Ok(Self { connection })
    }

    pub async fn create_run(&self, run: &Run, requests: &[Request]) -> Result<(), String> {
        let run = run.clone();
        let requests = requests.to_vec();
        self.connection.call(move |db| -> Result<(), Error> {
            let transaction = db.transaction()?;
            transaction.execute("INSERT INTO runs(id,status,data) VALUES (?,?,?)", params![run.id, run.status, serde_json::to_string(&run)?])?;
            for (ordinal, request) in requests.iter().enumerate() {
                transaction.execute("INSERT INTO requests(id,run_id,status,data) VALUES (?,?,?,?)", params![request.id, run.id, request.status, serde_json::to_string(request)?])?;
                transaction.execute("INSERT INTO run_members(run_id,critic_id,ordinal,request_id) VALUES (?,?,?,?)", params![run.id, request.critic_id, ordinal as i64, request.id])?;
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

    pub async fn finish(&self, run: &Run, requests: &[Request]) -> Result<(), String> {
        let run = run.clone();
        let requests = requests.to_vec();
        self.connection
            .call(move |db| -> Result<(), Error> {
                let transaction = db.transaction()?;
                for request in requests {
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

fn update_request(db: &rusqlite::Connection, request: &Request) -> Result<(), Error> {
    if db.execute(
        "UPDATE requests SET status=?,data=? WHERE id=?",
        params![request.status, serde_json::to_string(request)?, request.id],
    )? != 1
    {
        return Err(Error::Invalid("Review request not found.".into()));
    }
    Ok(())
}

fn check_repo(db: &rusqlite::Connection, expected: Option<&str>) -> Result<String, Error> {
    let repo: String = db.query_row(
        "SELECT value FROM metadata WHERE key='repo_path'",
        [],
        |row| row.get(0),
    )?;
    if expected.is_some_and(|expected| repo != expected) {
        return Err(Error::Invalid(
            "State directory belongs to a different repository.".into(),
        ));
    }
    Ok(repo)
}

fn check_files(state: &Path) -> Result<(), String> {
    for suffix in ["", "-wal", "-shm"] {
        let path = state.join(format!("{DATABASE}{suffix}"));
        match path.symlink_metadata() {
            Ok(metadata) if !metadata.is_file() => {
                return Err(format!(
                    "Receipt files must be regular files: {}",
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

/// Existing store only: no discovery, schema creation, or worker reconciliation.
pub async fn read_run(state: &Path, repo: Option<&Path>, id: &str) -> Result<RunView, String> {
    let state = canonical_target(state).map_err(|e| e.to_string())?;
    check_files(&state)?;
    let connection = Connection::open_with_flags(
        state.join(DATABASE),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .await
    .map_err(|e| e.to_string())?;
    let repo = repo.map(|p| p.to_string_lossy().into_owned());
    let id = id.to_owned();
    connection.call(move |db| -> Result<RunView, Error> {
        db.busy_timeout(Duration::from_secs(5))?;
        let transaction = db.transaction()?;
        let version: u32 = transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version != STATE_SCHEMA_VERSION {
            return Err(Error::Invalid(format!("Unsupported receipts schema version: {version}")));
        }
        let stored_repo = check_repo(&transaction, repo.as_deref())?;
        outside_workspace(Path::new(&stored_repo), &state).map_err(|e| Error::Invalid(e.to_string()))?;
        let data: Option<String> = transaction.query_row("SELECT data FROM runs WHERE id=?", [&id], |row| row.get(0)).optional()?;
        let run = serde_json::from_str(&data.ok_or_else(|| Error::Invalid("Review handle not found.".into()))?)?;
        let requests = {
            let mut statement = transaction.prepare("SELECT q.data FROM requests q JOIN run_members m ON q.id=m.request_id WHERE m.run_id=? ORDER BY m.ordinal")?;
            statement.query_map([&id], |row| row.get::<_, String>(0))?.map(|row| Ok(serde_json::from_str(&row?)?)).collect::<Result<Vec<Request>, Error>>()?
        };
        transaction.commit()?;
        Ok(RunView { run, requests })
    }).await.map_err(|e| e.to_string())
}
