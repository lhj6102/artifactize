//! Global monitor catalog, intentionally independent of any visible Run page.
use super::{
    DATABASE,
    receipts::{Error, check_files, schema_initialized},
};
use crate::{
    repository::Identity,
    types::{ExecutionId, RequestId, RequestStatus, RunStatus, Sha256Bytes},
    workspace::{canonical_target, outside_workspaces},
};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Signoff {
    Execution(ExecutionId),
    Request(RequestId),
}

#[derive(Debug, Clone)]
pub struct CatalogRun {
    pub repo_path: PathBuf,
    pub repository: Identity,
    pub status: RunStatus,
    pub red: u64,
    pub error: u64,
    pub waiting: Vec<Signoff>,
}

/// Global attention facts; unreadable Runs are skipped individually with their reason.
pub async fn read_catalog(state: &Path) -> Result<Vec<CatalogRun>, String> {
    let state = canonical_target(state).map_err(|error| error.to_string())?;
    check_files(&state)?;
    if !state
        .join(DATABASE)
        .try_exists()
        .map_err(|error| error.to_string())?
    {
        return Ok(Vec::new());
    }
    let connection = tokio_rusqlite::Connection::open_with_flags(
        state.join(DATABASE),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .await
    .map_err(|error| error.to_string())?;
    connection
        .call(move |db| -> Result<_, Error> {
            db.busy_timeout(super::SQLITE_BUSY_TIMEOUT)?;
            let transaction = db.transaction()?;
            if !schema_initialized(&transaction)? {
                return Ok(Vec::new());
            }
            let catalog = in_snapshot(&transaction, &state)?;
            transaction.commit()?;
            Ok(catalog)
        })
        .await
        .map_err(|error| error.to_string())
}

pub(super) fn in_snapshot(
    db: &rusqlite::Connection,
    state: &Path,
) -> Result<Vec<CatalogRun>, Error> {
    in_snapshot_cached(db, state, &mut Facts::default())
}

#[derive(Default)]
pub(super) struct Facts {
    runs: std::collections::BTreeMap<crate::types::RunId, (Sha256Bytes, Option<CatalogRun>)>,
}

pub(super) fn in_snapshot_cached(
    db: &rusqlite::Connection,
    state: &Path,
    facts: &mut Facts,
) -> Result<Vec<CatalogRun>, Error> {
    let mut statement = db.prepare_cached("SELECT id,data FROM runs ORDER BY rowid")?;
    let runs = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, crate::types::RunId>(0)?,
                row.get::<_, String>(1)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut catalog = std::collections::BTreeMap::new();
    let mut order = Vec::new();
    let mut retained = std::collections::BTreeMap::new();
    for (id, data) in runs {
        let digest = Sha256Bytes::of(data.as_bytes());
        let fact = match facts.runs.remove(&id) {
            Some((saved, fact)) if saved == digest => fact,
            _ => super::unreadable::evidence::<super::Run>("run", &data).map(|run| {
                let status = run.status();
                CatalogRun {
                    repo_path: run.repo_path,
                    repository: run.repository,
                    status,
                    red: 0,
                    error: 0,
                    waiting: Vec::new(),
                }
            }),
        };
        retained.insert(id.clone(), (digest, fact.clone()));
        if let Some(fact) = fact {
            order.push(id.clone());
            catalog.insert(id, fact);
        }
    }
    outside_workspaces(catalog.values().map(|run| run.repo_path.as_path()), state)
        .map_err(|error| Error::Invalid(error.to_string()))?;
    facts.runs = retained;
    let mut statement = db.prepare_cached(
        "SELECT q.run_id,q.status,q.execution_id,coalesce(json_extract(e.data,'$.provenance.requestId'),json_extract(q.data,'$.provenance.requestId'),q.id)
        FROM requests q LEFT JOIN executions e ON e.id=q.execution_id WHERE q.status IN ('RED','ERROR','WAITING_HUMAN') ORDER BY q.run_id,q.ordinal",
    )?;
    let attention = statement.query_map([], |row| {
        Ok((
            row.get::<_, crate::types::RunId>(0)?,
            row.get::<_, RequestStatus>(1)?,
            row.get::<_, Option<ExecutionId>>(2)?,
            row.get::<_, RequestId>(3)?,
        ))
    })?;
    for row in attention {
        let (id, status, execution, request) = row?;
        if let Some(run) = catalog.get_mut(&id) {
            match status {
                RequestStatus::Red => run.red += 1,
                RequestStatus::Error => run.error += 1,
                RequestStatus::WaitingHuman => run
                    .waiting
                    .push(execution.map_or(Signoff::Request(request), Signoff::Execution)),
                _ => unreachable!("attention query filters states"),
            }
        }
    }
    Ok(order
        .into_iter()
        .filter_map(|id| catalog.remove(&id))
        .collect())
}
