//! Withdraw blocked work and remove only its own execution from reuse history.
use rusqlite::{OptionalExtension, params};

use super::{
    Execution, Receipts, Request,
    receipts::{Error, update_request},
};

impl Receipts {
    /// Called after an owned machine task has acknowledged cancellation. Joined work
    /// belongs to its producer: withdrawing this request cannot stop another Run's task.
    pub(crate) async fn block_request(&self, request: &Request) -> Result<(), String> {
        let request = request.clone();
        self.connection.call(move |db| -> Result<(), Error> {
            let transaction = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            if let Some(id) = &request.execution_id {
                let data: Option<String> = transaction.query_row("SELECT data FROM executions WHERE id=?", [id], |row| row.get(0)).optional()?;
                if let Some(mut execution) = data.and_then(|data| super::unreadable::evidence::<Execution>("execution", &data))
                    && execution.provenance.request_id == request.id {
                    // Audit remains, but cancelled work cannot be reused or hold capacity.
                    execution.state = super::ExecutionState::Failed {
                        error: "Execution withdrawn: dependency is RED or ERROR.".into(),
                        code: Some(crate::types::FailureCode::Cancelled),
                        at: crate::broker::now(),
                    };
                    execution.provenance.completed_at = execution.completed_at();
                    transaction.execute("UPDATE executions SET status='ERROR',backend=NULL,completed_at=NULL,bytes=NULL,last_used=NULL,data=? WHERE id=?", params![serde_json::to_string(&execution)?, id])?;
                }
            }
            update_request(&transaction, &request)?;
            transaction.execute("UPDATE requests SET claimed_by=NULL,claimed_at=NULL WHERE id=?", [&request.id])?;
            transaction.commit()?;
            Ok(())
        }).await.map_err(|error| error.to_string())
    }
}
