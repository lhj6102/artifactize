use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::{
    Execution, Receipts, Request, executions,
    receipts::{Error, update_request},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HumanClaim {
    pub request_id: crate::types::RequestId,
    pub reviewer: crate::types::ReviewerId,
    pub claimed_at: crate::types::Timestamp,
}

fn waiting(db: &rusqlite::Connection, id: &str) -> Result<(Request, Execution), Error> {
    let data: Option<String> = db
        .query_row(
            "SELECT e.data FROM requests q JOIN executions e ON e.id=q.execution_id WHERE q.id=? AND q.status='WAITING_HUMAN' AND e.status='WAITING_HUMAN'",
            [id],
            |row| row.get(0),
        )
        .optional()?;
    let data =
        data.ok_or_else(|| Error::Invalid("Request is not waiting for a Human review.".into()))?;
    let execution: Execution = super::unreadable::evidence("execution", &data)
        .ok_or_else(|| Error::Invalid(format!("Request {id} has an unreadable execution.")))?;
    let data: String = db.query_row(
        "SELECT data FROM requests WHERE id=? AND status='WAITING_HUMAN'",
        [&execution.provenance.request_id],
        |row| row.get(0),
    )?;
    let request: Request =
        super::unreadable::decode("request", execution.provenance.request_id.as_str(), &data)
            .map_err(|error| Error::Invalid(error.to_string()))?;
    if request.profile.kind() != crate::config::ProfileKind::Human
        || request.human_definition.is_none()
    {
        return Err(Error::Invalid(
            "Request has no recorded Human definition.".into(),
        ));
    }
    Ok((request, execution))
}

fn claim(db: &rusqlite::Connection, request: &str) -> Result<HumanClaim, Error> {
    Ok(db.query_row(
        "SELECT id,claimed_by,claimed_at FROM requests WHERE id=? AND claimed_by IS NOT NULL",
        [request],
        |row| {
            Ok(HumanClaim {
                request_id: row.get(0)?,
                reviewer: row.get(1)?,
                claimed_at: row.get(2)?,
            })
        },
    )?)
}

/// Clear a request's Human claim.
fn unclaim(db: &rusqlite::Connection, request: &str) -> Result<(), Error> {
    db.execute(
        "UPDATE requests SET claimed_by=NULL,claimed_at=NULL WHERE id=?",
        [request],
    )?;
    Ok(())
}

fn claimant(
    db: &rusqlite::Connection,
    request: &str,
    reviewer: &crate::types::ReviewerId,
) -> Result<(), Error> {
    let owner: Option<crate::types::ReviewerId> = db
        .query_row(
            "SELECT claimed_by FROM requests WHERE id=?",
            [request],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    if owner.as_ref() != Some(reviewer) {
        return Err(Error::Invalid(
            "Only the Human claimant may perform this action.".into(),
        ));
    }
    Ok(())
}

impl Receipts {
    pub(crate) async fn settled_human_requests(
        &self,
        ids: Vec<crate::types::RequestId>,
    ) -> Result<Vec<Request>, String> {
        self.connection
            .call(move |db| -> Result<_, Error> {
                let mut statement = db.prepare(
                    "SELECT data FROM requests WHERE id IN (SELECT value FROM json_each(?)) AND status!='WAITING_HUMAN'",
                )?;
                statement
                    .query_map([serde_json::to_string(&ids)?], |row| {
                        row.get::<_, String>(0)
                    })?
                    .filter_map(|row| match row {
                    Ok(data) => super::unreadable::evidence("request", &data).map(Ok),
                    Err(error) => Some(Err(Error::Sql(error))),
                })
                    .collect()
            })
            .await
            .map_err(|e| e.to_string())
    }

    pub(crate) async fn wait_for_human(
        &self,
        execution: &Execution,
        request: &Request,
    ) -> Result<(), String> {
        let execution = execution.clone();
        let request = request.clone();
        self.connection
            .call(move |db| -> Result<(), Error> {
                let transaction =
                    db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                let data = serde_json::to_string(&execution)?;
                if execution.key.is_some() && !request.force {
                    if transaction.execute(
                        "UPDATE executions SET status='WAITING_HUMAN',data=? WHERE id=? AND owner_pid=? AND owner_start_time=? AND status='RUNNING'",
                        params![
                            data,
                            execution.id,
                            execution.owner_pid,
                            execution.owner_start_time as i64
                        ],
                    )? != 1
                    {
                        return Err(Error::Invalid("Active execution not found.".into()));
                    }
                } else {
                    // Unclaimed: a forced or unkeyed wait never holds its key's active slot.
                    transaction.execute(
                        "INSERT INTO executions(id,key,eval_def_hash,status,owner_pid,owner_start_time,data) VALUES (?,NULL,?,'WAITING_HUMAN',?,?,?)",
                        params![
                            execution.id,
                            execution.eval_def_hash,
                            execution.owner_pid,
                            execution.owner_start_time as i64,
                            data
                        ],
                    )?;
                }
                update_request(&transaction, &request)?;
                transaction.commit()?;
                Ok(())
            })
            .await
            .map_err(|e| e.to_string())
    }

    pub(crate) async fn follow_human(&self, request: &Request) -> Result<Request, String> {
        let mut request = request.clone();
        self.connection
            .call(move |db| -> Result<_, Error> {
                let transaction =
                    db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                let data: String = transaction.query_row(
                    "SELECT data FROM executions WHERE id=? AND key=?",
                    params![request.execution_id, request.key],
                    |row| row.get(0),
                )?;
                let Some(execution) = super::unreadable::evidence::<Execution>("execution", &data)
                else {
                    request.execution_id = None;
                    request.state =
                        super::RequestState::pending(crate::types::RequestStatus::Queued, None)
                            .expect("queued state");
                    update_request(&transaction, &request)?;
                    transaction.commit()?;
                    return Ok(request);
                };
                if execution.status() != crate::types::ExecutionStatus::WaitingHuman {
                    receive(&mut request, &execution);
                }
                update_request(&transaction, &request)?;
                transaction.commit()?;
                Ok(request)
            })
            .await
            .map_err(|e| e.to_string())
    }

    pub(crate) async fn claim_human(
        &self,
        id: &str,
        reviewer: &crate::types::ReviewerId,
    ) -> Result<HumanClaim, String> {
        let id: crate::types::RequestId = id.parse()?;
        let reviewer = reviewer.to_owned();
        self.connection
            .call(move |db| -> Result<_, Error> {
                let transaction =
                    db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                let (request, _) = waiting(&transaction, &id)?;
                transaction.execute(
                    "UPDATE requests SET claimed_by=?,claimed_at=? WHERE id=? AND claimed_by IS NULL",
                    params![reviewer, crate::broker::now(), request.id],
                )?;
                claimant(&transaction, &request.id, &reviewer)?;
                let claim = claim(&transaction, &request.id)?;
                transaction.commit()?;
                Ok(claim)
            })
            .await
            .map_err(|e| e.to_string())
    }

    /// Only the claimant releases, and only while the original request still waits.
    pub(crate) async fn release_human(
        &self,
        id: &str,
        reviewer: &crate::types::ReviewerId,
    ) -> Result<HumanClaim, String> {
        let id: crate::types::RequestId = id.parse()?;
        let reviewer = reviewer.to_owned();
        self.connection
            .call(move |db| -> Result<_, Error> {
                let transaction =
                    db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                let (request, _) = waiting(&transaction, &id)?;
                claimant(&transaction, &request.id, &reviewer)?;
                let claim = claim(&transaction, &request.id)?;
                unclaim(&transaction, &request.id)?;
                transaction.commit()?;
                Ok(claim)
            })
            .await
            .map_err(|e| e.to_string())
    }

    /// The original waiting request that a request or follower forwards Human actions to.
    pub(crate) async fn waiting_human(&self, id: &str) -> Result<Request, String> {
        let id: crate::types::RequestId = id.parse()?;
        self.connection
            .call(move |db| -> Result<_, Error> {
                let transaction = db.transaction()?;
                let (request, _) = waiting(&transaction, &id)?;
                transaction.commit()?;
                Ok(request)
            })
            .await
            .map_err(|e| e.to_string())
    }

    pub(crate) async fn human_request(
        &self,
        id: &str,
        reviewer: &crate::types::ReviewerId,
    ) -> Result<(Request, Execution), String> {
        let id: crate::types::RequestId = id.parse()?;
        let reviewer = reviewer.to_owned();
        self.connection
            .call(move |db| -> Result<_, Error> {
                let transaction = db.transaction()?;
                let (request, execution) = waiting(&transaction, &id)?;
                claimant(&transaction, &request.id, &reviewer)?;
                transaction.commit()?;
                Ok((request, execution))
            })
            .await
            .map_err(|e| e.to_string())
    }

    pub(crate) async fn settle_human(
        &self,
        request: &Request,
        reviewer: &crate::types::ReviewerId,
    ) -> Result<Request, String> {
        let mut request = request.clone();
        let reviewer = reviewer.to_owned();
        self.connection
            .call(move |db| -> Result<_, Error> {
                let transaction =
                    db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                let (current, mut execution) = waiting(&transaction, &request.id)?;
                claimant(&transaction, &current.id, &reviewer)?;
                request.state = match request.state {
                    super::RequestState::Completed { result, .. } => {
                        super::RequestState::completed(result, crate::broker::now())
                    }
                    super::RequestState::Failed { error, code, .. } => {
                        super::RequestState::failed(error, code, crate::broker::now())
                    }
                    _ => {
                        return Err(Error::Invalid(
                            "A Human submission needs a result or error.".into(),
                        ));
                    }
                };
                request.queue = None;
                request.blocked_reason = None;
                execution.state = request.state.clone().try_into().map_err(Error::Invalid)?;
                execution.reviewer = Some(reviewer.clone());
                execution.provenance.completed_at = request.completed_at();
                request.provenance = Some(execution.provenance.clone());
                let published = executions::settle(&transaction, &execution, &request)?;
                let followers = {
                    let mut statement = transaction.prepare(
                        "SELECT data FROM requests WHERE execution_id=? AND status='WAITING_HUMAN'",
                    )?;
                    statement
                        .query_map([&execution.id], |row| row.get::<_, String>(0))?
                        .filter_map(|row| match row {
                            Ok(data) => super::unreadable::evidence("request", &data).map(Ok),
                            Err(error) => Some(Err(Error::Sql(error))),
                        })
                        .collect::<Result<Vec<Request>, Error>>()?
                };
                for mut follower in followers {
                    receive(&mut follower, &execution);
                    update_request(&transaction, &follower)?;
                }
                unclaim(&transaction, &request.id)?;
                transaction.commit()?;
                if published {
                    super::history::collect_after(db);
                }
                Ok(request)
            })
            .await
            .map_err(|e| e.to_string())
    }
}

/// A completed record for the key settles a local Human wait, as a submission settles
/// followers; the never-reviewed waiting execution becomes ERROR (SUPERSEDED).
pub(super) fn settle_waiting(db: &rusqlite::Connection, entry: &Execution) -> Result<(), Error> {
    let waiting: Option<String> = db
        .query_row(
            "SELECT data FROM executions WHERE key=? AND status='WAITING_HUMAN'",
            [&entry.key],
            |row| row.get(0),
        )
        .optional()?;
    let Some(waiting) = waiting else {
        return Ok(());
    };
    let Some(mut execution) = super::unreadable::evidence::<Execution>("execution", &waiting)
    else {
        return Ok(());
    };
    let now = crate::broker::now();
    execution.state = super::ExecutionState::Failed {
        error: format!(
            "Superseded by the completed result {} for this key.",
            entry.id
        ),
        code: Some(crate::types::FailureCode::Superseded),
        at: now,
    };
    execution.provenance.completed_at = Some(now);
    db.execute(
        "UPDATE executions SET status='ERROR',data=? WHERE id=? AND status='WAITING_HUMAN'",
        params![serde_json::to_string(&execution)?, execution.id],
    )?;
    let followers = {
        let mut statement = db
            .prepare("SELECT data FROM requests WHERE execution_id=? AND status='WAITING_HUMAN'")?;
        statement
            .query_map([&execution.id], |row| row.get::<_, String>(0))?
            .filter_map(|row| match row {
                Ok(data) => super::unreadable::evidence("request", &data).map(Ok),
                Err(error) => Some(Err(Error::Sql(error))),
            })
            .collect::<Result<Vec<Request>, Error>>()?
    };
    for mut follower in followers {
        receive(&mut follower, entry);
        update_request(db, &follower)?;
        unclaim(db, &follower.id)?;
    }
    Ok(())
}

fn receive(request: &mut Request, execution: &Execution) {
    crate::cache::reuse(request, execution, crate::broker::now());
}
