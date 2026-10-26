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
    /// What the requester said it is working on (`verify --comment`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<crate::types::RunComment>,
    pub counts: BTreeMap<crate::types::RequestStatus, u64>,
    /// The Run's records that do not read, where the reader audited them (the monitor's
    /// catalog); a listing reads columns only and leaves this empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unreadable: Vec<super::Unreadable>,
}

impl RunSummary {
    /// Whether the Run's own record reads; a Run with only unreadable requests still opens.
    pub fn readable(&self) -> bool {
        !self.unreadable.iter().any(|record| record.kind == "run")
    }
}

/// A page of Runs with their saved request-state counts, in one read-only snapshot and from
/// columns alone: no record is decoded, so a page costs the same however large its Runs are.
/// `status` and `run show` report the records that do not read. No repository discovery.
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
    let result = connection
        .call(move |db| -> Result<_, Error> {
            super::wait_when_busy(db)?;
            let transaction = db.transaction()?;
            if !schema_initialized(&transaction)? {
                return Ok(Vec::new());
            }
            let runs = page(&transaction, &state, repos.as_deref(), limit, offset, None)?;
            transaction.commit()?;
            Ok(runs)
        })
        .await
        .map_err(|e| e.to_string());
    super::connection::finish(connection, result).await
}

/// A page of Runs with their counts, read from columns alone: no record of a Run is decoded.
/// `pinned` is listed too when it is outside the page.
pub(super) fn page(
    db: &rusqlite::Connection,
    state: &Path,
    repos: Option<&str>,
    limit: u32,
    offset: u32,
    pinned: Option<&crate::types::RunId>,
) -> Result<Vec<RunSummary>, Error> {
    let mut runs = listed(db, state, repos, limit, offset, pinned)?;
    count(db, &mut runs)?;
    Ok(runs)
}

fn listed(
    db: &rusqlite::Connection,
    state: &Path,
    repos: Option<&str>,
    limit: u32,
    offset: u32,
    pinned: Option<&crate::types::RunId>,
) -> Result<Vec<RunSummary>, Error> {
    let mut statement = db.prepare_cached(
        "WITH page AS (
         SELECT id,repo,json_extract(data,'$.createdAt') created,json_extract(data,'$.completedAt') completed,status,rowid serial,json_extract(data,'$.comment') comment
         FROM runs WHERE (?1 IS NULL OR repo IN (SELECT value FROM json_each(?1)))
         ORDER BY json_extract(data,'$.createdAt') DESC,rowid DESC LIMIT ?2 OFFSET ?3)
         SELECT id,repo,created,completed,status,0,serial,comment FROM page UNION ALL
         SELECT id,repo,json_extract(data,'$.createdAt'),json_extract(data,'$.completedAt'),status,1,rowid,json_extract(data,'$.comment') FROM runs
         WHERE id=?4 AND id NOT IN (SELECT id FROM page)
         AND (?1 IS NULL OR repo IN (SELECT value FROM json_each(?1))) ORDER BY 3 DESC,7 DESC",
    )?;
    let runs = statement
        .query_map(params![repos, limit, offset, pinned], |row| {
            Ok(RunSummary {
                id: row.get(0)?,
                repo_path: PathBuf::from(row.get::<_, String>(1)?),
                created_at: row.get(2)?,
                completed_at: row.get(3)?,
                status: row.get(4)?,
                // Only a comment that reads is shown; the Run's audit reports a record that
                // does not.
                comment: row
                    .get::<_, Option<String>>(7)?
                    .and_then(|comment| comment.parse().ok()),
                counts: BTreeMap::new(),
                unreadable: Vec::new(),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    crate::workspace::outside_workspaces(runs.iter().map(|run| run.repo_path.as_path()), state)
        .map_err(|error| Error::Invalid(error.to_string()))?;
    Ok(runs)
}

/// Fill in each Run's request counts by saved status, and the status they show it with.
fn count(db: &rusqlite::Connection, runs: &mut [RunSummary]) -> Result<(), Error> {
    let ids: Vec<_> = runs.iter().map(|run| run.id.clone()).collect();
    let mut counts: BTreeMap<crate::types::RunId, BTreeMap<crate::types::RequestStatus, u64>> =
        BTreeMap::new();
    let mut statement = db.prepare_cached(
        "SELECT run_id,status,count(*) FROM requests WHERE run_id IN (SELECT value FROM json_each(?1)) GROUP BY run_id,status",
    )?;
    let rows = statement.query_map([serde_json::to_string(&ids)?], |row| {
        Ok((
            row.get::<_, crate::types::RunId>(0)?,
            row.get::<_, crate::types::RequestStatus>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    for row in rows {
        let (run, status, count) = row?;
        let count = u64::try_from(count).expect("COUNT(*) is nonnegative");
        counts.entry(run).or_default().insert(status, count);
    }
    for run in runs {
        run.counts = counts.remove(&run.id).unwrap_or_default();
        run.status = run
            .status
            .with_signoffs(run.counts.keys().any(|status| status.is_human_wait()));
    }
    Ok(())
}
