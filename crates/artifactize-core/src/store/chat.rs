//! The Chat catalog, single-writer turn ownership, v12 single-transaction permission CAS,
//! atomic `AcpBinding` generation CAS, and SQLite multi-table watermark.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use super::{Receipts, receipts::Error};
use crate::{
    acp::{
        AcpBinding, ConfigOption, PermissionOption, PermissionRecord, PermissionStatus,
        VerifyBridgeState, WarmOwner, wire::MAX_PENDING_PERMISSIONS,
    },
    chat::{ChatTarget, Session, SessionView, ToolKind},
    monitor::live::Place,
    process::{self, ChildIdentity},
    repository,
    types::{SessionId, Timestamp},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Owner {
    birth: ChildIdentity,
    pub(crate) token: String,
    #[serde(default)]
    pub(crate) stop_requested: bool,
    #[serde(default)]
    pub(crate) live_config_options: Vec<ConfigOption>,
    #[serde(default)]
    pub(crate) verify_bridge_state: VerifyBridgeState,
}

impl Owner {
    pub(crate) fn current() -> Result<Self, String> {
        Ok(Self {
            birth: process::child_identity(std::process::id())
                .map_err(|error| error.to_string())?,
            token: crate::agent::uuid()?,
            stop_requested: false,
            live_config_options: Vec::new(),
            verify_bridge_state: VerifyBridgeState::NotConnected,
        })
    }

    pub(crate) fn in_current_process(&self) -> bool {
        self.birth.pid.get() == std::process::id()
    }

    pub(crate) fn live(&self) -> Result<bool, String> {
        process::is_alive(Some(self.birth)).map_err(|error| error.to_string())
    }
}

/// Multi-table SQLite watermark across `(chat_sessions, chat_active_turns, chat_pending_permissions)`
/// tracked alongside JSONL size in Desktop `watch.rs` so secondary windows converge via `chat_open`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatWatermark {
    pub session_updated_at: Option<String>,
    pub active_turn_hash: Option<String>,
    pub permissions_digest: String,
}

/// Input to insert a new `PENDING` permission row in one `IMMEDIATE` transaction.
pub(crate) struct NewPermission {
    pub session_id: SessionId,
    pub owner_token: String,
    pub permission_id: String,
    pub rpc_request_id: String,
    pub title: String,
    pub tool_kind: ToolKind,
    pub summary: String,
    pub detail: Option<String>,
    pub options: Vec<PermissionOption>,
    pub created_at: Timestamp,
}

/// Result of attempting to transition a permission from `SELECTED` to `DELIVERING` right before
/// writing the ACP wire response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DeliverClaim {
    /// CAS succeeded (`SELECTED -> DELIVERING`); owner may now write `selected` to wire.
    Deliver { option_id: String },
    /// Stop won or owner lost token; owner must send ACP `cancelled` instead.
    Cancelled,
    /// Permission is still `PENDING` (user has not chosen yet).
    StillPending,
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
            "SELECT s.data, a.data, \
             EXISTS(SELECT 1 FROM chat_pending_permissions p WHERE p.session_id=s.id AND p.status='PENDING') \
             FROM chat_sessions s LEFT JOIN chat_active_turns a ON a.session_id=s.id \
             WHERE ?1 IS NULL OR s.worktree=?1 ORDER BY s.updated_at DESC,s.id DESC",
        )?;
        let rows = statement
            .query_map([worktree], session_row)?
            .map(|row| session(row?, &state_path))
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(rows)
    })
    .await?
    .unwrap_or_default())
}

pub(crate) async fn for_branch(state: &Path, branch: &Place) -> Result<Vec<Session>, String> {
    let branch = branch.clone();
    let state_path = state.to_owned();
    Ok(read(state, move |db| {
        let mut statement = db.prepare(
            "SELECT s.data, a.data, \
             EXISTS(SELECT 1 FROM chat_pending_permissions p WHERE p.session_id=s.id AND p.status='PENDING') \
             FROM chat_sessions s LEFT JOIN chat_active_turns a ON a.session_id=s.id \
             ORDER BY s.updated_at DESC,s.id DESC",
        )?;
        let mut sessions = Vec::new();
        for row in statement.query_map([], session_row)? {
            let (data, owner, pending_perm) = row?;
            let saved: Session = serde_json::from_str(&data)?;
            if saved.branch() == branch {
                sessions.push(with_activity(saved, owner, pending_perm, &state_path)?);
            }
        }
        Ok(sessions)
    })
    .await?
    .unwrap_or_default())
}

/// Read only active owners, even if they are no longer alive.
pub(crate) async fn active_turns(
    state: &Path,
    live: bool,
) -> Result<Vec<(Session, Owner)>, String> {
    let state_path = state.to_owned();
    Ok(read(state, move |db| {
        let mut query = db.prepare(
            "SELECT s.data, a.data FROM chat_active_turns a JOIN chat_sessions s ON s.id=a.session_id",
        )?;
        let rows = query.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
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
    })
    .await?
    .unwrap_or_default())
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
        .ok_or_else(|| "No Chat sessions have been saved.".into())
}

/// Read all permission rows for a session (ordered by creation time).
pub(crate) async fn list_permissions(
    state: &Path,
    id: &SessionId,
) -> Result<Vec<PermissionRecord>, String> {
    let id = id.clone();
    Ok(read(state, move |db| query_permissions(db, &id))
        .await?
        .unwrap_or_default())
}

/// Compute a lightweight SQLite watermark across `(chat_sessions, chat_active_turns,
/// chat_pending_permissions)` for `watch.rs`.
pub(crate) async fn watermark(
    state: &Path,
    id: &SessionId,
) -> Result<Option<ChatWatermark>, String> {
    let id = id.clone();
    read(state, move |db| {
        let session_updated_at: Option<String> = db
            .query_row(
                "SELECT updated_at || '|' || data FROM chat_sessions WHERE id=?1",
                [&id],
                |row| row.get(0),
            )
            .optional()?;
        let active_turn_hash: Option<String> = db
            .query_row(
                "SELECT data FROM chat_active_turns WHERE session_id=?1",
                [&id],
                |row| row.get(0),
            )
            .optional()?;
        let mut stmt = db.prepare(
            "SELECT permission_id, status, COALESCE(selected_option_id,'') \
             FROM chat_pending_permissions WHERE session_id=?1 ORDER BY created_at, permission_id",
        )?;
        let perms = stmt
            .query_map([&id], |row| {
                Ok(format!(
                    "{}:{}:{}",
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?
            .join(",");
        Ok(ChatWatermark {
            session_updated_at,
            active_turn_hash,
            permissions_digest: perms,
        })
    })
    .await
}

fn session_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(String, Option<String>, bool)> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
}

fn load_session(db: &Connection, state: &Path, id: &SessionId) -> Result<Session, Error> {
    let row = db
        .query_row(
            "SELECT s.data, a.data, \
             EXISTS(SELECT 1 FROM chat_pending_permissions p WHERE p.session_id=s.id AND p.status='PENDING') \
             FROM chat_sessions s LEFT JOIN chat_active_turns a ON a.session_id=s.id WHERE s.id=?1",
            [id],
            session_row,
        )
        .optional()?
        .ok_or_else(|| Error::Invalid(format!("No Chat session {id}.")))?;
    session(row, state)
}

fn session(
    (data, owner, pending_perm): (String, Option<String>, bool),
    state: &Path,
) -> Result<Session, Error> {
    with_activity(serde_json::from_str(&data)?, owner, pending_perm, state)
}

fn with_activity(
    mut session: Session,
    owner_json: Option<String>,
    pending_perm: bool,
    state: &Path,
) -> Result<Session, Error> {
    if let Some(owner_json) = owner_json {
        let active: Owner = serde_json::from_str(&owner_json)?;
        session.running = active.live().map_err(Error::Invalid)?;
        if session.running {
            session.live_config_options = active.live_config_options;
            session.verify_bridge_state = active.verify_bridge_state;
        }
    }
    session.waiting_permission = session.running && pending_perm;
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

fn query_permissions(db: &Connection, id: &SessionId) -> Result<Vec<PermissionRecord>, Error> {
    let mut stmt = db.prepare(
        "SELECT session_id, owner_token, permission_id, rpc_request_id, title, tool_kind, \
         summary, detail, options_json, status, selected_option_id, created_at, resolved_at \
         FROM chat_pending_permissions WHERE session_id=?1 ORDER BY created_at ASC, permission_id ASC",
    )?;
    let rows = stmt.query_map([id], |row| {
        Ok((
            row.get::<_, SessionId>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, String>(8)?,
            row.get::<_, String>(9)?,
            row.get::<_, Option<String>>(10)?,
            row.get::<_, Timestamp>(11)?,
            row.get::<_, Option<Timestamp>>(12)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (
            session_id,
            owner_token,
            permission_id,
            rpc_request_id,
            title,
            tool_kind,
            summary,
            detail,
            options_json,
            status_str,
            selected_option_id,
            created_at,
            resolved_at,
        ) = row?;
        let options: Vec<PermissionOption> = serde_json::from_str(&options_json)?;
        let status = PermissionStatus::from_db(&status_str).map_err(Error::Invalid)?;
        out.push(PermissionRecord {
            session_id,
            owner_token,
            permission_id,
            rpc_request_id,
            title,
            tool_kind: ToolKind::from_db(&tool_kind),
            summary,
            detail,
            options,
            status,
            selected_option_id,
            created_at,
            resolved_at,
        });
    }
    Ok(out)
}

/// Settle any remaining open permissions for a session inside an active transaction:
/// `PENDING`/`SELECTED` -> `CANCELLED`, interrupted `DELIVERING` -> `EXPIRED`.
fn cleanup_open_permissions(
    tx: &rusqlite::Transaction<'_>,
    session_id: &SessionId,
    at: Timestamp,
) -> Result<(), Error> {
    tx.execute(
        "UPDATE chat_pending_permissions SET status='CANCELLED', resolved_at=?1 \
         WHERE session_id=?2 AND status IN ('PENDING','SELECTED')",
        params![at, session_id],
    )?;
    tx.execute(
        "UPDATE chat_pending_permissions SET status='EXPIRED', resolved_at=?1 \
         WHERE session_id=?2 AND status='DELIVERING'",
        params![at, session_id],
    )?;
    Ok(())
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

    /// Accept the turn with an explicit target, or take the latest saved target atomically.
    /// Merges `view` and `acp_binding` from the current committed row so stale caller copies
    /// never overwrite a concurrent `generation` bump.
    pub(crate) async fn accept_chat(
        &self,
        session: &Session,
        selection: Option<ChatTarget>,
    ) -> Result<(ChatTarget, Option<AcpBinding>), String> {
        let mut session = session.clone();
        self.connection
            .call(move |db| {
                let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let current = saved_chat(&transaction, &session.id)?;
                let target = selection.or(current.target.clone()).ok_or_else(|| {
                    Error::Invalid(
                        "The saved Chat target was removed before the turn started.".into(),
                    )
                })?;
                let mut acp_binding = current.acp_binding.clone();
                // If the explicit selection switches to a different ACP agent (or away from ACP),
                // atomically bump generation and clear remote_session_id / warm_owner.
                if let Some(binding) = acp_binding.as_mut() {
                    let same_agent = matches!(
                        &target,
                        ChatTarget::Acp { agent, .. } if agent == &binding.agent_id
                    );
                    if !same_agent {
                        binding.generation = binding.generation.saturating_add(1);
                        binding.remote_session_id = None;
                        binding.warm_owner = None;
                    }
                }
                session.target = Some(target.clone());
                session.view = current.view;
                session.acp_binding = acp_binding.clone();
                session.moved_from = None;
                save(&transaction, &session)?;
                transaction.commit()?;
                Ok::<_, Error>((target, acp_binding))
            })
            .await
            .map_err(|error| error.to_string())
    }

    /// Save the session's ChatTarget; when switching ACP agent (or engine), atomically
    /// increment `acp_binding.generation` and clear `remote_session_id` and `warm_owner`.
    pub(crate) async fn set_chat_target(
        &self,
        state: &Path,
        id: &SessionId,
        target: ChatTarget,
    ) -> Result<(Session, bool), String> {
        let state = state.to_owned();
        let id = id.clone();
        self.connection
            .call(move |db| {
                let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let mut session = load_session(&transaction, &state, &id)?;
                let changed = session.target.as_ref() != Some(&target);
                let mut agent_changed = false;
                if changed {
                    let prev_agent = match &session.target {
                        Some(ChatTarget::Acp { agent, .. }) => Some(agent.clone()),
                        _ => None,
                    };
                    let next_agent = match &target {
                        ChatTarget::Acp { agent, .. } => Some(agent.clone()),
                        _ => None,
                    };
                    if prev_agent != next_agent {
                        agent_changed = true;
                        if let Some(binding) = session.acp_binding.as_mut() {
                            binding.generation = binding.generation.saturating_add(1);
                            binding.remote_session_id = None;
                            binding.warm_owner = None;
                        }
                    }
                    session.target = Some(target);
                    session.updated_at = crate::broker::now();
                    save(&transaction, &session)?;
                }
                transaction.commit()?;
                Ok::<_, Error>((session, agent_changed))
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

    /// Share the same writer reservation as turn admission. A dead owner does not bar a move.
    /// Atomically bumps `acp_binding.generation` and clears `remote_session_id` + `warm_owner`.
    pub(crate) async fn set_chat_worktree(
        &self,
        state: &Path,
        id: &SessionId,
        worktree: PathBuf,
        repository: repository::Identity,
    ) -> Result<Option<(Session, bool)>, String> {
        let state = state.to_owned();
        let id = id.clone();
        self.connection
            .call(move |db| {
                let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let mut session = load_session(&transaction, &state, &id)?;
                if session.running {
                    return Ok::<_, Error>(None);
                }
                let changed = session.worktree != worktree;
                if changed {
                    session.moved_from = Some(std::mem::replace(&mut session.worktree, worktree));
                    session.repository = Some(repository);
                    if let Some(binding) = session.acp_binding.as_mut() {
                        binding.generation = binding.generation.saturating_add(1);
                        binding.remote_session_id = None;
                        binding.warm_owner = None;
                    }
                    session.updated_at = crate::broker::now().max(session.updated_at);
                    save(&transaction, &session)?;
                }
                transaction.commit()?;
                Ok(Some((session, changed)))
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

    /// Claim active turn ownership, cleaning up any stale open permissions from a dead owner
    /// in the same `IMMEDIATE` transaction.
    pub(crate) async fn claim_chat(
        &self,
        state: &Path,
        id: &SessionId,
        claimant: &Owner,
    ) -> Result<Session, String> {
        let state = state.to_owned();
        let id = id.clone();
        let claimant = claimant.clone();
        self.connection
            .call(move |db| {
                let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
                if let Some(current) = owner(&transaction, &id)?
                    && current.live().map_err(Error::Invalid)?
                {
                    return Err(Error::Invalid(
                        "This Chat session already has an active turn.".into(),
                    ));
                }
                cleanup_open_permissions(&transaction, &id, crate::broker::now())?;
                let session = load_session(&transaction, &state, &id)?;
                let pid = claimant.birth.pid.get();
                let start = i64::try_from(claimant.birth.start_time)
                    .map_err(|error| Error::Invalid(error.to_string()))?;
                transaction.execute(
                    "INSERT INTO chat_active_turns(session_id,owner_pid,owner_start_time,data) VALUES (?,?,?,?) \
                     ON CONFLICT(session_id) DO UPDATE SET owner_pid=excluded.owner_pid,owner_start_time=excluded.owner_start_time,data=excluded.data",
                    params![id, pid, start, serde_json::to_string(&claimant)?],
                )?;
                transaction.commit()?;
                Ok(session)
            })
            .await
            .map_err(|error| error.to_string())
    }

    /// Update live ACP turn metadata (`live_config_options`, `verify_bridge_state`) on
    /// `chat_active_turns.data` for the active owner token.
    pub(crate) async fn update_chat_turn_meta(
        &self,
        id: &SessionId,
        owner_token: &str,
        live_config_options: Vec<ConfigOption>,
        verify_bridge_state: VerifyBridgeState,
    ) -> Result<(), String> {
        let id = id.clone();
        let owner_token = owner_token.to_owned();
        self.connection
            .call(move |db| {
                let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
                if let Some(mut active) = owner(&transaction, &id)?
                    && active.token == owner_token
                {
                    active.live_config_options = live_config_options;
                    active.verify_bridge_state = verify_bridge_state;
                    transaction.execute(
                        "UPDATE chat_active_turns SET data=?1 WHERE session_id=?2",
                        params![serde_json::to_string(&active)?, id],
                    )?;
                }
                transaction.commit()?;
                Ok::<_, Error>(())
            })
            .await
            .map_err(|error| error.to_string())
    }

    /// Step 1 of Permission CAS: insert `PENDING` row in one `IMMEDIATE` transaction verifying
    /// `Owner.token == owner_token && !stop_requested` and open count `< MAX_PENDING_PERMISSIONS`.
    /// Returns `Ok(true)` if admitted, `Ok(false)` if rejected (caller immediately sends ACP `cancelled`).
    pub(crate) async fn insert_chat_permission(&self, perm: NewPermission) -> Result<bool, String> {
        self.connection
            .call(move |db| {
                let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let Some(active) = owner(&transaction, &perm.session_id)? else {
                    return Ok::<_, Error>(false);
                };
                if active.token != perm.owner_token || active.stop_requested {
                    return Ok(false);
                }
                let open_count: i64 = transaction.query_row(
                    "SELECT COUNT(*) FROM chat_pending_permissions \
                     WHERE session_id=?1 AND status IN ('PENDING','SELECTED','DELIVERING')",
                    [&perm.session_id],
                    |row| row.get(0),
                )?;
                if open_count as usize >= MAX_PENDING_PERMISSIONS {
                    return Ok(false);
                }
                transaction.execute(
                    "INSERT INTO chat_pending_permissions(\
                        session_id, owner_token, permission_id, rpc_request_id, title, tool_kind, \
                        summary, detail, options_json, status, selected_option_id, created_at, resolved_at\
                    ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,'PENDING',NULL,?10,NULL)",
                    params![
                        perm.session_id,
                        perm.owner_token,
                        perm.permission_id,
                        perm.rpc_request_id,
                        perm.title,
                        perm.tool_kind.as_str(),
                        perm.summary,
                        perm.detail,
                        serde_json::to_string(&perm.options)?,
                        perm.created_at,
                    ],
                )?;
                transaction.commit()?;
                Ok(true)
            })
            .await
            .map_err(|error| error.to_string())
    }

    /// Step 2 of Permission CAS: user selects `option_id` (`PENDING -> SELECTED`), validating
    /// active owner token, `!stop_requested`, and membership in `options_json` in one `IMMEDIATE`
    /// transaction.
    pub(crate) async fn respond_chat_permission(
        &self,
        session_id: &SessionId,
        permission_id: &str,
        option_id: &str,
    ) -> Result<bool, String> {
        let session_id = session_id.clone();
        let permission_id = permission_id.to_owned();
        let option_id = option_id.to_owned();
        self.connection
            .call(move |db| {
                let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let Some(active) = owner(&transaction, &session_id)? else {
                    return Err(Error::Invalid(
                        "No active Chat turn owns this permission request.".into(),
                    ));
                };
                if !active.live().map_err(Error::Invalid)? || active.stop_requested {
                    return Err(Error::Invalid(
                        "This Chat turn is stopping or no longer running.".into(),
                    ));
                }
                let options_json: Option<String> = transaction
                    .query_row(
                        "SELECT options_json FROM chat_pending_permissions \
                         WHERE session_id=?1 AND owner_token=?2 AND permission_id=?3 AND status='PENDING'",
                        params![session_id, active.token, permission_id],
                        |row| row.get(0),
                    )
                    .optional()?;
                let Some(options_json) = options_json else {
                    return Ok::<_, Error>(false);
                };
                let options: Vec<PermissionOption> = serde_json::from_str(&options_json)?;
                if !options.iter().any(|opt| opt.option_id == option_id) {
                    return Err(Error::Invalid(format!(
                        "Permission option {option_id:?} is not valid for this request."
                    )));
                }
                let updated = transaction.execute(
                    "UPDATE chat_pending_permissions SET status='SELECTED', selected_option_id=?1 \
                     WHERE session_id=?2 AND owner_token=?3 AND permission_id=?4 AND status='PENDING'",
                    params![option_id, session_id, active.token, permission_id],
                )?;
                transaction.commit()?;
                Ok(updated != 0)
            })
            .await
            .map_err(|error| error.to_string())
    }

    /// Step 3 of Permission CAS: immediately before writing `selected` onto the ACP wire,
    /// atomically check `Owner.token == owner_token && !stop_requested` and CAS
    /// `SELECTED -> DELIVERING`.
    pub(crate) async fn begin_deliver_chat_permission(
        &self,
        session_id: &SessionId,
        owner_token: &str,
        permission_id: &str,
    ) -> Result<DeliverClaim, String> {
        let session_id = session_id.clone();
        let owner_token = owner_token.to_owned();
        let permission_id = permission_id.to_owned();
        self.connection
            .call(move |db| {
                let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let Some(active) = owner(&transaction, &session_id)? else {
                    return Ok::<_, Error>(DeliverClaim::Cancelled);
                };
                if active.token != owner_token || active.stop_requested {
                    return Ok(DeliverClaim::Cancelled);
                }
                let row: Option<(String, Option<String>)> = transaction
                    .query_row(
                        "SELECT status, selected_option_id FROM chat_pending_permissions \
                         WHERE session_id=?1 AND owner_token=?2 AND permission_id=?3",
                        params![session_id, owner_token, permission_id],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()?;
                match row {
                    Some((status, _)) if status == "PENDING" => Ok(DeliverClaim::StillPending),
                    Some((status, Some(option_id))) if status == "SELECTED" => {
                        let updated = transaction.execute(
                            "UPDATE chat_pending_permissions SET status='DELIVERING' \
                             WHERE session_id=?1 AND owner_token=?2 AND permission_id=?3 AND status='SELECTED'",
                            params![session_id, owner_token, permission_id],
                        )?;
                        transaction.commit()?;
                        if updated == 1 {
                            Ok(DeliverClaim::Deliver { option_id })
                        } else {
                            Ok(DeliverClaim::Cancelled)
                        }
                    }
                    _ => Ok(DeliverClaim::Cancelled),
                }
            })
            .await
            .map_err(|error| error.to_string())
    }

    /// Finish delivery after writing the ACP wire reply: `DELIVERING -> DELIVERED` or `EXPIRED`
    /// (on broken pipe / EOF).
    pub(crate) async fn finish_deliver_chat_permission(
        &self,
        session_id: &SessionId,
        owner_token: &str,
        permission_id: &str,
        delivered: bool,
    ) -> Result<(), String> {
        let session_id = session_id.clone();
        let owner_token = owner_token.to_owned();
        let permission_id = permission_id.to_owned();
        let next = if delivered { "DELIVERED" } else { "EXPIRED" };
        self.connection
            .call(move |db| {
                let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
                transaction.execute(
                    "UPDATE chat_pending_permissions SET status=?1, resolved_at=?2 \
                     WHERE session_id=?3 AND owner_token=?4 AND permission_id=?5 AND status='DELIVERING'",
                    params![next, crate::broker::now(), session_id, owner_token, permission_id],
                )?;
                transaction.commit()?;
                Ok::<_, Error>(())
            })
            .await
            .map_err(|error| error.to_string())
    }

    /// Uniform SQLite Stop transaction used across ALL local and remote Stop paths:
    /// atomically commits `stop_requested = true` AND cancels all `PENDING`/`SELECTED` permission
    /// rows for the session in one `IMMEDIATE` transaction.
    pub(crate) async fn stop_chat(&self, id: &SessionId, expected: Owner) -> Result<bool, String> {
        let id = id.clone();
        self.connection
            .call(move |db| {
                if !expected.live().map_err(Error::Invalid)? {
                    return Ok(false);
                }
                let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let updated = transaction.execute(
                    "UPDATE chat_active_turns SET data=json_set(data,'$.stop_requested',json('true')) \
                     WHERE session_id=?1 AND json_extract(data,'$.token')=?2",
                    params![id, expected.token],
                )?;
                if updated != 0 {
                    transaction.execute(
                        "UPDATE chat_pending_permissions SET status='CANCELLED', resolved_at=?1 \
                         WHERE session_id=?2 AND owner_token=?3 AND status IN ('PENDING','SELECTED')",
                        params![crate::broker::now(), id, expected.token],
                    )?;
                }
                transaction.commit()?;
                Ok::<_, Error>(updated != 0)
            })
            .await
            .map_err(|error| error.to_string())
    }

    /// Stop by exact owner token (used by local `Chat::stop` / `Chat::close` before firing the
    /// in-memory cancellation token so delivery CAS always observes `stop_requested = true`).
    pub(crate) async fn stop_chat_by_token(
        &self,
        id: &SessionId,
        token: &str,
    ) -> Result<bool, String> {
        let id = id.clone();
        let token = token.to_owned();
        self.connection
            .call(move |db| {
                let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let updated = transaction.execute(
                    "UPDATE chat_active_turns SET data=json_set(data,'$.stop_requested',json('true')) \
                     WHERE session_id=?1 AND json_extract(data,'$.token')=?2",
                    params![id, token],
                )?;
                if updated != 0 {
                    transaction.execute(
                        "UPDATE chat_pending_permissions SET status='CANCELLED', resolved_at=?1 \
                         WHERE session_id=?2 AND owner_token=?3 AND status IN ('PENDING','SELECTED')",
                        params![crate::broker::now(), id, token],
                    )?;
                }
                transaction.commit()?;
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

    /// Atomically update `acp_binding` with `generation` CAS (initializing if `expected_generation`
    /// is `None`, or checking `current.generation == expected_generation`).
    pub(crate) async fn save_acp_binding_cas(
        &self,
        id: &SessionId,
        expected_generation: Option<u64>,
        binding: AcpBinding,
    ) -> Result<bool, String> {
        let id = id.clone();
        self.connection
            .call(move |db| {
                let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let mut current = saved_chat(&transaction, &id)?;
                let current_gen = current.acp_binding.as_ref().map(|b| b.generation);
                if current_gen != expected_generation {
                    return Ok::<_, Error>(false);
                }
                current.acp_binding = Some(binding);
                save(&transaction, &current)?;
                transaction.commit()?;
                Ok(true)
            })
            .await
            .map_err(|error| error.to_string())
    }

    /// Bump `acp_binding.generation` and clear `remote_session_id` + `warm_owner` atomically
    /// (e.g. when refusing `session/load` for a dead/foreign `warm_owner` or digest mismatch).
    pub(crate) async fn bump_acp_generation(&self, id: &SessionId) -> Result<u64, String> {
        let id = id.clone();
        self.connection
            .call(move |db| {
                let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let mut current = saved_chat(&transaction, &id)?;
                let next_gen = current
                    .acp_binding
                    .as_ref()
                    .map_or(1, |b| b.generation.saturating_add(1));
                if let Some(binding) = current.acp_binding.as_mut() {
                    binding.generation = next_gen;
                    binding.remote_session_id = None;
                    binding.warm_owner = None;
                    save(&transaction, &current)?;
                }
                transaction.commit()?;
                Ok::<_, Error>(next_gen)
            })
            .await
            .map_err(|error| error.to_string())
    }

    /// Conditionally clear `warm_owner = None` only when `generation == expected_generation`
    /// and `warm_owner == current_owner`.
    pub(crate) async fn clear_acp_warm_owner(
        &self,
        id: &SessionId,
        expected_generation: u64,
        current_owner: WarmOwner,
    ) -> Result<bool, String> {
        let id = id.clone();
        self.connection
            .call(move |db| {
                let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let mut current = saved_chat(&transaction, &id)?;
                let mut cleared = false;
                if let Some(binding) = current.acp_binding.as_mut()
                    && binding.generation == expected_generation
                    && binding.warm_owner == Some(current_owner)
                {
                    binding.warm_owner = None;
                    save(&transaction, &current)?;
                    cleared = true;
                }
                transaction.commit()?;
                Ok::<_, Error>(cleared)
            })
            .await
            .map_err(|error| error.to_string())
    }

    /// Settle the turn, clean up remaining permissions (`PENDING`/`SELECTED` -> `CANCELLED`,
    /// `DELIVERING` -> `EXPIRED`), release active turn ownership, and optionally advance
    /// `acp_binding.settled_send` / `warm_owner` with `generation` CAS — all in one transaction.
    pub(crate) async fn finish_chat(
        &self,
        id: &SessionId,
        owner: &Owner,
        settled: Option<&Session>,
        settled_acp: Option<(u64, Option<usize>, Option<WarmOwner>)>,
    ) -> Result<bool, String> {
        let id = id.clone();
        let token = owner.token.clone();
        let settled = settled.cloned();
        self.connection
            .call(move |db| {
                let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let released = transaction.execute(
                    "DELETE FROM chat_active_turns WHERE session_id=?1 AND json_extract(data,'$.token')=?2",
                    params![id, token],
                )?;
                if released != 0 {
                    cleanup_open_permissions(&transaction, &id, crate::broker::now())?;
                    if let Some(session) = settled {
                        let mut current = saved_chat(&transaction, &id)?;
                        current.settle_from(&session);
                        if let Some((expected_gen, completed_send, warm_owner)) = settled_acp
                            && let Some(binding) = current.acp_binding.as_mut()
                            && binding.generation == expected_gen
                        {
                            if let Some(send) = completed_send {
                                binding.settled_send = binding.settled_send.max(send);
                            }
                            binding.warm_owner = warm_owner;
                        }
                        save(&transaction, &current)?;
                    }
                }
                transaction.commit()?;
                Ok::<_, Error>(released != 0)
            })
            .await
            .map_err(|error| error.to_string())
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
        "UPDATE chat_sessions SET worktree=?, updated_at=?, data=? WHERE id=?",
        params![
            crate::platform::path_text(&session.worktree),
            session.updated_at,
            serde_json::to_string(session)?,
            session.id
        ],
    )?;
    Ok(())
}
