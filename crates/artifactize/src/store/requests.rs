use std::path::Path;

use rusqlite::params;
use serde::Serialize;
use serde_json::{Value, json};
use tokio_rusqlite::Connection;

use super::{
    DATABASE, Execution, HumanClaim, Request,
    receipts::{Error, check_files, schema_initialized},
};
use crate::workspace::{canonical_target, outside_workspace};

#[derive(Debug, Clone, Serialize)]
pub struct RequestView {
    #[serde(flatten)]
    pub request: Request,
    pub claim: Option<HumanClaim>,
    pub execution: Option<Execution>,
    pub definition: Option<Value>,
}

#[derive(Default)]
struct Filter<'a> {
    run: Option<&'a str>,
    id: Option<&'a str>,
    repo: Option<&'a Path>,
    waiting: bool,
    session: Option<&'a str>,
}

pub async fn read_requests(state: &Path, run: Option<&str>) -> Result<Vec<RequestView>, String> {
    read(
        state,
        Filter {
            run,
            ..Filter::default()
        },
    )
    .await
}

pub async fn read_request(state: &Path, id: &str) -> Result<RequestView, String> {
    read(
        state,
        Filter {
            id: Some(id),
            ..Filter::default()
        },
    )
    .await?
    .pop()
    .ok_or_else(|| "Review request not found.".into())
}

/// The request whose Agent review ran as session `id`, if this state has it.
pub async fn read_session_request(state: &Path, id: &str) -> Result<Option<RequestView>, String> {
    Ok(read(
        state,
        Filter {
            session: Some(id),
            ..Filter::default()
        },
    )
    .await?
    .pop())
}

/// WAITING_HUMAN requests, newest Run first, from the canonical `repo`'s Runs or every Run.
pub async fn read_waiting(state: &Path, repo: Option<&Path>) -> Result<Vec<RequestView>, String> {
    read(
        state,
        Filter {
            repo,
            waiting: true,
            ..Filter::default()
        },
    )
    .await
}

async fn read(state: &Path, filter: Filter<'_>) -> Result<Vec<RequestView>, String> {
    let state = canonical_target(state).map_err(|e| e.to_string())?;
    let repo = filter
        .repo
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
    let run = filter
        .run
        .map(str::parse::<crate::types::RunId>)
        .transpose()?;
    let id = filter
        .id
        .map(str::parse::<crate::types::RequestId>)
        .transpose()?;
    let repo = repo.map(|repo| repo.to_string_lossy().into_owned());
    let waiting = filter.waiting;
    let session = filter
        .session
        .map(str::parse::<crate::types::SessionId>)
        .transpose()?;
    connection
        .call(move |db| -> Result<_, Error> {
            db.busy_timeout(super::SQLITE_BUSY_TIMEOUT)?;
            let transaction = db.transaction()?;
            if !schema_initialized(&transaction)? {
                return Ok(Vec::new());
            }
            let views = {
                // A follower shows the claim of the waiting request whose execution it follows.
                let mut statement = transaction.prepare(
                    "SELECT q.data,r.repo,e.data,h.id,h.claimed_by,h.claimed_at,
                json_extract(r.data,'$.definitions.artifacts'),
                (SELECT value FROM json_each(r.data,'$.definitions.evals') WHERE json_extract(value,'$.id')=q.eval_id)
                FROM requests q JOIN runs r ON r.id=q.run_id
                LEFT JOIN executions e ON e.id=q.execution_id
                LEFT JOIN requests h ON h.id=json_extract(e.data,'$.provenance.requestId') AND h.claimed_by IS NOT NULL
                WHERE (?1 IS NULL OR q.run_id=?1) AND (?2 IS NULL OR q.id=?2)
                AND (?3 IS NULL OR r.repo=?3) AND (NOT ?4 OR q.status='WAITING_HUMAN')
                AND (?5 IS NULL OR json_extract(q.data,'$.sessionId')=?5)
                ORDER BY r.rowid DESC,q.ordinal",
                )?;
                statement
                    .query_map(params![run, id, repo, waiting, session], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<String>>(2)?,
                            row.get::<_, Option<crate::types::RequestId>>(3)?,
                            row.get::<_, Option<String>>(4)?,
                            row.get::<_, Option<String>>(5)?,
                            row.get::<_, Option<String>>(6)?,
                            row.get::<_, Option<String>>(7)?,
                        ))
                    })?
                    .map(|row| {
                        let (
                            data,
                            repo,
                            execution,
                            request_id,
                            reviewer,
                            claimed_at,
                            artifacts,
                            eval,
                        ) = row?;
                        outside_workspace(Path::new(&repo), &state)
                            .map_err(|e| Error::Invalid(e.to_string()))?;
                        let request: Request = serde_json::from_str(&data)?;
                        let artifacts: Option<Value> = artifacts
                            .map(|data| serde_json::from_str(&data))
                            .transpose()?;
                        let eval: Option<Value> =
                            eval.map(|data| serde_json::from_str(&data)).transpose()?;
                        let definition = eval
                            .zip(
                                artifacts
                                    .as_ref()
                                    .and_then(|artifacts| artifacts.get(&request.target)),
                            )
                            .map(|(eval, artifact)| json!({"eval":eval,"artifact":artifact}));
                        Ok(RequestView {
                            request,
                            definition,
                            claim: request_id.zip(reviewer).zip(claimed_at).map(
                                |((request_id, reviewer), claimed_at)| HumanClaim {
                                    request_id,
                                    reviewer,
                                    claimed_at,
                                },
                            ),
                            execution: execution
                                .map(|data| serde_json::from_str(&data))
                                .transpose()?,
                        })
                    })
                    .collect::<Result<Vec<_>, Error>>()?
            };
            transaction.commit()?;
            Ok(views)
        })
        .await
        .map_err(|e| e.to_string())
}
