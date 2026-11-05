//! Human sign-offs whose repository folder is gone. Nothing can be reviewed, run or submitted
//! against a workspace that no longer exists, so such a sign-off stops waiting on its own: it
//! ends as ERROR (`REPOSITORY_REMOVED`). A shared Human execution lives on in the repositories
//! that remain: when its original request is the one that lost its folder, the newest request
//! still waiting in an existing repository takes over as the original.
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use rusqlite::params;

use super::{
    Execution, Receipts, Request,
    receipts::{Error, schema_initialized, update_request},
    run_control::{retire_execution, retire_request},
};
use crate::{
    config::Field,
    types::{ExecutionId, FailureCode, RequestId},
};

/// Why a sign-off of a removed repository ended, on the request and the execution.
const REMOVED: &str = "Its Run's repository folder no longer exists.";

/// End every waiting Human sign-off whose Run's repository folder no longer exists. Only a
/// read happens while every repository is present.
pub async fn retire_removed_signoffs(state: &Path) -> Result<(), String> {
    let removed = removed_repositories(state).await?;
    let Some(first) = removed.first() else {
        return Ok(());
    };
    // The removed repository is only the boundary the state is checked against.
    let receipts = Receipts::open(state, Path::new(first)).await?;
    let result = receipts.retire_removed(removed).await;
    let closed = receipts.close().await;
    result.and(closed)
}

/// The repositories, as the state saves them, of Runs with a waiting Human request whose
/// folder is certainly gone. A folder that cannot be checked counts as present.
async fn removed_repositories(state: &Path) -> Result<Vec<String>, String> {
    let state = crate::workspace::canonical_target(state).map_err(|e| e.to_string())?;
    let Some(connection) = super::requests::open_read(&state).await? else {
        return Ok(Vec::new());
    };
    let result = connection
        .call(|db| -> Result<Vec<String>, Error> {
            super::wait_when_busy(db)?;
            let transaction = db.transaction()?;
            if !schema_initialized(&transaction)? {
                return Ok(Vec::new());
            }
            let repos = transaction
                .prepare("SELECT DISTINCT r.repo FROM requests q JOIN runs r ON r.id=q.run_id WHERE q.status IN ('WAITING_HUMAN','DEFERRED')")?
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            transaction.commit()?;
            Ok(repos)
        })
        .await
        .map_err(|e| e.to_string());
    let repos = super::connection::finish(connection, result).await?;
    Ok(repos
        .into_iter()
        .filter(|repo| matches!(Path::new(repo).try_exists(), Ok(false)))
        .collect())
}

/// A waiting request of an execution that has one in a removed repository.
struct Waiter {
    request: Request,
    repo: PathBuf,
    /// Where its Run writes output, as the broker lays it out under the Run's state.
    run_dir: Option<PathBuf>,
    removed: bool,
}

/// What becomes of one waiting Human execution some of whose requests lost their repository.
enum Fate<'a> {
    /// No request waits in an existing repository: the execution ends with its requests.
    End,
    /// The requests of removed repositories end; `heir`, when the original was one of them,
    /// becomes the execution's original.
    Narrow {
        retired: Vec<&'a Waiter>,
        heir: Option<&'a Waiter>,
    },
}

fn fate<'a>(original: &RequestId, waiters: &'a [Waiter]) -> Fate<'a> {
    let (retired, living): (Vec<_>, Vec<_>) = waiters.iter().partition(|waiter| waiter.removed);
    if living.is_empty() {
        return Fate::End;
    }
    let heir = retired
        .iter()
        .any(|waiter| &waiter.request.id == original)
        .then(|| {
            living
                .into_iter()
                .max_by_key(|waiter| (waiter.request.created_at, &waiter.request.id))
        })
        .flatten();
    Fate::Narrow { retired, heir }
}

impl Receipts {
    async fn retire_removed(&self, removed: Vec<String>) -> Result<(), String> {
        self.connection
            .call(move |db| -> Result<(), Error> {
                let transaction =
                    db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                for (id, (mut execution, waiters)) in affected(&transaction, &removed)? {
                    match fate(&execution.provenance.request_id, &waiters) {
                        Fate::End => {
                            for waiter in &waiters {
                                unclaim(&transaction, &waiter.request.id)?;
                            }
                            retire_execution(
                                &transaction,
                                &id,
                                FailureCode::RepositoryRemoved,
                                REMOVED,
                            )?;
                        }
                        Fate::Narrow { retired, heir } => {
                            if let Some(heir) = heir {
                                hand_over(&transaction, &mut execution, &waiters, heir)?;
                            }
                            let at = crate::broker::now();
                            for waiter in retired {
                                unclaim(&transaction, &waiter.request.id)?;
                                retire_request(
                                    &transaction,
                                    waiter.request.clone(),
                                    FailureCode::RepositoryRemoved,
                                    REMOVED,
                                    at,
                                )?;
                            }
                        }
                    }
                }
                transaction.commit()?;
                Ok(())
            })
            .await
            .map_err(|e| e.to_string())
    }
}

/// A retired request's claim ends with it: a person cannot hold what no longer waits.
fn unclaim(db: &rusqlite::Connection, request: &RequestId) -> Result<(), Error> {
    db.execute(
        "UPDATE requests SET claimed_by=NULL,claimed_at=NULL WHERE id=?",
        [request],
    )?;
    Ok(())
}

/// Each waiting Human execution with a request in one of the `removed` repositories, with all
/// of its waiting requests. Unreadable records are left as they are.
fn affected(
    db: &rusqlite::Connection,
    removed: &[String],
) -> Result<BTreeMap<ExecutionId, (Execution, Vec<Waiter>)>, Error> {
    let removed_set: BTreeSet<&str> = removed.iter().map(String::as_str).collect();
    let mut statement = db.prepare(
        "SELECT q.data,r.repo,json_extract(r.data,'$.stateDir'),e.id,e.data FROM requests q JOIN runs r ON r.id=q.run_id JOIN executions e ON e.id=q.execution_id WHERE q.status IN ('WAITING_HUMAN','DEFERRED') AND e.status='WAITING_HUMAN' AND e.id IN (SELECT q.execution_id FROM requests q JOIN runs r ON r.id=q.run_id WHERE q.status IN ('WAITING_HUMAN','DEFERRED') AND r.repo IN (SELECT value FROM json_each(?1)))",
    )?;
    let rows = statement
        .query_map([serde_json::to_string(removed)?], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, ExecutionId>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut found: BTreeMap<ExecutionId, (Execution, Vec<Waiter>)> = BTreeMap::new();
    for (request, repo, state_dir, id, execution) in rows {
        let (Some(request), Some(execution)) = (
            super::unreadable::evidence::<Request>("request", &request),
            super::unreadable::evidence::<Execution>("execution", &execution),
        ) else {
            continue;
        };
        let removed = removed_set.contains(repo.as_str());
        let run_dir = state_dir.map(|state| {
            PathBuf::from(state)
                .join("runs")
                .join(request.run_id.as_str())
        });
        found
            .entry(id)
            .or_insert_with(|| (execution, Vec::new()))
            .1
            .push(Waiter {
                request,
                repo: PathBuf::from(repo),
                run_dir,
                removed,
            });
    }
    Ok(found)
}

/// Make `heir` the original of `execution`: it records the Human definition, now for its own
/// repository, rechecks fingerprints in its own Run's output directory and receives every
/// Human action.
fn hand_over(
    db: &rusqlite::Connection,
    execution: &mut Execution,
    waiters: &[Waiter],
    heir: &Waiter,
) -> Result<(), Error> {
    let definition = waiters
        .iter()
        .find(|waiter| waiter.request.id == execution.provenance.request_id)
        .and_then(|original| original.request.human_definition.clone());
    execution.provenance.request_id = heir.request.id.clone();
    execution.provenance.run_id = heir.request.run_id.clone();
    execution.provenance.repo_path = heir.repo.clone();
    db.execute(
        "UPDATE executions SET data=? WHERE id=? AND status='WAITING_HUMAN'",
        params![serde_json::to_string(&*execution)?, execution.id],
    )?;
    let mut request = heir.request.clone();
    request.human_definition = definition.map(|mut definition| {
        definition.repo = Field::Value(heir.repo.clone());
        definition
    });
    request.provenance = Some(execution.provenance.clone());
    request.run_dir = request.run_dir.or_else(|| heir.run_dir.clone());
    update_request(db, &request)
}
