//! Global monitor catalog, intentionally independent of any visible Run page.
use super::{
    DATABASE,
    receipts::{Error, check_files, schema_initialized},
};
use crate::{
    repository::Identity,
    types::{ExecutionId, RequestId, RequestStatus, RunStatus},
    workspace::{canonical_target, outside_workspace},
};
use serde::Deserialize;
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Facts {
    repo_path: PathBuf,
    #[serde(flatten)]
    repository: Identity,
    status: RunStatus,
}

/// Only identity and attention facts are loaded, never definitions or result bodies.
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
            let runs = {
                let mut statement = transaction.prepare(
                    "SELECT id,json_object('repoPath',repo,'commonDir',json_extract(data,'$.commonDir'),'worktreePath',json_extract(data,'$.worktreePath'),'branch',json_extract(data,'$.branch'),'status',status) FROM runs ORDER BY rowid",
                )?;
                statement
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, crate::types::RunId>(0)?,
                            row.get::<_, String>(1)?,
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()?
            };
            let mut requests = transaction.prepare(
                "SELECT q.status,q.execution_id,coalesce(json_extract(e.data,'$.provenance.requestId'),json_extract(q.data,'$.provenance.requestId'),q.id) FROM requests q LEFT JOIN executions e ON e.id=q.execution_id WHERE q.run_id=? AND q.status IN ('RED','ERROR','WAITING_HUMAN')",
            )?;
            let catalog = runs
                .into_iter()
                .map(|(id, data)| {
                    let facts: Facts = serde_json::from_str(&data)?;
                    outside_workspace(&facts.repo_path, &state)
                        .map_err(|error| Error::Invalid(error.to_string()))?;
                    let attention = requests
                        .query_map([id], |row| {
                            Ok((
                                row.get::<_, RequestStatus>(0)?,
                                row.get::<_, Option<ExecutionId>>(1)?,
                                row.get::<_, RequestId>(2)?,
                            ))
                        })?
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(CatalogRun {
                        repo_path: facts.repo_path,
                        repository: facts.repository,
                        status: facts.status,
                        red: attention
                            .iter()
                            .filter(|(status, _, _)| *status == RequestStatus::Red)
                            .count() as u64,
                        error: attention
                            .iter()
                            .filter(|(status, _, _)| *status == RequestStatus::Error)
                            .count() as u64,
                        waiting: attention
                            .into_iter()
                            .filter(|(status, _, _)| *status == RequestStatus::WaitingHuman)
                            .map(|(_, execution, request)| {
                                execution.map_or(Signoff::Request(request), Signoff::Execution)
                            })
                            .collect(),
                    })
                })
                .collect::<Result<Vec<_>, Error>>()?;
            drop(requests);
            transaction.commit()?;
            Ok(catalog)
        })
        .await
        .map_err(|error| error.to_string())
}
