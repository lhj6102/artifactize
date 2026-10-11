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
            for error in in_runs(&transaction, &runs)?.into_values().flatten() {
                if seen.insert((error.kind.clone(), error.id.clone())) {
                    unreadable.push(error);
                }
            }
            Ok(unreadable)
        })
        .await
        .map_err(|error| error.to_string())
}

/// Audit a bounded page in one query, including executions produced by it but no longer linked.
pub(super) fn in_runs(
    db: &rusqlite::Connection,
    runs: &[crate::types::RunId],
) -> Result<std::collections::BTreeMap<crate::types::RunId, Vec<Unreadable>>, Error> {
    in_runs_cached(db, runs, &mut Audit::default())
}

#[derive(Default)]
pub(super) struct Audit {
    records: std::collections::BTreeMap<(String, String), (String, Option<Unreadable>)>,
    pub selected: Option<crate::types::RunId>,
    parsed: std::collections::BTreeMap<(String, String), Record>,
}

/// Only the selected Run's freshly decoded records survive its page audit; the selected loader
/// consumes them, avoiding a second decode without caching every page's rich request bodies.
enum Record {
    Run(Box<super::Run>),
    Request(Box<super::Request>),
    Execution(Box<super::Execution>),
}
impl Record {
    fn decode(kind: &str, id: &str, data: &str) -> Result<Self, Unreadable> {
        match kind {
            "run" => decode("run", id, data).map(|run| Self::Run(Box::new(run))),
            "request" => {
                decode("request", id, data).map(|request| Self::Request(Box::new(request)))
            }
            _ => {
                decode("execution", id, data).map(|execution| Self::Execution(Box::new(execution)))
            }
        }
    }
}
impl Audit {
    fn take(&mut self, kind: &str, id: &str, data: &str) -> Option<Record> {
        let key = (kind.to_owned(), id.to_owned());
        (self
            .records
            .get(&key)
            .is_some_and(|(saved, _)| saved == data))
        .then(|| self.parsed.remove(&key))
        .flatten()
    }
    pub fn take_run(&mut self, id: &crate::types::RunId, data: &str) -> Option<super::Run> {
        match self.take("run", id, data)? {
            Record::Run(run) => Some(*run),
            _ => unreachable!("Run audit key"),
        }
    }
    pub fn take_request(
        &mut self,
        id: &crate::types::RequestId,
        data: &str,
    ) -> Option<super::Request> {
        match self.take("request", id, data)? {
            Record::Request(request) => Some(*request),
            _ => unreachable!("request audit key"),
        }
    }
    pub fn take_execution(
        &mut self,
        id: &crate::types::ExecutionId,
        data: &str,
    ) -> Option<super::Execution> {
        match self.take("execution", id, data)? {
            Record::Execution(execution) => Some(*execution),
            _ => unreachable!("execution audit key"),
        }
    }
}

pub(super) fn in_runs_cached(
    db: &rusqlite::Connection,
    runs: &[crate::types::RunId],
    audit: &mut Audit,
) -> Result<std::collections::BTreeMap<crate::types::RunId, Vec<Unreadable>>, Error> {
    let mut statement = db.prepare_cached(
        "WITH page(id) AS (SELECT value FROM json_each(?1))
        SELECT r.id,'run',r.id,r.data FROM runs r JOIN page p ON p.id=r.id
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
    audit.parsed.clear();
    let mut unreadable: std::collections::BTreeMap<_, Vec<_>> = std::collections::BTreeMap::new();
    let mut retained = std::collections::BTreeMap::new();
    let mut seen = BTreeSet::new();
    for record in records {
        let (run, kind, id, data) = record?;
        if !seen.insert((run.clone(), kind.clone(), id.clone())) {
            continue;
        }
        let key = (kind.clone(), id.clone());
        let error = if let Some((saved, error)) =
            audit.records.get(&key).or_else(|| retained.get(&key))
            && saved == &data
        {
            error.clone()
        } else {
            match Record::decode(&kind, &id, &data) {
                Ok(record) => {
                    if audit.selected.as_ref() == Some(&run) {
                        audit.parsed.insert(key.clone(), record);
                    }
                    None
                }
                Err(error) => Some(error),
            }
        };
        retained.insert(key, (data, error.clone()));
        if let Some(error) = error {
            unreadable.entry(run).or_default().push(error);
        }
    }
    for records in unreadable.values_mut() {
        records.sort_by(|a, b| (&a.kind, &a.id).cmp(&(&b.kind, &b.id)));
    }
    audit.records = retained;
    Ok(unreadable)
}
