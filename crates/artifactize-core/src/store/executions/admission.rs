//! Claims, capacity, and the identity of in-flight executions.
use super::{Execution, lookup, mutex};
use crate::{
    process,
    store::{self, Receipts, receipts::Error},
    types::{ExecutionStatus, ReuseKey},
};
use rusqlite::{OptionalExtension, params};

pub enum Claim {
    Owned,
    BudgetExhausted,
    /// Every machine-wide slot of the execution's backend is held.
    Full,
    /// A live execution holds an equal, ancestor or descendant mutex tag.
    Mutex {
        eval_id: crate::types::EvalId,
        run_id: crate::types::RunId,
    },
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

/// End a RUNNING execution whose owner process is gone, which frees its key and its slot.
pub(super) fn owner_died(
    db: &rusqlite::Connection,
    id: &crate::types::ExecutionId,
    at: crate::types::Timestamp,
) -> Result<(), Error> {
    db.execute(
        "UPDATE executions SET status='ERROR',data=json_set(data,'$.status','ERROR','$.error','Execution owner died.','$.errorCode','OWNER_DIED','$.result',NULL,'$.completedAt',?1,'$.provenance.completedAt',?1) WHERE id=?2 AND status='RUNNING'",
        params![at, id],
    )?;
    Ok(())
}

/// How many slots of `backend` the RUNNING executions of live owners hold; the executions of
/// dead owners end, freeing theirs.
fn held_slots(
    db: &rusqlite::Connection,
    backend: &str,
    at: crate::types::Timestamp,
) -> Result<u32, Error> {
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
        if process::is_alive(Some(owner)).map_err(|e| Error::Invalid(e.to_string()))? {
            held += 1;
        } else {
            owner_died(db, &id, at)?;
        }
    }
    Ok(held)
}

/// Corruption must not reserve a reuse key or machine-wide slot forever. Preserve its
/// original JSON for the unreadable audit, but retire the invalid active index entry.
fn retire_unreadable(
    db: &rusqlite::Connection,
    key: Option<&ReuseKey>,
    backend: Option<&str>,
) -> Result<(), Error> {
    let records = {
        let mut statement = db.prepare("SELECT id,data FROM executions WHERE status IN ('RUNNING','WAITING_HUMAN') AND ((?1 IS NOT NULL AND key=?1) OR (?2 IS NOT NULL AND backend=?2))")?;
        statement
            .query_map(params![key, backend], |row| {
                Ok((
                    row.get::<_, crate::types::ExecutionId>(0)?,
                    row.get::<_, String>(1)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    for (id, data) in records {
        if let Err(error) = store::unreadable::decode::<Execution>("execution", id.as_str(), &data)
        {
            use std::io::Write;
            let _ = writeln!(std::io::stderr().lock(), "{error}");
            db.execute(
                "UPDATE executions SET status='ERROR',backend=NULL WHERE id=?",
                [id],
            )?;
        }
    }
    Ok(())
}

/// The complete declared effective profile an in-flight execution is joined on, as its
/// stored JSON. Only [`ProfileIdentity::of`] makes one, so every join compares one spelling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ProfileIdentity(String);

impl ProfileIdentity {
    fn of(profile: &crate::config::StoredProfile) -> Result<Self, Error> {
        Ok(Self(serde_json::to_string(profile)?))
    }
}

impl rusqlite::ToSql for ProfileIdentity {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        self.0.to_sql()
    }
}

/// The one definition of in-flight identity. Completed evidence still uses only its
/// reuse key; joining additionally requires the complete declared effective profile.
pub(super) fn sharing_identity(
    key: &ReuseKey,
    profile: &crate::config::StoredProfile,
) -> Result<(ReuseKey, ProfileIdentity), Error> {
    Ok((key.clone(), ProfileIdentity::of(profile)?))
}

pub(super) fn active_owner(
    db: &rusqlite::Connection,
    key: &ReuseKey,
    profile: Option<&ProfileIdentity>,
) -> Result<
    Option<(
        crate::types::ExecutionId,
        process::ChildIdentity,
        ExecutionStatus,
    )>,
    Error,
> {
    let row = db
        .query_row(
            "SELECT id,owner_pid,owner_start_time,status,data FROM executions WHERE key=?1 AND (?2 IS NULL OR profile_identity=?2) AND status IN ('RUNNING','WAITING_HUMAN')",
            params![key, profile],
            |row| {
                Ok((
                    row.get::<_, crate::types::ExecutionId>(0)?,
                    process::ChildIdentity {
                        pid: row.get(1)?,
                        start_time: row.get::<_, i64>(2)? as u64,
                    },
                    row.get::<_, ExecutionStatus>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .optional()?;
    Ok(row.and_then(|(id, owner, status, data)| {
        store::unreadable::evidence::<Execution>("execution", &data).map(|_| (id, owner, status))
    }))
}

pub(super) fn available_to_waiter(
    db: &rusqlite::Connection,
    key: &ReuseKey,
    waiting_for: Option<&crate::types::ExecutionId>,
    profile: &ProfileIdentity,
) -> Result<Option<Claim>, Error> {
    if let Some(id) = waiting_for {
        let data: Option<String> = db
            .query_row(
                "SELECT data FROM executions WHERE id=? AND key=? AND profile_identity=?",
                params![id, key, profile],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(data) = data
            && let Some(execution) = store::unreadable::evidence::<Execution>("execution", &data)
        {
            match execution.status() {
                ExecutionStatus::Green | ExecutionStatus::Red => {
                    return Ok(Some(Claim::Reuse(Box::new(execution))));
                }
                ExecutionStatus::Error
                    if !matches!(
                        execution.error_code(),
                        Some(
                            crate::types::FailureCode::Cancelled
                                | crate::types::FailureCode::OwnerDied
                        )
                    ) =>
                {
                    return Ok(Some(Claim::Reuse(Box::new(execution))));
                }
                // A joined request stays with its execution, even if an independent
                // forced or different-profile review publishes another result meanwhile.
                ExecutionStatus::Running
                    if process::is_alive(execution.owner)
                        .map_err(|error| Error::Invalid(error.to_string()))? =>
                {
                    return Ok(Some(Claim::Wait(execution.id)));
                }
                ExecutionStatus::WaitingHuman => return Ok(Some(Claim::WaitHuman(execution.id))),
                _ => {}
            }
        }
    }
    available(db, key, Some(profile))
}

pub(super) fn available(
    db: &rusqlite::Connection,
    key: &ReuseKey,
    profile: Option<&ProfileIdentity>,
) -> Result<Option<Claim>, Error> {
    if let Some(execution) = lookup(db, key)? {
        return Ok(Some(Claim::Reuse(Box::new(execution))));
    }
    if let Some((id, owner, status)) = active_owner(db, key, profile)? {
        if status == ExecutionStatus::WaitingHuman {
            return Ok(Some(Claim::WaitHuman(id)));
        }
        if process::is_alive(Some(owner)).map_err(|e| Error::Invalid(e.to_string()))? {
            return Ok(Some(Claim::Wait(id)));
        }
    }
    Ok(None)
}

pub(super) async fn open_read_only(
    state: &std::path::Path,
) -> Result<Option<tokio_rusqlite::Connection>, String> {
    let state = crate::workspace::canonical_target(state).map_err(|e| e.to_string())?;
    store::receipts::check_files(&state)?;
    if !state
        .join(store::DATABASE)
        .try_exists()
        .map_err(|e| e.to_string())?
    {
        return Ok(None);
    }
    tokio_rusqlite::Connection::open_with_flags(
        state.join(store::DATABASE),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .await
    .map(Some)
    .map_err(|e| e.to_string())
}

/// Read the latest completed record or the live owner of each key without creating or
/// changing the database.
impl Receipts {
    /// Claim an execution for its request. A `keyed` claim (a key, not forced) first reuses
    /// the key's latest record or waits for its live execution with the same profile. Starting needs
    /// `allow_start` and, on a backend with a machine-wide limit, a free slot: the slots the
    /// backend's RUNNING executions hold are counted and this execution inserted RUNNING in
    /// one write transaction. Mutex tags are also checked and held in that transaction,
    /// including for unkeyed and forced executions. An unkeyed or forced execution without
    /// a limit or mutex tags is saved when it completes.
    pub async fn claim_execution(
        &self,
        execution: &Execution,
        keyed: bool,
        waiting_for: Option<&crate::types::ExecutionId>,
        allow_start: bool,
        capacity: Option<Capacity>,
    ) -> Result<Claim, String> {
        let execution = execution.clone();
        let waiting_for = waiting_for.cloned();
        let identity = execution
            .key
            .as_ref()
            .filter(|_| keyed)
            .map(|key| sharing_identity(key, &execution.profile))
            .transpose()
            .map_err(|error| error.to_string())?;
        self.connection
            .call(move |db| -> Result<Claim, Error> {
                let key = identity.as_ref().map(|(key, _)| key);
                let profile = identity.as_ref().map(|(_, profile)| profile);
                if let Some(key) = key {
                    let transaction = db.transaction()?;
                    if let Some(claim) =
                        available_to_waiter(&transaction, key, waiting_for.as_ref(), profile.expect("keyed identity"))?
                    {
                        return Ok(claim);
                    }
                }
                if !allow_start && key.is_none() {
                    return Ok(Claim::BudgetExhausted);
                }
                let capacity = capacity.zip(execution.backend());
                if key.is_none() && capacity.is_none() && execution.options.mutex.is_empty() {
                    return Ok(Claim::Owned);
                }
                let transaction =
                    db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                if let Some(key) = key
                    && let Some(claim) =
                        available_to_waiter(&transaction, key, waiting_for.as_ref(), profile.expect("keyed identity"))?
                {
                    return Ok(claim);
                }
                // The read fast path may have observed RUNNING before publication and
                // then a dead publisher. Confirm availability in a fresh write snapshot
                // even when this waiter has no budget to start another execution.
                if !allow_start {
                    return Ok(Claim::BudgetExhausted);
                }
                retire_unreadable(&transaction, key, capacity.as_ref().map(|(_, backend)| *backend))?;
                if !execution.options.mutex.is_empty()
                    && let Some(conflict) = mutex::conflict(&transaction, &execution)?
                {
                    transaction.commit()?;
                    return Ok(conflict);
                }
                if let Some((capacity, backend)) = capacity {
                    if (capacity.stopped)() {
                        return Ok(Claim::BudgetExhausted);
                    }
                    if held_slots(&transaction, backend, execution.started_at)? >= capacity.limit {
                        transaction.commit()?;
                        return Ok(Claim::Full);
                    }
                }
                if let Some(key) = key
                    && let Some((id, _, _)) = active_owner(&transaction, key, profile)?
                {
                    owner_died(&transaction, &id, execution.started_at)?;
                }
                let inserted = transaction.execute(
                    "INSERT INTO executions(id,key,eval_def_hash,status,owner_pid,owner_start_time,backend,profile_identity,data) VALUES (?,?,?,?,?,?,?,?,?) ON CONFLICT(key,profile_identity) WHERE key IS NOT NULL AND status IN ('RUNNING','WAITING_HUMAN') DO NOTHING",
                    params![
                        execution.id,
                        key,
                        execution.eval_def_hash,
                        execution.status(),
                        execution.owner_pid(),
                        execution.owner_start_time() as i64,
                        execution.backend(),
                        profile,
                        serde_json::to_string(&execution)?
                    ],
                )?;
                let claim = match key {
                    Some(key) if inserted == 0 => Claim::Wait(
                        active_owner(&transaction, key, profile)?
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
}
