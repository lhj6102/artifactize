use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::{
    Execution, Receipts, Request, executions,
    receipts::{Error, update_request},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HumanClaim {
    pub request_id: String,
    pub reviewer: String,
    pub claimed_at: String,
}

fn waiting(db: &rusqlite::Connection, id: &str) -> Result<(Request, Execution), Error> {
    let data: Option<String> = db.query_row(
        "SELECT e.data FROM requests q JOIN executions e ON e.id=q.execution_id WHERE q.id=? AND q.status='WAITING_HUMAN' AND e.status='WAITING_HUMAN'",
        [id], |row| row.get(0),
    ).optional()?;
    let execution: Execution = serde_json::from_str(
        &data.ok_or_else(|| Error::Invalid("Request is not waiting for a Human review.".into()))?,
    )?;
    let data: String = db.query_row(
        "SELECT data FROM requests WHERE id=? AND status='WAITING_HUMAN'",
        [&execution.provenance.request_id],
        |row| row.get(0),
    )?;
    let request: Request = serde_json::from_str(&data)?;
    if request.profile["kind"] != "human" || request.human_definition.is_none() {
        return Err(Error::Invalid(
            "Request has no recorded Human definition.".into(),
        ));
    }
    Ok((request, execution))
}

fn claim(db: &rusqlite::Connection, request: &str) -> Result<HumanClaim, Error> {
    Ok(db.query_row(
        "SELECT request_id,reviewer,claimed_at FROM human_claims WHERE request_id=?",
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

fn claimant(db: &rusqlite::Connection, request: &str, reviewer: &str) -> Result<(), Error> {
    let owner: Option<String> = db
        .query_row(
            "SELECT reviewer FROM human_claims WHERE request_id=?",
            [request],
            |row| row.get(0),
        )
        .optional()?;
    if owner.as_deref() != Some(reviewer) {
        return Err(Error::Invalid(
            "Only the Human claimant may perform this action.".into(),
        ));
    }
    Ok(())
}

impl Receipts {
    pub(crate) async fn settled_human_requests(
        &self,
        ids: Vec<String>,
    ) -> Result<Vec<Request>, String> {
        self.connection.call(move |db| -> Result<_, Error> {
            let mut statement = db.prepare("SELECT data FROM requests WHERE id IN (SELECT value FROM json_each(?)) AND status!='WAITING_HUMAN'")?;
            statement.query_map([serde_json::to_string(&ids)?], |row| row.get::<_, String>(0))?
                .map(|row| Ok(serde_json::from_str(&row?)?)).collect()
        }).await.map_err(|e| e.to_string())
    }

    pub(crate) async fn wait_for_human(
        &self,
        execution: &Execution,
        request: &Request,
    ) -> Result<(), String> {
        let execution = execution.clone();
        let request = request.clone();
        self.connection.call(move |db| -> Result<(), Error> {
            let transaction = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let data = serde_json::to_string(&execution)?;
            if execution.key.is_some() && !request.force {
                if transaction.execute("UPDATE executions SET status='WAITING_HUMAN',data=? WHERE id=? AND owner_pid=? AND owner_start_time=? AND status='RUNNING'", params![data, execution.id, execution.owner_pid, execution.owner_start_time as i64])? != 1 {
                    return Err(Error::Invalid("Active execution not found.".into()));
                }
            } else {
                // Unclaimed: a forced or unkeyed wait never holds its key's active slot.
                transaction.execute("INSERT INTO executions(id,key,owner_pid,owner_start_time,status,data) VALUES (?,NULL,?,?,'WAITING_HUMAN',?)", params![execution.id, execution.owner_pid, execution.owner_start_time as i64, data])?;
            }
            update_request(&transaction, &request)?;
            transaction.commit()?;
            Ok(())
        }).await.map_err(|e| e.to_string())
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
                let execution: Execution = serde_json::from_str(&data)?;
                if execution.status != "WAITING_HUMAN" {
                    receive(&mut request, &execution);
                }
                update_request(&transaction, &request)?;
                transaction.commit()?;
                Ok(request)
            })
            .await
            .map_err(|e| e.to_string())
    }

    pub(crate) async fn claim_human(&self, id: &str, reviewer: &str) -> Result<HumanClaim, String> {
        let id = id.to_owned();
        let reviewer = reviewer.to_owned();
        self.connection.call(move |db| -> Result<_, Error> {
            let transaction = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let (request, _) = waiting(&transaction, &id)?;
            transaction.execute("INSERT INTO human_claims(request_id,reviewer,claimed_at) VALUES (?,?,?) ON CONFLICT(request_id) DO NOTHING", params![request.id, reviewer, crate::broker::now()])?;
            claimant(&transaction, &request.id, &reviewer)?;
            let claim = claim(&transaction, &request.id)?;
            transaction.commit()?;
            Ok(claim)
        }).await.map_err(|e| e.to_string())
    }

    /// Only the claimant releases, and only while the original request still waits.
    pub(crate) async fn release_human(
        &self,
        id: &str,
        reviewer: &str,
    ) -> Result<HumanClaim, String> {
        let id = id.to_owned();
        let reviewer = reviewer.to_owned();
        self.connection
            .call(move |db| -> Result<_, Error> {
                let transaction =
                    db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                let (request, _) = waiting(&transaction, &id)?;
                claimant(&transaction, &request.id, &reviewer)?;
                let claim = claim(&transaction, &request.id)?;
                transaction
                    .execute("DELETE FROM human_claims WHERE request_id=?", [&request.id])?;
                transaction.commit()?;
                Ok(claim)
            })
            .await
            .map_err(|e| e.to_string())
    }

    /// The original waiting request that a request or follower forwards Human actions to.
    pub(crate) async fn waiting_human(&self, id: &str) -> Result<Request, String> {
        let id = id.to_owned();
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
        reviewer: &str,
    ) -> Result<(Request, Execution), String> {
        let id = id.to_owned();
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

    pub(crate) async fn record_human_tool(
        &self,
        id: &str,
        reviewer: &str,
        call: serde_json::Value,
    ) -> Result<(), String> {
        let id = id.to_owned();
        let reviewer = reviewer.to_owned();
        self.connection
            .call(move |db| -> Result<_, Error> {
                let transaction =
                    db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                let (mut request, mut execution) = waiting(&transaction, &id)?;
                claimant(&transaction, &request.id, &reviewer)?;
                request.tool_calls.push(call);
                execution.tool_calls = request.tool_calls.clone();
                transaction.execute(
                    "UPDATE executions SET data=? WHERE id=?",
                    params![serde_json::to_string(&execution)?, execution.id],
                )?;
                update_request(&transaction, &request)?;
                transaction.commit()?;
                Ok(())
            })
            .await
            .map_err(|e| e.to_string())
    }

    pub(crate) async fn settle_human(
        &self,
        request: &Request,
        reviewer: &str,
    ) -> Result<Request, String> {
        let mut request = request.clone();
        let reviewer = reviewer.to_owned();
        self.connection
            .call(move |db| -> Result<_, Error> {
                let transaction =
                    db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                let (current, mut execution) = waiting(&transaction, &request.id)?;
                claimant(&transaction, &current.id, &reviewer)?;
                request.tool_calls = current.tool_calls;
                request.completed_at = Some(crate::broker::now());
                request.blocked_reason = None;
                execution.status = request.status.clone();
                execution.result = request.result.clone();
                execution.error = request.error.clone();
                execution.error_code = request.error_code.clone();
                execution.reviewer = Some(reviewer.clone());
                execution.completed_at = request.completed_at.clone();
                execution.provenance.completed_at = request.completed_at.clone();
                request.provenance = Some(execution.provenance.clone());
                let published = executions::settle(&transaction, &execution, &request)?;
                let followers = {
                    let mut statement = transaction.prepare(
                        "SELECT data FROM requests WHERE execution_id=? AND status='WAITING_HUMAN'",
                    )?;
                    statement
                        .query_map([&execution.id], |row| row.get::<_, String>(0))?
                        .map(|row| Ok(serde_json::from_str(&row?)?))
                        .collect::<Result<Vec<Request>, Error>>()?
                };
                for mut follower in followers {
                    receive(&mut follower, &execution);
                    update_request(&transaction, &follower)?;
                }
                transaction
                    .execute("DELETE FROM human_claims WHERE request_id=?", [&request.id])?;
                transaction.commit()?;
                if published && let Err(error) = super::cache_entries::collect(db) {
                    use std::io::Write;
                    let _ = writeln!(std::io::stderr().lock(), "Cache GC failed: {error}");
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
    let mut execution: Execution = serde_json::from_str(&waiting)?;
    let now = crate::broker::now();
    execution.status = "ERROR".into();
    execution.error = Some(format!(
        "Superseded by the completed result {} for this key.",
        entry.id
    ));
    execution.error_code = Some("SUPERSEDED".into());
    execution.completed_at = Some(now.clone());
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
            .map(|row| Ok(serde_json::from_str(&row?)?))
            .collect::<Result<Vec<Request>, Error>>()?
    };
    for mut follower in followers {
        receive(&mut follower, entry);
        update_request(db, &follower)?;
        db.execute(
            "DELETE FROM human_claims WHERE request_id=?",
            [&follower.id],
        )?;
    }
    Ok(())
}

fn receive(request: &mut Request, execution: &Execution) {
    crate::cache::reuse(request, execution, crate::broker::now());
    request.error = execution.error.clone();
    request.error_code = execution.error_code.clone();
}
