use std::{path::Path, time::Duration};

use rusqlite::params;
use serde::Serialize;
use serde_json::{Value, json};
use tokio_rusqlite::Connection;

use super::{
    DATABASE, Execution, HumanClaim, Request,
    receipts::{Error, check_files, schema_initialized},
};
use crate::workspace::{canonical_target, outside_workspace};

#[derive(Debug, Serialize)]
pub struct RequestView {
    #[serde(flatten)]
    pub request: Request,
    pub claim: Option<HumanClaim>,
    pub execution: Option<Execution>,
    pub definition: Option<Value>,
}

pub async fn read_requests(state: &Path, run: Option<&str>) -> Result<Vec<RequestView>, String> {
    read(state, run, None).await
}

pub async fn read_request(state: &Path, id: &str) -> Result<RequestView, String> {
    read(state, None, Some(id))
        .await?
        .pop()
        .ok_or_else(|| "Review request not found.".into())
}

async fn read(
    state: &Path,
    run: Option<&str>,
    id: Option<&str>,
) -> Result<Vec<RequestView>, String> {
    let state = canonical_target(state).map_err(|e| e.to_string())?;
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
    let run = run.map(str::to_owned);
    let id = id.map(str::to_owned);
    connection.call(move |db| -> Result<_, Error> {
        db.busy_timeout(Duration::from_secs(5))?;
        let transaction = db.transaction()?;
        if !schema_initialized(&transaction)? {
            return Ok(Vec::new());
        }
        let views = {
            let mut statement = transaction.prepare("SELECT q.data,r.repo,e.data,h.request_id,h.reviewer,h.claimed_at,
                json_extract(r.data,'$.definitions.artifacts'),
                (SELECT value FROM json_each(r.data,'$.definitions.evals') WHERE json_extract(value,'$.id')=m.eval_id)
                FROM requests q JOIN runs r ON r.id=q.run_id
                JOIN run_members m ON m.request_id=q.id
                LEFT JOIN executions e ON e.id=q.execution_id
                LEFT JOIN human_claims h ON h.request_id=json_extract(e.data,'$.provenance.requestId')
                WHERE (?1 IS NULL OR q.run_id=?1) AND (?2 IS NULL OR q.id=?2)
                ORDER BY r.rowid DESC,m.ordinal")?;
            statement.query_map(params![run, id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?, row.get::<_, Option<String>>(4)?, row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?, row.get::<_, Option<String>>(7)?))
            })?.map(|row| {
                let (data, repo, execution, request_id, reviewer, claimed_at, artifacts, eval) = row?;
                outside_workspace(Path::new(&repo), &state).map_err(|e| Error::Invalid(e.to_string()))?;
                let mut request: Request = serde_json::from_str(&data)?;
                request.tool_calls = super::tool_calls::project(&transaction, request.execution_id.as_deref(), &request.tool_calls)?;
                let artifacts: Option<Value> = artifacts.map(|data| serde_json::from_str(&data)).transpose()?;
                let eval: Option<Value> = eval.map(|data| serde_json::from_str(&data)).transpose()?;
                let definition = eval.zip(artifacts.as_ref().and_then(|artifacts| artifacts.get(&request.target)))
                    .map(|(eval, artifact)| json!({"eval":eval,"artifact":artifact}));
                Ok(RequestView {
                    request,
                    definition,
                    claim: request_id.zip(reviewer).zip(claimed_at).map(|((request_id, reviewer), claimed_at)| HumanClaim {request_id, reviewer, claimed_at}),
                    execution: execution.map(|data| serde_json::from_str(&data)).transpose()?,
                })
            }).collect::<Result<Vec<_>, Error>>()?
        };
        transaction.commit()?;
        Ok(views)
    }).await.map_err(|e| e.to_string())
}
