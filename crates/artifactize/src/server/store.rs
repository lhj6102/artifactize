use std::path::Path;

use base64::Engine;
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio_rusqlite::Connection;

use crate::broker::now;

pub const DATABASE: &str = "review-store.sqlite";
const SCHEMA_VERSION: u32 = 3;
/// Bound retained shared-review history, including small records that would not
/// reach the byte cap; least-recently-used records are evicted above either limit.
pub const MAX_ENTRIES: i64 = 100_000;
/// Bound JSON payload storage in the shared review database independently of record count.
pub const MAX_BYTES: i64 = 4 * 1024 * 1024 * 1024;
/// Keep persistent principal labels and token-admin output bounded; token names
/// are printable ASCII identifiers, not the opaque bearer secrets themselves.
const MAX_TOKEN_NAME_BYTES: usize = 64;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, clap::ValueEnum,
)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// Look up entries.
    Read,
    /// Publish runtime and Agent entries.
    Publish,
    /// Additionally publish Human sign-offs.
    Human,
}

impl Scope {
    pub fn name(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Publish => "publish",
            Self::Human => "human",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Token {
    pub name: String,
    pub scopes: Vec<Scope>,
    pub created_at: String,
    pub revoked_at: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Revocation {
    pub name: String,
    pub revoked_at: String,
    pub purged_entries: usize,
}

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Invalid(String),
}

#[derive(Clone)]
pub struct Store {
    connection: Connection,
}

fn digest(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Version 3 keeps every record of a 0.5 reuse key. Records of versions 1 and 2, keyed by
/// (Eval definition hash, fingerprint) as 0.3 and 0.4 computed them, cannot be mapped to the new
/// key and are dropped; tokens are kept.
fn reuse_keys(db: &rusqlite::Transaction<'_>) -> Result<(), Error> {
    db.execute_batch("DROP TABLE IF EXISTS entries;")?;
    Ok(())
}

impl Store {
    /// Open or create `review-store.sqlite`, separate from the local `state.sqlite`.
    pub async fn open(state: &Path) -> Result<Self, String> {
        let state = crate::store::state_dir(Some(state))?;
        crate::platform::create_private_dir_all(&state).map_err(|e| e.to_string())?;
        for suffix in ["", "-wal", "-shm"] {
            let path = state.join(format!("{DATABASE}{suffix}"));
            if path
                .symlink_metadata()
                .is_ok_and(|metadata| !metadata.is_file())
            {
                return Err(format!(
                    "Review store files must be regular files: {}",
                    path.display()
                ));
            }
        }
        let connection = Connection::open(state.join(DATABASE))
            .await
            .map_err(|e| e.to_string())?;
        connection
            .call(|db| -> Result<(), Error> {
                db.busy_timeout(crate::store::SQLITE_BUSY_TIMEOUT)?;
                let version: u32 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
                if version > SCHEMA_VERSION {
                    return Err(Error::Invalid(format!(
                        "Unsupported review store schema version: {version}"
                    )));
                }
                db.pragma_update(None, "journal_mode", "WAL")?;
                let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
                // Another process may have upgraded between the two reads.
                if matches!(
                    transaction.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))?,
                    1 | 2
                ) {
                    reuse_keys(&transaction)?;
                }
                transaction.execute_batch(
                    "CREATE TABLE IF NOT EXISTS tokens(name TEXT PRIMARY KEY, hash TEXT NOT NULL UNIQUE, scopes TEXT NOT NULL, created_at TEXT NOT NULL, revoked_at TEXT);
                    CREATE TABLE IF NOT EXISTS entries(key TEXT NOT NULL, execution_id TEXT NOT NULL, publisher TEXT NOT NULL, completed_at TEXT NOT NULL, bytes INTEGER NOT NULL, last_used TEXT NOT NULL, data TEXT NOT NULL, UNIQUE(key, execution_id));
                    CREATE INDEX IF NOT EXISTS entries_key ON entries(key, completed_at);
                    CREATE INDEX IF NOT EXISTS entries_lru ON entries(last_used, key);
                    CREATE INDEX IF NOT EXISTS entries_publisher ON entries(publisher);",
                )?;
                transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
                transaction.commit()?;
                Ok(())
            })
            .await
            .map_err(|e| e.to_string())?;
        Ok(Self { connection })
    }

    /// Create a token; only its SHA-256 is stored, so the caller prints it once.
    pub async fn add_token(&self, name: &str, scopes: &[Scope]) -> Result<String, String> {
        if !(1..=MAX_TOKEN_NAME_BYTES).contains(&name.len())
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err("Token names need 1–64 characters from [A-Za-z0-9._-].".into());
        }
        let mut scopes = scopes.to_vec();
        scopes.sort();
        scopes.dedup();
        if scopes.is_empty() {
            return Err("A token needs at least one scope.".into());
        }
        let mut secret = [0; 32];
        getrandom::fill(&mut secret).map_err(|_| "Cannot obtain secure randomness.".to_owned())?;
        let token = format!(
            "azt_{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(secret)
        );
        let (name, hash, scopes) = (
            name.to_owned(),
            digest(&token),
            serde_json::to_string(&scopes).map_err(|e| e.to_string())?,
        );
        self.connection
            .call(move |db| -> Result<(), Error> {
                let inserted = db.execute(
                    "INSERT INTO tokens(name,hash,scopes,created_at) VALUES (?,?,?,?) ON CONFLICT(name) DO NOTHING",
                    params![name, hash, scopes, now()],
                )?;
                if inserted == 0 {
                    return Err(Error::Invalid(format!(
                        "Token name {name} is already used; names of revoked tokens are kept."
                    )));
                }
                Ok(())
            })
            .await
            .map_err(|e| e.to_string())?;
        Ok(token)
    }

    pub async fn tokens(&self) -> Result<Vec<Token>, String> {
        self.connection
            .call(|db| -> Result<_, Error> {
                let mut statement = db.prepare(
                    "SELECT name,scopes,created_at,revoked_at FROM tokens ORDER BY name",
                )?;
                let rows = statement
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get(2)?,
                            row.get(3)?,
                        ))
                    })?
                    .map(|row| {
                        let (name, scopes, created_at, revoked_at) = row?;
                        Ok(Token {
                            name,
                            scopes: serde_json::from_str(&scopes)?,
                            created_at,
                            revoked_at,
                        })
                    })
                    .collect::<Result<_, Error>>()?;
                Ok(rows)
            })
            .await
            .map_err(|e| e.to_string())
    }

    /// Revoke immediately; `purge` also deletes every entry that token published.
    pub async fn revoke(&self, name: &str, purge: bool) -> Result<Revocation, String> {
        let name = name.to_owned();
        self.connection
            .call(move |db| -> Result<_, Error> {
                let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
                transaction.execute(
                    "UPDATE tokens SET revoked_at=? WHERE name=? AND revoked_at IS NULL",
                    params![now(), name],
                )?;
                let revoked_at: String = transaction
                    .query_row(
                        "SELECT revoked_at FROM tokens WHERE name=?",
                        [&name],
                        |row| row.get(0),
                    )
                    .optional()?
                    .ok_or_else(|| Error::Invalid(format!("Unknown token {name}.")))?;
                let purged_entries = if purge {
                    transaction.execute("DELETE FROM entries WHERE publisher=?", [&name])?
                } else {
                    0
                };
                transaction.commit()?;
                Ok(Revocation {
                    name,
                    revoked_at,
                    purged_entries,
                })
            })
            .await
            .map_err(|e| e.to_string())
    }

    /// Remove every record of a key.
    pub async fn remove(&self, key: &str) -> Result<bool, String> {
        let key = key.to_owned();
        self.connection
            .call(move |db| -> Result<_, Error> {
                Ok(db.execute("DELETE FROM entries WHERE key=?", [key])? != 0)
            })
            .await
            .map_err(|e| e.to_string())
    }

    /// The active token's name and scopes; revoked and unknown tokens yield `None`.
    pub(super) async fn authenticate(
        &self,
        token: &str,
    ) -> Result<Option<(String, Vec<Scope>)>, String> {
        let hash = digest(token);
        self.connection
            .call(move |db| -> Result<_, Error> {
                let row: Option<(String, String)> = db
                    .query_row(
                        "SELECT name,scopes FROM tokens WHERE hash=? AND revoked_at IS NULL",
                        [hash],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()?;
                row.map(|(name, scopes)| Ok((name, serde_json::from_str(&scopes)?)))
                    .transpose()
            })
            .await
            .map_err(|e| e.to_string())
    }

    /// The latest record of each found key; hits update that record's last use.
    pub(super) async fn lookup(&self, keys: Vec<String>) -> Result<Vec<String>, String> {
        self.connection
            .call(move |db| -> Result<_, Error> {
                let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let used = now();
                let mut found = Vec::new();
                for key in keys {
                    let data: Option<String> = transaction
                        .query_row(
                            "UPDATE entries SET last_used=? WHERE rowid=(SELECT rowid FROM entries WHERE key=? ORDER BY completed_at DESC, rowid DESC LIMIT 1) RETURNING data",
                            params![used, key],
                            |row| row.get(0),
                        )
                        .optional()?;
                    found.extend(data);
                }
                transaction.commit()?;
                Ok(found)
            })
            .await
            .map_err(|e| e.to_string())
    }

    /// Append a record to its key's history; `false` when this execution is already stored.
    pub(super) async fn insert(
        &self,
        key: &str,
        execution_id: &str,
        publisher: &str,
        completed_at: &str,
        data: String,
    ) -> Result<bool, String> {
        let record = (
            key.to_owned(),
            execution_id.to_owned(),
            publisher.to_owned(),
            completed_at.to_owned(),
        );
        self.connection
            .call(move |db| -> Result<_, Error> {
                let (key, execution_id, publisher, completed_at) = record;
                let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let used = now();
                let created = transaction.execute(
                    "INSERT INTO entries(key,execution_id,publisher,completed_at,bytes,last_used,data) VALUES (?,?,?,?,?,?,?) ON CONFLICT DO NOTHING",
                    params![key, execution_id, publisher, completed_at, data.len() as i64, used, data],
                )? == 1;
                // Evict least-recently-used records above the caps, never the new one.
                loop {
                    let (count, bytes): (i64, i64) = transaction.query_row(
                        "SELECT count(*),coalesce(sum(bytes),0) FROM entries",
                        [],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )?;
                    if count <= MAX_ENTRIES && bytes <= MAX_BYTES {
                        break;
                    }
                    if transaction.execute(
                        "DELETE FROM entries WHERE rowid=(SELECT rowid FROM entries WHERE NOT (key=?1 AND execution_id=?2) ORDER BY last_used,key,completed_at LIMIT 1)",
                        params![key, execution_id],
                    )? == 0
                    {
                        break;
                    }
                }
                transaction.commit()?;
                Ok(created)
            })
            .await
            .map_err(|e| e.to_string())
    }
}
