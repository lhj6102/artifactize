//! Persist remote executions without replacing a newer local result.
use super::{Execution, lookup};
use crate::store::{self, Receipts, history, receipts::Error};
use rusqlite::{OptionalExtension, params};

impl Receipts {
    /// Add a remote result to its key's history as a self-contained execution when it
    /// completed after the key's latest local record, which it then replaces as the one reused;
    /// `None` when the local record is as new. The mirrored record settles a local Human wait
    /// for the key.
    pub async fn mirror_execution(
        &self,
        execution: &Execution,
    ) -> Result<Option<Execution>, String> {
        let execution = execution.clone();
        self.connection
            .call(move |db| -> Result<Option<Execution>, Error> {
                let (Some(key), Some(_), Some(_)) =
                    (&execution.key, &execution.origin, execution.verdict())
                else {
                    return Err(Error::Invalid(
                        "Only completed remote GREEN/RED results can be mirrored.".into(),
                    ));
                };
                let data = serde_json::to_string(&execution)?;
                if data.len() > history::MAX_ENTRY_BYTES {
                    return Err(Error::Invalid(
                        "Remote result exceeds the cache entry limit.".into(),
                    ));
                }
                let transaction =
                    db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                if lookup(&transaction, key)?
                    .is_some_and(|local| !execution.completed_after(&local))
                {
                    transaction.commit()?;
                    return Ok(None);
                }
                // A mirror kept after `cache rm` or GC is reused for the same remote execution.
                transaction.execute(
                    "INSERT INTO executions(id,key,eval_def_hash,status,owner_pid,owner_start_time,data) VALUES (?,?,?,?,?,?,?) ON CONFLICT(id) DO NOTHING",
                    params![
                        execution.id,
                        key,
                        execution.eval_def_hash,
                        execution.status(),
                        execution.owner_pid(),
                        execution.owner_start_time() as i64,
                        data
                    ],
                )?;
                let data: String = transaction
                    .query_row(
                        "SELECT data FROM executions WHERE id=? AND key=? AND status=?",
                        params![execution.id, key, execution.status()],
                        |row| row.get(0),
                    )
                    .optional()?
                    .ok_or_else(|| {
                        Error::Invalid(
                            "Mirrored execution ID conflicts with another execution.".into(),
                        )
                    })?;
                let Some(mirrored) = store::unreadable::evidence::<Execution>("execution", &data) else {
                    transaction.commit()?;
                    return Ok(None);
                };
                if let Some((completed_at, bytes)) = history::columns(&mirrored, data.len())? {
                    transaction.execute(
                        "UPDATE executions SET completed_at=?,bytes=?,last_used=? WHERE id=? AND completed_at IS NULL",
                        params![completed_at, bytes, crate::broker::now(), mirrored.id],
                    )?;
                }
                store::human::settle_waiting(&transaction, &mirrored)?;
                transaction.commit()?;
                Ok(Some(mirrored))
            })
            .await
            .map_err(|e| e.to_string())
    }
}
