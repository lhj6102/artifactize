use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
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
    pub id: String,
    pub repo_path: PathBuf,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub status: String,
    pub counts: BTreeMap<String, u64>,
}

/// Saved request-state counts in one read-only snapshot; no repository discovery.
pub async fn read_runs(
    state: &Path,
    repo: Option<&Path>,
    limit: u32,
    offset: u32,
) -> Result<Vec<RunSummary>, String> {
    let state = canonical_target(state).map_err(|e| e.to_string())?;
    let repo = repo
        .map(canonical_target)
        .transpose()
        .map_err(|e| e.to_string())?;
    if let Some(repo) = &repo {
        outside_workspace(repo, &state).map_err(|e| e.to_string())?;
    }
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
    connection.call(move |db| -> Result<_, Error> {
        db.busy_timeout(Duration::from_secs(5))?;
        let transaction = db.transaction()?;
        if !schema_initialized(&transaction)? {
            return Ok(Vec::new());
        }
        let mut runs = {
            let mut statement = transaction.prepare(
                "SELECT id,repo,json_extract(data,'$.createdAt'),json_extract(data,'$.completedAt'),status
                 FROM runs WHERE (?1 IS NULL OR repo=?1)
                 ORDER BY json_extract(data,'$.createdAt') DESC,rowid DESC LIMIT ?2 OFFSET ?3",
            )?;
            statement.query_map(params![repo.as_ref().map(|path| path.to_string_lossy()), limit, offset], |row| {
                Ok(RunSummary {
                    id: row.get(0)?,
                    repo_path: PathBuf::from(row.get::<_, String>(1)?),
                    created_at: row.get(2)?,
                    completed_at: row.get(3)?,
                    status: row.get(4)?,
                    counts: BTreeMap::new(),
                })
            })?.collect::<Result<Vec<_>, _>>()?
        };
        {
            let mut statement = transaction.prepare(
                "SELECT status,count(*) FROM requests WHERE run_id=? GROUP BY status",
            )?;
            for run in &mut runs {
                outside_workspace(&run.repo_path, &state).map_err(|e| Error::Invalid(e.to_string()))?;
                run.counts = statement.query_map([&run.id], |row| Ok((row.get(0)?, row.get::<_, i64>(1)? as u64)))?
                    .collect::<Result<_, _>>()?;
            }
        }
        transaction.commit()?;
        Ok(runs)
    }).await.map_err(|e| e.to_string())
}
