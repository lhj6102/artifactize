use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use rusqlite::params;
use serde::Serialize;
use tokio_rusqlite::Connection;

use super::{
    DATABASE,
    receipts::{Error, check_files, schema_initialized},
};
use crate::workspace::{canonical_target, outside_workspace};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub id: crate::types::RunId,
    #[serde(serialize_with = "crate::platform::path_serde::serialize")]
    pub repo_path: PathBuf,
    pub created_at: crate::types::Timestamp,
    pub completed_at: Option<crate::types::Timestamp>,
    pub status: crate::types::RunStatus,
    pub counts: BTreeMap<crate::types::RequestStatus, u64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unreadable: Vec<super::Unreadable>,
}

/// Saved request-state counts in one read-only snapshot; no repository discovery.
pub async fn read_runs(
    state: &Path,
    repo: Option<&Path>,
    limit: u32,
    offset: u32,
) -> Result<Vec<RunSummary>, String> {
    let repos = repo.map(|path| vec![path.to_path_buf()]);
    read_scoped_runs(state, repos.as_deref(), limit, offset).await
}

/// Filter workspace paths before sorting and paging; an empty scope has no Runs.
pub async fn read_scoped_runs(
    state: &Path,
    repos: Option<&[PathBuf]>,
    limit: u32,
    offset: u32,
) -> Result<Vec<RunSummary>, String> {
    let state = canonical_target(state).map_err(|e| e.to_string())?;
    let repos = repos
        .map(|paths| {
            paths
                .iter()
                .map(|path| {
                    let path = canonical_target(path).map_err(|error| error.to_string())?;
                    outside_workspace(&path, &state).map_err(|error| error.to_string())?;
                    Ok(crate::platform::path_text(&path))
                })
                .collect::<Result<Vec<_>, String>>()
        })
        .transpose()?;
    let repos = repos
        .map(|paths| serde_json::to_string(&paths))
        .transpose()
        .map_err(|error| error.to_string())?;
    check_files(&state)?;
    if !state
        .join(DATABASE)
        .try_exists()
        .map_err(|e| e.to_string())?
    {
        return Ok(Vec::new());
    }
    let connection = Connection::open_with_flags(
        state.join(DATABASE),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .await
    .map_err(|e| e.to_string())?;
    connection
        .call(move |db| -> Result<_, Error> {
            db.busy_timeout(super::SQLITE_BUSY_TIMEOUT)?;
            let transaction = db.transaction()?;
            if !schema_initialized(&transaction)? {
                return Ok(Vec::new());
            }
            let runs = in_snapshot(&transaction, &state, repos.as_deref(), limit, offset)?;
            transaction.commit()?;
            Ok(runs)
        })
        .await
        .map_err(|e| e.to_string())
}

pub(super) fn in_snapshot(
    db: &rusqlite::Connection,
    state: &Path,
    repos: Option<&str>,
    limit: u32,
    offset: u32,
) -> Result<Vec<RunSummary>, Error> {
    in_snapshot_cached(
        db,
        state,
        repos,
        limit,
        offset,
        None,
        &mut super::unreadable::Audit::default(),
    )
}

pub(super) fn in_snapshot_cached(
    db: &rusqlite::Connection,
    state: &Path,
    repos: Option<&str>,
    limit: u32,
    offset: u32,
    pinned: Option<&crate::types::RunId>,
    audit: &mut super::unreadable::Audit,
) -> Result<Vec<RunSummary>, Error> {
    let mut runs = {
        let mut statement = db.prepare_cached(
                    "WITH page AS (
                 SELECT id,repo,json_extract(data,'$.createdAt') created,json_extract(data,'$.completedAt') completed,status,rowid serial
                 FROM runs WHERE (?1 IS NULL OR repo IN (SELECT value FROM json_each(?1)))
                 ORDER BY json_extract(data,'$.createdAt') DESC,rowid DESC LIMIT ?2 OFFSET ?3)
                 SELECT id,repo,created,completed,status,0,serial FROM page UNION ALL
                 SELECT id,repo,json_extract(data,'$.createdAt'),json_extract(data,'$.completedAt'),status,1,rowid FROM runs
                 WHERE id=?4 AND id NOT IN (SELECT id FROM page)
                 AND (?1 IS NULL OR repo IN (SELECT value FROM json_each(?1))) ORDER BY 3 DESC,7 DESC",
                )?;
        statement
            .query_map(params![repos, limit, offset, pinned], |row| {
                Ok(RunSummary {
                    id: row.get(0)?,
                    repo_path: PathBuf::from(row.get::<_, String>(1)?),
                    created_at: row.get(2)?,
                    completed_at: row.get(3)?,
                    status: row.get(4)?,
                    counts: BTreeMap::new(),
                    unreadable: Vec::new(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    audit.selected = pinned
        .cloned()
        .filter(|id| runs.iter().any(|run| &run.id == id))
        .or_else(|| runs.first().map(|run| run.id.clone()));
    let ids: Vec<_> = runs.iter().map(|run| run.id.clone()).collect();
    let mut unreadable = super::unreadable::in_runs_cached(db, &ids, audit)?;
    let invalid: Vec<_> = unreadable
        .values()
        .flatten()
        .filter(|record| record.kind == "request")
        .map(|record| &record.id)
        .collect();
    let mut counts: BTreeMap<crate::types::RunId, BTreeMap<crate::types::RequestStatus, u64>> =
        BTreeMap::new();
    let mut statement = db.prepare_cached(
        "SELECT run_id,status,count(*) FROM requests WHERE run_id IN (SELECT value FROM json_each(?1)) AND id NOT IN (SELECT value FROM json_each(?2)) GROUP BY run_id,status",
    )?;
    let rows = statement.query_map(
        params![
            serde_json::to_string(&ids)?,
            serde_json::to_string(&invalid)?
        ],
        |row| {
            Ok((
                row.get::<_, crate::types::RunId>(0)?,
                row.get::<_, crate::types::RequestStatus>(1)?,
                row.get::<_, i64>(2)? as u64,
            ))
        },
    )?;
    for row in rows {
        let (run, status, count) = row?;
        counts.entry(run).or_default().insert(status, count);
    }
    crate::workspace::outside_workspaces(runs.iter().map(|run| run.repo_path.as_path()), state)
        .map_err(|error| Error::Invalid(error.to_string()))?;
    for run in &mut runs {
        run.unreadable = unreadable.remove(&run.id).unwrap_or_default();
        run.counts = counts.remove(&run.id).unwrap_or_default();
    }
    Ok(runs)
}
