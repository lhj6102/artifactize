//! Last-request pointers for browsing the saved audit.
use super::{DATABASE, Error, Request, Run, check_files, schema_initialized};
use crate::{
    store,
    types::{Fingerprint, RequestStatus, RunId},
    workspace::{canonical_target, outside_workspace},
};
use serde::Serialize;
use std::path::Path;
use tokio_rusqlite::Connection;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LastRequest {
    pub run_id: RunId,
    pub verdict: RequestStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<Fingerprint>,
}

/// Last attempts are audit pointers, never evidence for a new current-input query.
pub async fn read_latest_requests(
    state: &Path,
    repo: &Path,
) -> Result<std::collections::BTreeMap<crate::types::EvalId, LastRequest>, String> {
    let state = canonical_target(state).map_err(|e| e.to_string())?;
    outside_workspace(repo, &state).map_err(|e| e.to_string())?;
    check_files(&state)?;
    if !state
        .join(DATABASE)
        .try_exists()
        .map_err(|e| e.to_string())?
    {
        return Ok(Default::default());
    }
    let connection = Connection::open_with_flags(
        state.join(DATABASE),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .await
    .map_err(|e| e.to_string())?;
    let repo = repo.to_path_buf();
    let result = connection
        .call(move |db| -> Result<_, Error> {
            store::wait_when_busy(db)?;
            let transaction = db.transaction()?;
            if !schema_initialized(&transaction)? {
                return Ok(Default::default());
            }
            let latest = {
                let mut statement = transaction.prepare(
                    "SELECT data,run_id FROM (
                SELECT q.eval_id,q.data,q.run_id,
                    row_number() OVER (PARTITION BY q.eval_id ORDER BY r.rowid DESC) AS rank
                FROM requests q JOIN runs r ON r.id=q.run_id
                WHERE r.repo=?
            ) WHERE rank=1 ORDER BY eval_id",
                )?;
                // A Run's record is large and most latest requests share a few Runs: read and
                // decode each Run once, not once per eval.
                let mut run_data = transaction.prepare("SELECT data FROM runs WHERE id=?")?;
                let mut readable = std::collections::BTreeMap::<String, bool>::new();
                statement
                    .query_map([crate::platform::path_text(&repo)], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })?
                    .filter_map(|row| match row {
                        Err(error) => Some(Err(error)),
                        Ok((data, run)) => {
                            let run_readable = match readable.get(&run) {
                                Some(known) => *known,
                                None => {
                                    let known = match run_data
                                        .query_row([&run], |row| row.get::<_, String>(0))
                                    {
                                        Ok(run_data) => {
                                            store::unreadable::evidence::<Run>("run", &run_data)
                                                .is_some()
                                        }
                                        Err(error) => return Some(Err(error)),
                                    };
                                    readable.insert(run, known);
                                    known
                                }
                            };
                            run_readable
                                .then(|| store::unreadable::evidence::<Request>("request", &data))
                                .flatten()
                                .map(|request| {
                                    Ok((
                                        request.eval_id.clone(),
                                        LastRequest {
                                            run_id: request.run_id.clone(),
                                            verdict: request.status(),
                                            fingerprint: request.fingerprint,
                                        },
                                    ))
                                })
                        }
                    })
                    .collect::<Result<_, _>>()?
            };
            transaction.commit()?;
            Ok(latest)
        })
        .await
        .map_err(|e| e.to_string());
    store::connection::finish(connection, result).await
}
