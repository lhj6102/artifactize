//! Machine-wide backend slots: one row per Agent review in flight, shared by every process
//! that uses this state database. A slot whose owner process is gone (by pid and start time)
//! is free again.

use rusqlite::{OptionalExtension, params};

use super::{Receipts, receipts::Error};
use crate::process::{self, ChildIdentity};

pub(super) const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS backend_slots(execution_id TEXT PRIMARY KEY, backend TEXT NOT NULL, owner_pid INTEGER NOT NULL, owner_start_time INTEGER NOT NULL, acquired_at TEXT NOT NULL);
    CREATE INDEX IF NOT EXISTS backend_slots_backend ON backend_slots(backend);";

impl Receipts {
    /// Take one of the `limit` slots of `backend` for an execution, freeing the slots of owners
    /// that are no longer alive first. False while every slot is held by a live process.
    pub(crate) async fn acquire_slot(
        &self,
        backend: &str,
        limit: u32,
        execution_id: &str,
        owner: ChildIdentity,
    ) -> Result<bool, String> {
        let (backend, execution_id) = (backend.to_owned(), execution_id.to_owned());
        self.connection
            .call(move |db| -> Result<bool, Error> {
                let transaction =
                    db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                let held: Option<String> = transaction
                    .query_row(
                        "SELECT backend FROM backend_slots WHERE execution_id=?",
                        [&execution_id],
                        |row| row.get(0),
                    )
                    .optional()?;
                if held.is_some() {
                    return Ok(true);
                }
                let owners = {
                    let mut statement = transaction.prepare(
                        "SELECT execution_id,owner_pid,owner_start_time FROM backend_slots WHERE backend=?",
                    )?;
                    statement
                        .query_map([&backend], |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                ChildIdentity {
                                    pid: row.get(1)?,
                                    start_time: row.get::<_, i64>(2)? as u64,
                                },
                            ))
                        })?
                        .collect::<Result<Vec<_>, _>>()?
                };
                let mut live = 0;
                for (id, holder) in owners {
                    if process::is_alive(holder).map_err(|e| Error::Invalid(e.to_string()))? {
                        live += 1;
                    } else {
                        transaction
                            .execute("DELETE FROM backend_slots WHERE execution_id=?", [id])?;
                    }
                }
                let acquired = live < limit;
                if acquired {
                    transaction.execute(
                        "INSERT INTO backend_slots(execution_id,backend,owner_pid,owner_start_time,acquired_at) VALUES (?,?,?,?,?)",
                        params![execution_id, backend, owner.pid, owner.start_time as i64, crate::broker::now()],
                    )?;
                }
                transaction.commit()?;
                Ok(acquired)
            })
            .await
            .map_err(|e| e.to_string())
    }

    pub(crate) async fn release_slot(&self, execution_id: &str) -> Result<(), String> {
        let execution_id = execution_id.to_owned();
        self.connection
            .call(move |db| -> Result<(), Error> {
                db.execute(
                    "DELETE FROM backend_slots WHERE execution_id=?",
                    [execution_id],
                )?;
                Ok(())
            })
            .await
            .map_err(|e| e.to_string())
    }
}
