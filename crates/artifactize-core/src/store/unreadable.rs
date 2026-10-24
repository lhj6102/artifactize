//! Per-record corruption reporting, separate from valid lifecycle models.

use std::{collections::BTreeSet, path::Path};

use rusqlite::params;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use super::receipts::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Unreadable {
    pub kind: String,
    pub id: String,
    pub reason: String,
}

impl std::fmt::Display for Unreadable {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(out, "Unreadable {} {}: {}", self.kind, self.id, self.reason)
    }
}

pub(super) fn decode<T: DeserializeOwned>(
    kind: &'static str,
    id: &str,
    data: &str,
) -> Result<T, Unreadable> {
    serde_json::from_str(data).map_err(|error| Unreadable {
        kind: kind.into(),
        id: id.to_owned(),
        reason: error.to_string(),
    })
}

/// Reads used only for evidence skip one bad record and explain which record was skipped.
pub(super) fn evidence<T: DeserializeOwned>(kind: &'static str, data: &str) -> Option<T> {
    match serde_json::from_str(data) {
        Ok(record) => Some(record),
        Err(error) => {
            // Decode the identity only on failure, not the whole payload a second time.
            #[derive(Deserialize)]
            struct Identity {
                id: String,
            }
            let id = serde_json::from_str::<Identity>(data)
                .map(|identity| identity.id)
                .unwrap_or_else(|_| "(unknown id)".into());
            skipped(&Unreadable {
                kind: kind.into(),
                id,
                reason: error.to_string(),
            });
            None
        }
    }
}

/// Explain a record a read skipped.
pub(super) fn skipped(unreadable: &Unreadable) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr().lock(), "{unreadable}");
}

pub(super) fn in_run(
    db: &rusqlite::Connection,
    run: &crate::types::RunId,
) -> Result<Vec<Unreadable>, Error> {
    Ok(in_runs(db, std::slice::from_ref(run))?
        .remove(run)
        .unwrap_or_default())
}

/// Existing-state audit for current-input status and the monitor, without rejecting a listing.
pub async fn read_unreadable(state: &Path, repo: Option<&Path>) -> Result<Vec<Unreadable>, String> {
    super::receipts::check_files(state)?;
    if !state
        .join(super::DATABASE)
        .try_exists()
        .map_err(|error| error.to_string())?
    {
        return Ok(Vec::new());
    }
    let connection = tokio_rusqlite::Connection::open_with_flags(
        state.join(super::DATABASE),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .await
    .map_err(|error| error.to_string())?;
    let repo = repo.map(crate::platform::path_text);
    let result = connection
        .call(move |db| -> Result<_, Error> {
            super::wait_when_busy(db)?;
            let transaction = db.transaction()?;
            if !super::receipts::schema_initialized(&transaction)? {
                return Ok(Vec::new());
            }
            let mut statement =
                transaction.prepare("SELECT id FROM runs WHERE ?1 IS NULL OR repo=?1")?;
            let runs = statement
                .query_map(params![repo], |row| row.get::<_, crate::types::RunId>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            let mut unreadable = Vec::new();
            let mut seen = BTreeSet::new();
            for error in in_runs(&transaction, &runs)?.into_values().flatten() {
                if seen.insert((error.kind.clone(), error.id.clone())) {
                    unreadable.push(error);
                }
            }
            Ok(unreadable)
        })
        .await
        .map_err(|error| error.to_string());
    super::connection::finish(connection, result).await
}

/// Audit the given Runs in one query: their state and definitions rows, their requests and
/// executions, including executions produced by them but no longer linked. Every record is
/// decoded once; nothing is kept between audits.
pub(super) fn in_runs(
    db: &rusqlite::Connection,
    runs: &[crate::types::RunId],
) -> Result<std::collections::BTreeMap<crate::types::RunId, Vec<Unreadable>>, Error> {
    let mut statement = db.prepare_cached(
        "WITH page(id) AS (SELECT value FROM json_each(?1))
        SELECT r.id,'run',r.id,r.data FROM runs r JOIN page p ON p.id=r.id
        UNION ALL SELECT d.run_id,'definitions',d.run_id,d.data FROM run_definitions d JOIN page p ON p.id=d.run_id
        UNION ALL SELECT q.run_id,'request',q.id,q.data FROM requests q JOIN page p ON p.id=q.run_id
        UNION ALL SELECT q.run_id,'execution',e.id,e.data FROM (SELECT DISTINCT run_id,execution_id FROM requests WHERE run_id IN (SELECT id FROM page) AND execution_id IS NOT NULL) q JOIN executions e ON e.id=q.execution_id
        UNION ALL SELECT json_extract(e.data,'$.provenance.runId'),'execution',e.id,e.data FROM executions e WHERE json_extract(e.data,'$.provenance.runId') IN (SELECT id FROM page)",
    )?;
    let records = statement.query_map([serde_json::to_string(runs)?], |row| {
        Ok((
            row.get::<_, crate::types::RunId>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    let mut unreadable: std::collections::BTreeMap<_, Vec<_>> = std::collections::BTreeMap::new();
    let mut seen = BTreeSet::new();
    for record in records {
        let (run, kind, id, data) = record?;
        if !seen.insert((run.clone(), kind.clone(), id.clone())) {
            continue;
        }
        let error = match kind.as_str() {
            "run" => decode::<super::Run>("run", &id, &data).err(),
            // A Run whose definitions do not read is an unreadable Run.
            "definitions" => decode::<super::definitions::Definitions>("run", &id, &data).err(),
            "request" => decode::<super::Request>("request", &id, &data).err(),
            _ => decode::<super::Execution>("execution", &id, &data).err(),
        };
        if let Some(error) = error {
            unreadable.entry(run).or_default().push(error);
        }
    }
    for records in unreadable.values_mut() {
        records.sort_by(|a, b| (&a.kind, &a.id).cmp(&(&b.kind, &b.id)));
    }
    Ok(unreadable)
}
