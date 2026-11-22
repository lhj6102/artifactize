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
use serde::Deserialize;
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

impl CatalogRun {
    /// The Run's recorded branch, independent of the current checkout or any search filter.
    pub async fn branch(&self) -> Result<crate::monitor::live::Place, String> {
        let place = crate::monitor::live::Place::for_checkout;
        if self.repository.common_dir.is_some() && self.repository.worktree_path.is_some() {
            return Ok(place(&self.repo_path, &self.repository));
        }
        let repo = self.repo_path.clone();
        crate::task::blocking(move || place(&repo, &crate::repository::identify(&repo))).await
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Metadata {
    #[serde(with = "crate::platform::path_serde")]
    repo_path: PathBuf,
    #[serde(flatten)]
    repository: Identity,
    status: RunStatus,
    created_at: crate::types::Timestamp,
    completed_at: Option<crate::types::Timestamp>,
    error: Option<String>,
    comment: Option<crate::types::RunComment>,
}

impl Metadata {
    fn state(&mut self) -> Result<super::RunState, String> {
        super::RunState::from_parts(self.status, self.completed_at, self.error.take())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunPlace {
    #[serde(with = "crate::platform::path_serde")]
    pub repo_path: PathBuf,
    #[serde(flatten)]
    pub repository: Identity,
}

/// The saved repository identity of one Run, without decoding its definitions.
pub async fn run_checkout(
    state: &Path,
    run: &crate::types::RunId,
) -> Result<(PathBuf, Identity), String> {
    places(state, vec![run.clone()])
        .await?
        .into_iter()
        .next()
        .map(|(_, place)| (place.repo_path, place.repository))
        .ok_or_else(|| "Run's checkout was not found.".into())
}

/// Resolve the Run's recorded workspace. Older records did not retain Git metadata; in that
/// case use the live identity of the exact recorded directory, never a different worktree.
pub async fn viewer_checkout(
    state: &Path,
    run: &crate::types::RunId,
) -> Result<(PathBuf, Identity), String> {
    let (worktree, saved) = run_checkout(state, run).await?;
    if saved != Identity::default() {
        return Ok((worktree, saved));
    }
    crate::task::blocking(move || {
        let identity = crate::repository::identify(&worktree);
        (worktree, identity)
    })
    .await
}

/// Only the saved display identities of the named Runs. No Run states, attention counts or
/// definitions are returned to Review, which already has the request page.
pub(crate) async fn places(
    state: &Path,
    ids: Vec<crate::types::RunId>,
) -> Result<Vec<(crate::types::RunId, RunPlace)>, String> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let Some(connection) = super::requests::open_read(state).await? else {
        return Ok(Vec::new());
    };
    let result = connection.call(move |db| {
        super::wait_when_busy(db)?;
        let transaction = db.transaction()?;
        if !schema_initialized(&transaction)? { return Ok(Vec::new()); }
        let mut statement = transaction.prepare("SELECT id,CASE WHEN json_valid(data) THEN json_object(
            'repoPath',repo,'commonDir',json_extract(data,'$.commonDir'),
            'worktreePath',json_extract(data,'$.worktreePath'),'branch',json_extract(data,'$.branch')) END
            FROM runs WHERE id IN (SELECT value FROM json_each(?1))")?;
        let mut rows = statement.query([serde_json::to_string(&ids)?])?;
        let mut places = Vec::new();
        while let Some(row) = rows.next()? {
            let id: crate::types::RunId = row.get(0)?;
            let data = row.get_ref(1)?.as_str().unwrap_or("null");
            if let Ok(place) = decode("run", &id, data) { places.push((id, place)); }
        }
        Ok::<_, Error>(places)
    }).await.map_err(|error| error.to_string());
    super::connection::finish(connection, result).await
}

/// Read only open Run IDs, irrespective of which checkout or screen displays them.
pub async fn read_open_run_ids(state: &Path) -> Result<Vec<crate::types::RunId>, String> {
    let Some(connection) = super::requests::open_read(state).await? else {
        return Ok(Vec::new());
    };
    let result = connection.call(move |db| {
        super::wait_when_busy(db)?;
        let transaction = db.transaction()?;
        if !schema_initialized(&transaction)? { return Ok(Vec::new()); }
        let mut query = transaction.prepare("SELECT id FROM runs WHERE CASE WHEN json_valid(data) THEN json_extract(data,'$.status') IN (?1,?2) ELSE 0 END")?;
        let ids = query.query_map([RunStatus::Running, RunStatus::WaitingHuman], |row| row.get(0))?.collect::<Result<Vec<_>, _>>()?;
        Ok::<_, Error>(ids)
    }).await.map_err(|error| error.to_string());
    super::connection::finish(connection, result).await
}

/// Global attention facts; unreadable Runs are skipped individually with their reason.
pub async fn read_catalog(state: &Path) -> Result<Vec<CatalogRun>, String> {
    catalog(state, None).await
}

/// The small timeline record for a Run named by a state change hint.
pub async fn read_catalog_run(
    state: &Path,
    id: &crate::types::RunId,
) -> Result<Option<CatalogRun>, String> {
    Ok(catalog(state, Some(id.clone())).await?.into_iter().next())
}

async fn catalog(state: &Path, id: Option<crate::types::RunId>) -> Result<Vec<CatalogRun>, String> {
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
            let catalog = in_snapshot(&transaction, &state, id.as_ref().map(std::slice::from_ref))?;
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
    let ids = only.map(serde_json::to_string).transpose()?;
    let filtered = ids.as_ref();
    let predicate = if filtered.is_some() {
        " WHERE id IN (SELECT value FROM json_each(?1))"
    } else {
        ""
    };
    // Keep the SQL projection small. Full Run state contains validation and evidence that are
    // not part of the timeline, and collecting those blobs made every cold read expensive.
    let sql = format!("SELECT id,CASE WHEN json_valid(data) THEN json_object(
        'repoPath',repo,'status',json_extract(data,'$.status'),'createdAt',json_extract(data,'$.createdAt'),
        'completedAt',json_extract(data,'$.completedAt'),'comment',json_extract(data,'$.comment'),
        'error',CASE coalesce(json_type(data,'$.error'),'null') WHEN 'text' THEN '' WHEN 'null' THEN NULL ELSE 0 END,
        'commonDir',json_extract(data,'$.commonDir'),'worktreePath',json_extract(data,'$.worktreePath'),
        'branch',json_extract(data,'$.branch')) END FROM runs{predicate} ORDER BY rowid");
    let mut statement = db.prepare_cached(&sql)?;
    let mut rows = statement.query(rusqlite::params_from_iter(filtered))?;
    let mut catalog = std::collections::BTreeMap::new();
    let mut order = Vec::new();
    let mut unreadable = Vec::new();
    while let Some(row) = rows.next()? {
        let id: crate::types::RunId = row.get(0)?;
        let data = row.get_ref(1)?.as_str().unwrap_or("null");
        let metadata = decode::<Metadata>("run", &id, data).and_then(|mut run| {
            let state = run.state().map_err(|reason| Unreadable {
                kind: "run".into(),
                id: id.to_string(),
                reason,
            })?;
            Ok((run, state))
        });
        match metadata {
            Ok((run, lifecycle)) => {
                order.push(id.clone());
                catalog.insert(
                    id.clone(),
                    CatalogRun {
                        id,
                        created_at: run.created_at,
                        completed_at: lifecycle.completed_at(),
                        repo_path: run.repo_path,
                        repository: run.repository,
                        status: lifecycle.status(),
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
    let filter = if filtered.is_some() {
        " AND run_id IN (SELECT value FROM json_each(?1))"
    } else {
        ""
    };
    let sql = format!(
        "SELECT run_id,status,count(*) FROM requests WHERE status IN ('RED','ERROR'){filter} GROUP BY run_id,status"
    );
    let mut statement = db.prepare_cached(&sql)?;
    let counts = statement.query_map(rusqlite::params_from_iter(filtered), |row| {
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
    let filter = if filtered.is_some() {
        " AND q.run_id IN (SELECT value FROM json_each(?1))"
    } else {
        ""
    };
    let sql = format!("SELECT q.run_id,q.execution_id,coalesce(json_extract(e.data,'$.provenance.requestId'),json_extract(q.data,'$.provenance.requestId'),q.id)
        FROM requests q LEFT JOIN executions e ON e.id=q.execution_id WHERE q.status IN ('WAITING_HUMAN','DEFERRED'){filter} ORDER BY q.run_id,q.ordinal");
    let mut statement = db.prepare_cached(&sql)?;
    let waiting = statement.query_map(rusqlite::params_from_iter(filtered), |row| {
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
