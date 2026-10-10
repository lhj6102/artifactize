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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub id: crate::types::RunId,
    #[serde(serialize_with = "crate::platform::path_serde::serialize")]
    pub repo_path: PathBuf,
    pub created_at: crate::types::Timestamp,
    pub completed_at: Option<crate::types::Timestamp>,
    pub status: crate::types::RunStatus,
    pub counts: BTreeMap<crate::types::RequestStatus, u64>,
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
            let mut runs = {
                let mut statement = transaction.prepare(
                    "SELECT id,repo,json_extract(data,'$.createdAt'),json_extract(data,'$.completedAt'),status
                 FROM runs WHERE (?1 IS NULL OR repo IN (SELECT value FROM json_each(?1)))
                 ORDER BY json_extract(data,'$.createdAt') DESC,rowid DESC LIMIT ?2 OFFSET ?3",
                )?;
                statement
                    .query_map(params![repos, limit, offset], |row| {
                        Ok(RunSummary {
                            id: row.get(0)?,
                            repo_path: PathBuf::from(row.get::<_, String>(1)?),
                            created_at: row.get(2)?,
                            completed_at: row.get(3)?,
                            status: row.get(4)?,
                            counts: BTreeMap::new(),
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?
            };
            {
                let mut statement = transaction.prepare(
                    "SELECT status,count(*) FROM requests WHERE run_id=? GROUP BY status",
                )?;
                for run in &mut runs {
                    outside_workspace(&run.repo_path, &state)
                        .map_err(|e| Error::Invalid(e.to_string()))?;
                    run.counts = statement
                        .query_map([&run.id], |row| {
                            Ok((row.get(0)?, row.get::<_, i64>(1)? as u64))
                        })?
                        .collect::<Result<_, _>>()?;
                }
            }
            transaction.commit()?;
            Ok(runs)
        })
        .await
        .map_err(|e| e.to_string())
}
