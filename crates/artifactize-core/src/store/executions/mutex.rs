//! All tags are checked together under the execution claim's write transaction.

use rusqlite::OptionalExtension;

use super::{Claim, Execution, owner_died};
use crate::{process, runs::Worker, store::receipts::Error, types::RunStatus};

pub(super) fn conflict(
    db: &rusqlite::Connection,
    candidate: &Execution,
) -> Result<Option<Claim>, Error> {
    let records = {
        let mut statement = db.prepare(
            "SELECT id,data FROM executions WHERE status='RUNNING' AND json_array_length(data,'$.options.mutex')>0",
        )?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, crate::types::ExecutionId>(0)?,
                    row.get::<_, String>(1)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    for (id, data) in records {
        let Some(active) = crate::store::unreadable::evidence::<Execution>("execution", &data)
        else {
            // Unreadable evidence cannot reserve resources forever; retain its audit JSON.
            db.execute(
                "UPDATE executions SET status='ERROR',backend=NULL WHERE id=?",
                [id],
            )?;
            continue;
        };
        if !candidate
            .options
            .mutex
            .iter()
            .any(|tag| active.options.mutex.iter().any(|held| tag.conflicts(held)))
        {
            continue;
        }
        if live(db, &active)? {
            return Ok(Some(Claim::Mutex {
                eval_id: active.provenance.eval_id,
                run_id: active.provenance.run_id,
            }));
        }
        owner_died(db, &id, candidate.started_at)?;
    }
    Ok(None)
}

fn live(db: &rusqlite::Connection, execution: &Execution) -> Result<bool, Error> {
    let run = db
        .query_row(
            "SELECT status,json_extract(data,'$.control') FROM runs WHERE id=?",
            [&execution.provenance.run_id],
            |row| {
                Ok((
                    row.get::<_, RunStatus>(0)?,
                    row.get::<_, Option<String>>(1)?,
                ))
            },
        )
        .optional()?;
    let Some((RunStatus::Running, control)) = run else {
        return Ok(false);
    };
    let control = control
        .map(|data| serde_json::from_str::<crate::runs::Control>(&data))
        .transpose()?;
    let worker = control.and_then(|control| match control.worker {
        Worker::Starting { launcher } => Some(launcher),
        Worker::Admitted { owner } => Some(owner),
        Worker::Legacy => None,
    });
    let alive = |owner| process::is_alive(owner).map_err(|error| Error::Invalid(error.to_string()));
    Ok(alive(execution.owner)? && (worker.is_none() || alive(worker)?))
}
