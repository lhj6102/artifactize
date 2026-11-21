//! The Chat catalog and one running writer per session.
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use super::{Receipts, receipts::Error};
use crate::{
    chat::{Model, Session},
    process::{self, ChildIdentity},
    types::SessionId,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Owner {
    birth: ChildIdentity,
    token: String,
}

impl Owner {
    pub(crate) fn current() -> Result<Self, String> {
        Ok(Self {
            birth: process::child_identity(std::process::id())
                .map_err(|error| error.to_string())?,
            token: crate::agent::uuid()?,
        })
    }

    pub(crate) fn live(&self) -> Result<bool, String> {
        process::is_alive(Some(self.birth)).map_err(|error| error.to_string())
    }
}

async fn read<T: Send + 'static>(
    state: &Path,
    read: impl FnOnce(&Connection) -> Result<T, Error> + Send + 'static,
) -> Result<Option<T>, String> {
    let Some(connection) = super::requests::open_read(state).await? else {
        return Ok(None);
    };
    let result = connection
        .call(move |db| {
            super::wait_when_busy(db)?;
            let transaction = db.transaction()?;
            if !super::receipts::schema_initialized(&transaction)? {
                return Ok(None);
            }
            let rows = read(&transaction)?;
            transaction.commit()?;
            Ok::<_, Error>(Some(rows))
        })
        .await
        .map_err(|error| error.to_string());
    super::connection::finish(connection, result).await
}

pub(crate) async fn list(state: &Path, worktree: Option<&Path>) -> Result<Vec<Session>, String> {
    let worktree = worktree
        .map(crate::workspace::canonical_target)
        .transpose()
        .map_err(|error| error.to_string())?
        .as_deref()
        .map(crate::platform::path_text);
    Ok(read(state, move |db| {
        let mut statement = db.prepare(
            "SELECT s.data, a.data FROM chat_sessions s LEFT JOIN chat_active_turns a ON a.session_id=s.id \
             WHERE ?1 IS NULL OR s.worktree=?1 ORDER BY s.updated_at DESC,s.id DESC",
        )?;
        let rows = statement.query_map([worktree], session_row)?
            .map(|row| session(row?)).collect::<Result<Vec<_>, Error>>()?;
        Ok(rows)
    }).await?.unwrap_or_default())
}

pub(crate) async fn latest_model(
    state: &Path,
    worktree: &Path,
    identity: &crate::repository::Identity,
) -> Result<Option<Model>, String> {
    let worktree = crate::platform::path_text(worktree);
    let identity = identity.clone();
    Ok(read(state, move |db| {
        let mut statement = db.prepare(
            "SELECT data FROM chat_sessions WHERE worktree=?1 ORDER BY updated_at DESC,id DESC",
        )?;
        let rows = statement.query_map([worktree], |row| row.get::<_, String>(0))?;
        for row in rows {
            let session: Session = serde_json::from_str(&row?)?;
            if session
                .repository
                .as_ref()
                .unwrap_or(&crate::repository::Identity::default())
                == &identity
            {
                return Ok(Some(session.selection));
            }
        }
        Ok(None)
    })
    .await?
    .flatten())
}

pub(crate) async fn load(state: &Path, id: &SessionId) -> Result<Session, String> {
    let id = id.clone();
    read(state, move |db| {
        let row = db.query_row(
            "SELECT s.data, a.data FROM chat_sessions s LEFT JOIN chat_active_turns a ON a.session_id=s.id WHERE s.id=?1",
            [&id], session_row,
        ).optional()?.ok_or_else(|| Error::Invalid(format!("No Chat session {id}.")))?;
        session(row)
    })
    .await?
    .ok_or("No Chat sessions have been saved.".into())
}

fn session_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(String, Option<String>)> {
    Ok((row.get(0)?, row.get(1)?))
}

fn session((data, owner): (String, Option<String>)) -> Result<Session, Error> {
    let mut session: Session = serde_json::from_str(&data)?;
    if let Some(owner) = owner {
        session.running = serde_json::from_str::<Owner>(&owner)?
            .live()
            .map_err(Error::Invalid)?;
    }
    Ok(session)
}

fn owner(db: &Connection, id: &SessionId) -> Result<Option<Owner>, Error> {
    let data: Option<String> = db
        .query_row(
            "SELECT data FROM chat_active_turns WHERE session_id=?",
            [id],
            |row| row.get(0),
        )
        .optional()?;
    data.map(|data| serde_json::from_str(&data).map_err(Error::from))
        .transpose()
}

impl Receipts {
    pub(crate) async fn create_chat(&self, session: &Session) -> Result<(), String> {
        let session = session.clone();
        self.connection
            .call(move |db| {
                db.execute(
                    "INSERT INTO chat_sessions(id,worktree,updated_at,data) VALUES (?,?,?,?)",
                    params![
                        session.id,
                        crate::platform::path_text(&session.worktree),
                        session.updated_at,
                        serde_json::to_string(&session)?
                    ],
                )?;
                Ok::<_, Error>(())
            })
            .await
            .map_err(|error| error.to_string())
    }

    pub(crate) async fn save_chat(&self, session: &Session) -> Result<(), String> {
        let session = session.clone();
        self.connection
            .call(move |db| {
                db.execute(
                    "UPDATE chat_sessions SET updated_at=?, data=? WHERE id=?",
                    params![
                        session.updated_at,
                        serde_json::to_string(&session)?,
                        session.id
                    ],
                )?;
                Ok::<_, Error>(())
            })
            .await
            .map_err(|error| error.to_string())
    }

    pub(crate) async fn claim_chat(&self, id: &SessionId, claimant: &Owner) -> Result<(), String> {
        let id = id.clone();
        let claimant = claimant.clone();
        self.connection.call(move |db| {
            let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            if let Some(current) = owner(&transaction, &id)?
                && current.live().map_err(Error::Invalid)?
            {
                return Err(Error::Invalid("This Chat session already has an active turn.".into()));
            }
            let pid = claimant.birth.pid.get();
            let start = i64::try_from(claimant.birth.start_time).map_err(|error| Error::Invalid(error.to_string()))?;
            transaction.execute(
                "INSERT INTO chat_active_turns(session_id,owner_pid,owner_start_time,data) VALUES (?,?,?,?) ON CONFLICT(session_id) DO UPDATE SET owner_pid=excluded.owner_pid,owner_start_time=excluded.owner_start_time,data=excluded.data",
                params![id, pid, start, serde_json::to_string(&claimant)?],
            )?;
            transaction.commit()?;
            Ok(())
        }).await.map_err(|error| error.to_string())
    }

    pub(crate) async fn finish_chat(&self, id: &SessionId, owner: &Owner) -> Result<(), String> {
        let id = id.clone();
        let token = owner.token.clone();
        self.connection.call(move |db| {
            db.execute("DELETE FROM chat_active_turns WHERE session_id=? AND json_extract(data,'$.token')=?", params![id, token])?;
            Ok::<_, Error>(())
        }).await.map_err(|error| error.to_string())
    }
}
