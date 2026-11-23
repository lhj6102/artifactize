//! The Chat catalog and one running writer per session.
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use super::{Receipts, receipts::Error};
use crate::{
    chat::{Model, Session, SessionView},
    monitor::live::Place,
    process::{self, ChildIdentity},
    types::SessionId,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Owner {
    birth: ChildIdentity,
    token: String,
    #[serde(default)]
    stop_requested: bool,
}

impl Owner {
    pub(crate) fn current() -> Result<Self, String> {
        Ok(Self {
            birth: process::child_identity(std::process::id())
                .map_err(|error| error.to_string())?,
            token: crate::agent::uuid()?,
            stop_requested: false,
        })
    }

    pub(crate) fn in_current_process(&self) -> bool {
        self.birth.pid.get() == std::process::id()
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
    let state_path = state.to_owned();
    Ok(read(state, move |db| {
        let mut statement = db.prepare(
            "SELECT s.data, a.data FROM chat_sessions s LEFT JOIN chat_active_turns a ON a.session_id=s.id \
             WHERE ?1 IS NULL OR s.worktree=?1 ORDER BY s.updated_at DESC,s.id DESC",
        )?;
        let rows = statement.query_map([worktree], session_row)?
            .map(|row| session(row?, &state_path)).collect::<Result<Vec<_>, Error>>()?;
        Ok(rows)
    }).await?.unwrap_or_default())
}

pub(crate) async fn for_branch(state: &Path, branch: &Place) -> Result<Vec<Session>, String> {
    let branch = branch.clone();
    let state_path = state.to_owned();
    Ok(read(state, move |db| {
        let mut statement = db.prepare(
            "SELECT s.data, a.data FROM chat_sessions s LEFT JOIN chat_active_turns a ON a.session_id=s.id \
             ORDER BY s.updated_at DESC,s.id DESC",
        )?;
        let mut sessions = Vec::new();
        for row in statement.query_map([], session_row)? {
            let (data, owner) = row?;
            let saved: Session = serde_json::from_str(&data)?;
            if saved.branch() == branch {
                sessions.push(with_activity(saved, owner, &state_path)?);
            }
        }
        Ok(sessions)
    }).await?.unwrap_or_default())
}

/// Read only active owners, even if they are no longer alive.
pub(crate) async fn active_turns(
    state: &Path,
    live: bool,
) -> Result<Vec<(Session, Owner)>, String> {
    let state_path = state.to_owned();
    Ok(read(state, move |db| {
        let mut query = db.prepare("SELECT s.data, a.data FROM chat_active_turns a JOIN chat_sessions s ON s.id=a.session_id")?;
        let rows = query.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
        let mut active = Vec::new();
        for row in rows {
            let (data, owner) = row?;
            let owner: Owner = serde_json::from_str(&owner)?;
            if owner.live().map_err(Error::Invalid)? == live {
                let mut session: Session = serde_json::from_str(&data)?;
                if !live {
                    refresh_activity(&mut session, &state_path);
                }
                active.push((session, owner));
            }
        }
        Ok(active)
    }).await?.unwrap_or_default())
}

pub(crate) async fn turn_owner(state: &Path, id: &SessionId) -> Result<Option<Owner>, String> {
    let id = id.clone();
    Ok(read(state, move |db| owner(db, &id)).await?.flatten())
}

pub(crate) async fn load(state: &Path, id: &SessionId) -> Result<Session, String> {
    let id = id.clone();
    let state_path = state.to_owned();
    read(state, move |db| load_session(db, &state_path, &id))
        .await?
        .ok_or("No Chat sessions have been saved.".into())
}

fn session_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(String, Option<String>)> {
    Ok((row.get(0)?, row.get(1)?))
}

fn load_session(db: &Connection, state: &Path, id: &SessionId) -> Result<Session, Error> {
    let row = db.query_row(
        "SELECT s.data, a.data FROM chat_sessions s LEFT JOIN chat_active_turns a ON a.session_id=s.id WHERE s.id=?1",
        [id], session_row,
    ).optional()?.ok_or_else(|| Error::Invalid(format!("No Chat session {id}.")))?;
    session(row, state)
}

fn session((data, owner): (String, Option<String>), state: &Path) -> Result<Session, Error> {
    with_activity(serde_json::from_str(&data)?, owner, state)
}

fn with_activity(
    mut session: Session,
    owner: Option<String>,
    state: &Path,
) -> Result<Session, Error> {
    if let Some(owner) = owner {
        session.running = serde_json::from_str::<Owner>(&owner)?
            .live()
            .map_err(Error::Invalid)?;
    }
    refresh_activity(&mut session, state);
    Ok(session)
}

fn refresh_activity(session: &mut Session, state: &Path) {
    if session.needs_activity() {
        session.refresh_activity(crate::chat::recorded_outcome(state, &session.id));
    }
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

    /// Accept the turn with an explicit choice, or take the latest saved choice atomically.
    pub(crate) async fn accept_chat(
        &self,
        session: &Session,
        selection: Option<Model>,
    ) -> Result<Model, String> {
        let mut session = session.clone();
        self.connection
            .call(move |db| {
                let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let current = saved_chat(&transaction, &session.id)?;
                session.selection = selection.unwrap_or(current.selection);
                session.view = current.view;
                save(&transaction, &session)?;
                transaction.commit()?;
                Ok::<_, Error>(session.selection)
            })
            .await
            .map_err(|error| error.to_string())
    }

    pub(crate) async fn set_chat_model(
        &self,
        state: &Path,
        id: &SessionId,
        selection: Model,
    ) -> Result<Session, String> {
        let state = state.to_owned();
        let id = id.clone();
        self.connection
            .call(move |db| {
                let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let mut session = load_session(&transaction, &state, &id)?;
                if session.selection != selection {
                    session.selection = selection;
                    session.updated_at = crate::broker::now();
                    save(&transaction, &session)?;
                }
                transaction.commit()?;
                Ok::<_, Error>(session)
            })
            .await
            .map_err(|error| error.to_string())
    }

    pub(crate) async fn set_chat_view(
        &self,
        state: &Path,
        id: &SessionId,
        view: SessionView,
    ) -> Result<(Session, bool), String> {
        let state = state.to_owned();
        let id = id.clone();
        self.connection
            .call(move |db| {
                let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let mut session = load_session(&transaction, &state, &id)?;
                let changed = session.view != view;
                if changed {
                    session.view = view;
                    save(&transaction, &session)?;
                }
                transaction.commit()?;
                Ok::<_, Error>((session, changed))
            })
            .await
            .map_err(|error| error.to_string())
    }

    pub(crate) async fn mark_chat_seen(
        &self,
        state: &Path,
        id: &SessionId,
    ) -> Result<bool, String> {
        let state = state.to_owned();
        let id = id.clone();
        self.connection
            .call(move |db| {
                let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let mut session = load_session(&transaction, &state, &id)?;
                let changed = session.mark_seen();
                if changed {
                    save(&transaction, &session)?;
                }
                transaction.commit()?;
                Ok::<_, Error>(changed)
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

    /// Request only this exact observed turn, not a replacement accepted after the read.
    pub(crate) async fn stop_chat(&self, id: &SessionId, expected: Owner) -> Result<bool, String> {
        let id = id.clone();
        self.connection
            .call(move |db| {
                if !expected.live().map_err(Error::Invalid)? {
                    return Ok(false);
                }
                let updated = db.execute(
                    "UPDATE chat_active_turns SET data=json_set(data,'$.stop_requested',json('true')) \
                     WHERE session_id=? AND json_extract(data,'$.token')=?",
                    params![id, expected.token],
                )?;
                Ok::<_, Error>(updated != 0)
            })
            .await
            .map_err(|error| error.to_string())
    }

    pub(crate) async fn chat_stop_requested(
        &self,
        id: &SessionId,
        expected: &Owner,
    ) -> Result<bool, String> {
        let id = id.clone();
        let token = expected.token.clone();
        self.connection
            .call(move |db| {
                Ok::<_, Error>(
                    owner(db, &id)?
                        .is_some_and(|current| current.token == token && current.stop_requested),
                )
            })
            .await
            .map_err(|error| error.to_string())
    }

    pub(crate) async fn finish_chat(
        &self,
        id: &SessionId,
        owner: &Owner,
        settled: Option<&Session>,
    ) -> Result<bool, String> {
        let id = id.clone();
        let token = owner.token.clone();
        let settled = settled.cloned();
        self.connection.call(move |db| {
            let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let released = transaction.execute("DELETE FROM chat_active_turns WHERE session_id=? AND json_extract(data,'$.token')=?", params![id, token])?;
            if released != 0 && let Some(mut session) = settled {
                // A selector change accepted during the turn is for the next message.
                let current = saved_chat(&transaction, &id)?;
                session.selection = current.selection;
                session.view = current.view;
                session.updated_at = session.updated_at.max(current.updated_at);
                save(&transaction, &session)?;
            }
            transaction.commit()?;
            Ok::<_, Error>(released != 0)
        }).await.map_err(|error| error.to_string())
    }
}

fn saved_chat(db: &Connection, id: &SessionId) -> Result<Session, Error> {
    let data: String = db.query_row("SELECT data FROM chat_sessions WHERE id=?", [id], |row| {
        row.get(0)
    })?;
    Ok(serde_json::from_str(&data)?)
}

fn save(db: &Connection, session: &Session) -> Result<(), Error> {
    db.execute(
        "UPDATE chat_sessions SET updated_at=?, data=? WHERE id=?",
        params![
            session.updated_at,
            serde_json::to_string(session)?,
            session.id
        ],
    )?;
    Ok(())
}
