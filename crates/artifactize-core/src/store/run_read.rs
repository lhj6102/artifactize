//! Reading one saved Run: the whole Run for showing it, or only the part a caller needs. A Run
//! with a large catalog has hundreds of megabytes of definitions and requests, so executing,
//! watching or settling it reads just what that step uses, and nothing is decoded twice.
use std::{collections::BTreeMap, path::Path};

use rusqlite::OptionalExtension;
use tokio_rusqlite::Connection;

use super::{
    DATABASE, Execution, Request, Run, RunView, Unreadable,
    receipts::{Error, check_files, schema_initialized},
    unreadable::{decode, skipped},
};
use crate::{
    types::{RequestId, RequestStatus, RunId},
    workspace::{canonical_target, outside_workspace},
};

/// Run `read` on one read-only snapshot of an existing state: no discovery, schema creation,
/// or execution.
async fn snapshot<T: Send + 'static>(
    state: &Path,
    read: impl FnOnce(&rusqlite::Connection, &Path) -> Result<T, Error> + Send + 'static,
) -> Result<T, String> {
    let state = canonical_target(state).map_err(|e| e.to_string())?;
    check_files(&state)?;
    let connection = Connection::open_with_flags(
        state.join(DATABASE),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .await
    .map_err(|e| e.to_string())?;
    let result = connection
        .call(move |db| -> Result<T, Error> {
            super::wait_when_busy(db)?;
            let transaction = db.transaction()?;
            if !schema_initialized(&transaction)? {
                return Err(Error::Invalid("Run not found.".into()));
            }
            let value = read(&transaction, &state)?;
            transaction.commit()?;
            Ok(value)
        })
        .await
        .map_err(|e| e.to_string());
    // A query result arrives before the DB worker drops its connection. Join the
    // close on every return path so callers can immediately release the state files.
    super::connection::finish(connection, result).await
}

/// The Run with its definitions, but none of its requests.
fn saved_run(db: &rusqlite::Connection, state: &Path, id: &RunId) -> Result<Run, Error> {
    let saved: Option<(String, String)> = db
        .query_row("SELECT repo,data FROM runs WHERE id=?", [id], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .optional()?;
    let (repo, data) = saved.ok_or_else(|| Error::Invalid("Run not found.".into()))?;
    outside_workspace(Path::new(&repo), state).map_err(|e| Error::Invalid(e.to_string()))?;
    let mut run: Run =
        decode("run", id.as_str(), &data).map_err(|error| Error::Invalid(error.to_string()))?;
    run.definitions = super::run_record::definitions(db, id)?
        .map_err(|error| Error::Invalid(error.to_string()))?;
    Ok(run)
}

/// The Run's requests in selection order, each decoded once. One that does not read is
/// skipped, explained on stderr, and returned among the unreadable records.
fn saved_requests(
    db: &rusqlite::Connection,
    id: &RunId,
) -> Result<(Vec<Request>, Vec<Unreadable>), Error> {
    let mut statement =
        db.prepare("SELECT id,data FROM requests WHERE run_id=? ORDER BY ordinal")?;
    let rows = statement.query_map([id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let (mut requests, mut unreadable) = (Vec::new(), Vec::new());
    for row in rows {
        let (request, data) = row?;
        match decode::<Request>("request", &request, &data) {
            Ok(request) => requests.push(request),
            Err(record) => {
                skipped(&record);
                unreadable.push(record);
            }
        }
    }
    Ok((requests, unreadable))
}

/// The executions the Run's requests name that do not read. Each is decoded once and let go.
fn unreadable_executions(db: &rusqlite::Connection, id: &RunId) -> Result<Vec<Unreadable>, Error> {
    let mut statement = db.prepare(
        "SELECT e.id,e.data FROM executions e WHERE e.id IN (SELECT execution_id FROM requests WHERE run_id=? AND execution_id IS NOT NULL)",
    )?;
    let rows = statement.query_map([id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut unreadable = Vec::new();
    for row in rows {
        let (execution, data) = row?;
        if let Err(record) = decode::<Execution>("execution", &execution, &data) {
            unreadable.push(record);
        }
    }
    Ok(unreadable)
}

/// The whole saved Run: its state, definitions and requests, and the records of it that do
/// not read, its requests' executions included. Existing store only: no discovery, schema
/// creation, or execution.
pub async fn read_run(state: &Path, id: &str) -> Result<RunView, String> {
    let id: RunId = id.parse()?;
    snapshot(state, move |db, state| {
        let mut run = saved_run(db, state, &id)?;
        let (requests, mut unreadable) = saved_requests(db, &id)?;
        run.show_signoffs(requests.iter().map(Request::status));
        unreadable.extend(unreadable_executions(db, &id)?);
        unreadable.sort_by(|a, b| (&a.kind, &a.id).cmp(&(&b.kind, &b.id)));
        Ok(RunView {
            run,
            requests: requests.into_iter().map(std::sync::Arc::new).collect(),
            unreadable,
        })
    })
    .await
}

/// The Run with its definitions, for executing it; its requests are read when they are needed.
pub(crate) async fn read_run_record(state: &Path, id: &RunId) -> Result<Run, String> {
    let id = id.clone();
    snapshot(state, move |db, state| saved_run(db, state, &id)).await
}

/// The Run's requests in selection order, without the Run or its definitions.
pub(crate) async fn read_run_requests_only(
    state: &Path,
    id: &RunId,
) -> Result<Vec<Request>, String> {
    let id = id.clone();
    snapshot(state, move |db, _| Ok(saved_requests(db, &id)?.0)).await
}

/// The Run's requests that have a result or an error and whose saved status differs from the
/// one `seen` holds for them, in selection order. Only those are decoded, so following a Run
/// reads each result once rather than the whole Run on every change.
pub(crate) async fn read_new_results(
    state: &Path,
    id: &RunId,
    seen: BTreeMap<RequestId, RequestStatus>,
) -> Result<Vec<Request>, String> {
    let id = id.clone();
    snapshot(state, move |db, _| {
        let changed: Vec<RequestId> = {
            let mut statement =
                db.prepare("SELECT id,status FROM requests WHERE run_id=? ORDER BY ordinal")?;
            let saved = statement
                .query_map([&id], |row| {
                    Ok((row.get::<_, RequestId>(0)?, row.get::<_, RequestStatus>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            saved
                .into_iter()
                .filter(|(request, status)| finished(*status) && seen.get(request) != Some(status))
                .map(|(request, _)| request)
                .collect()
        };
        let mut statement = db.prepare("SELECT data FROM requests WHERE id=?")?;
        let mut requests = Vec::new();
        for request in changed {
            let data: String = statement.query_row([&request], |row| row.get(0))?;
            match decode::<Request>("request", request.as_str(), &data) {
                Ok(request) => requests.push(request),
                Err(record) => skipped(&record),
            }
        }
        Ok(requests)
    })
    .await
}

/// Whether a request in `status` carries a result or an error.
fn finished(status: RequestStatus) -> bool {
    matches!(
        status,
        RequestStatus::Provisional
            | RequestStatus::Green
            | RequestStatus::Red
            | RequestStatus::Error
    )
}
