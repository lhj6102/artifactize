//! Global monitor catalog, intentionally independent of any visible Run page.
use super::{
    DATABASE, Unreadable,
    receipts::{Error, check_files, schema_initialized},
    unreadable::decode,
};
use crate::{
    repository::Identity,
    types::{ExecutionId, RequestId, RequestStatus, RunStatus},
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
    pub id: crate::types::RunId,
    pub created_at: crate::types::Timestamp,
    pub completed_at: Option<crate::types::Timestamp>,
    pub repo_path: PathBuf,
    pub repository: Identity,
    pub status: RunStatus,
    /// What the requester said it is working on (`verify --comment`).
    pub comment: Option<crate::types::RunComment>,
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
    let result = connection
        .call(move |db| -> Result<_, Error> {
            super::wait_when_busy(db)?;
            let transaction = db.transaction()?;
            if !schema_initialized(&transaction)? {
                return Ok(Vec::new());
            }
            let catalog = in_snapshot(&transaction, &state, None)?;
            transaction.commit()?;
            Ok(catalog.runs)
        })
        .await
        .map_err(|error| error.to_string());
    super::connection::finish(connection, result).await
}

/// The saved Runs, oldest first, and the Run records that could not be read.
pub(crate) struct Catalog {
    pub runs: Vec<CatalogRun>,
    pub unreadable: Vec<Unreadable>,
}

/// Read every saved Run's state, or only the state of the Runs `only` names, never their
/// definitions: they live in their own row, so a Run's state is small however many Artifacts
/// it has.
pub(super) fn in_snapshot(
    db: &rusqlite::Connection,
    state: &Path,
    only: Option<&[crate::types::RunId]>,
) -> Result<Catalog, Error> {
    let only = only.map(serde_json::to_string).transpose()?;
    let mut statement = db.prepare_cached(
        "SELECT id,data FROM runs WHERE ?1 IS NULL OR id IN (SELECT value FROM json_each(?1)) ORDER BY rowid",
    )?;
    let rows = statement
        .query_map([&only], |row| {
            Ok((
                row.get::<_, crate::types::RunId>(0)?,
                row.get::<_, String>(1)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut catalog = std::collections::BTreeMap::new();
    let mut order = Vec::new();
    let mut unreadable = Vec::new();
    for (id, data) in rows {
        match decode::<super::Run>("run", &id, &data) {
            Ok(run) => {
                let status = run.status();
                order.push(id.clone());
                catalog.insert(
                    id,
                    CatalogRun {
                        id: run.id.clone(),
                        created_at: run.created_at,
                        completed_at: run.completed_at(),
                        repo_path: run.repo_path,
                        repository: run.repository,
                        status,
                        comment: run.comment,
                        red: 0,
                        error: 0,
                        waiting: Vec::new(),
                    },
                );
            }
            Err(error) => unreadable.push(error),
        }
    }
    outside_workspaces(catalog.values().map(|run| run.repo_path.as_path()), state)
        .map_err(|error| Error::Invalid(error.to_string()))?;
    // Counts come from the status column alone; only a sign-off still waiting reads its records.
    let mut statement = db.prepare_cached(
        "SELECT run_id,status,count(*) FROM requests WHERE status IN ('RED','ERROR')
        AND (?1 IS NULL OR run_id IN (SELECT value FROM json_each(?1))) GROUP BY run_id,status",
    )?;
    let counts = statement.query_map([&only], |row| {
        Ok((
            row.get::<_, crate::types::RunId>(0)?,
            row.get::<_, RequestStatus>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    for row in counts {
        let (id, status, count) = row?;
        let count = u64::try_from(count).expect("COUNT(*) is nonnegative");
        if let Some(run) = catalog.get_mut(&id) {
            match status {
                RequestStatus::Red => run.red = count,
                RequestStatus::Error => run.error = count,
                _ => unreachable!("count query filters states"),
            }
        }
    }
    let mut statement = db.prepare_cached(
        "SELECT q.run_id,q.execution_id,coalesce(json_extract(e.data,'$.provenance.requestId'),json_extract(q.data,'$.provenance.requestId'),q.id)
        FROM requests q LEFT JOIN executions e ON e.id=q.execution_id WHERE q.status IN ('WAITING_HUMAN','DEFERRED')
        AND (?1 IS NULL OR q.run_id IN (SELECT value FROM json_each(?1))) ORDER BY q.run_id,q.ordinal",
    )?;
    let waiting = statement.query_map([&only], |row| {
        Ok((
            row.get::<_, crate::types::RunId>(0)?,
            row.get::<_, Option<ExecutionId>>(1)?,
            row.get::<_, RequestId>(2)?,
        ))
    })?;
    for row in waiting {
        let (id, execution, request) = row?;
        // A deferred sign-off waits for a person as well, after its Run has ended too.
        if let Some(run) = catalog.get_mut(&id) {
            run.waiting
                .push(execution.map_or(Signoff::Request(request), Signoff::Execution));
        }
    }
    for run in catalog.values_mut() {
        run.status = run.status.with_signoffs(!run.waiting.is_empty());
    }
    Ok(Catalog {
        runs: order
            .into_iter()
            .filter_map(|id| catalog.remove(&id))
            .collect(),
        unreadable,
    })
}
