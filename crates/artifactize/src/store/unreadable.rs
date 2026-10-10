//! Per-record corruption reporting, separate from valid lifecycle models.

use std::{collections::BTreeSet, path::Path};

use rusqlite::params;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use super::receipts::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
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
            let unreadable = Unreadable {
                kind: kind.into(),
                id,
                reason: error.to_string(),
            };
            use std::io::Write;
            let _ = writeln!(std::io::stderr().lock(), "{unreadable}");
            None
        }
    }
}

pub(super) fn in_run(
    db: &rusqlite::Connection,
    run: &crate::types::RunId,
) -> Result<Vec<Unreadable>, Error> {
    let mut statement = db.prepare("SELECT 'run',r.id,r.data FROM runs r WHERE r.id=?1 UNION SELECT 'request',q.id,q.data FROM requests q WHERE q.run_id=?1 UNION SELECT 'execution',e.id,e.data FROM executions e JOIN requests q ON q.execution_id=e.id WHERE q.run_id=?1 UNION SELECT 'execution',e.id,e.data FROM executions e WHERE json_extract(e.data,'$.provenance.runId')=?1")?;
    let records = statement.query_map([run], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    records
        .filter_map(|row| match row {
            Err(error) => Some(Err(error.into())),
            Ok((kind, id, data)) => {
                let error = if kind == "run" {
                    decode::<super::Run>("run", &id, &data).err()
                } else if kind == "request" {
                    decode::<super::Request>("request", &id, &data).err()
                } else {
                    decode::<super::Execution>("execution", &id, &data).err()
                };
                error.map(Ok)
            }
        })
        .collect()
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
    connection
        .call(move |db| -> Result<_, Error> {
            db.busy_timeout(super::SQLITE_BUSY_TIMEOUT)?;
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
            for run in runs {
                for error in in_run(&transaction, &run)? {
                    if seen.insert((error.kind.clone(), error.id.clone())) {
                        unreadable.push(error);
                    }
                }
            }
            Ok(unreadable)
        })
        .await
        .map_err(|error| error.to_string())
}
