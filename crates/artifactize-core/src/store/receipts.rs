//! Receipt writes and the public receipt interfaces.
pub(super) mod connection;
mod latest;
mod wire;

pub(crate) use connection::NotifyingConnection;
#[cfg(any(test, feature = "test-support"))]
pub use connection::OwnedReceipts;
pub(crate) use connection::regular_files;
pub use connection::{
    DATABASE, EARLIER_STATE, Receipts, read_state_id, schema_error, state_schema,
};
pub(super) use connection::{REVISIONS, check_files, schema_initialized};
pub use latest::{LastRequest, read_latest_requests};
use rusqlite::params;
pub use wire::{QueueReason, Request, Run, RunView, StoppedBackend};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Invalid(String),
    /// A saved record that does not decode: the Run it belongs to cannot be opened.
    #[error("{0}")]
    Unreadable(super::Unreadable),
}

impl Receipts {
    pub async fn create_run(&self, run: &Run, requests: &[Request]) -> Result<(), String> {
        for request in requests {
            if request.run_id != run.id {
                return Err("Request belongs to another Run.".into());
            }
        }
        let run = run.clone();
        let requests = requests.to_vec();
        self.connection
            .call(move |db| -> Result<(), Error> {
                let transaction = db.transaction()?;
                super::run_record::insert(&transaction, &run)?;
                for (ordinal, request) in requests.iter().enumerate() {
                    transaction.execute(
                        "INSERT INTO requests(id,run_id,eval_id,ordinal,status,data) VALUES (?,?,?,?,?,?)",
                        params![
                            request.id,
                            run.id,
                            request.eval_id,
                            ordinal as i64,
                            request.status(),
                            serde_json::to_string(request)?
                        ],
                    )?;
                }
                transaction.commit()?;
                Ok(())
            })
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn save_request(&self, request: &Request) -> Result<(), String> {
        let request = request.clone();
        self.connection
            .call(move |db| update_request(db, &request))
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn save_run(&self, run: &Run) -> Result<(), String> {
        let run = run.clone();
        self.connection
            .call(move |db| -> Result<(), Error> {
                let transaction =
                    db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                super::run_control::save(&transaction, run)?;
                transaction.commit()?;
                Ok(())
            })
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn finish(&self, run: &Run, requests: &[Request]) -> Result<(), String> {
        let run = run.clone();
        let requests = requests.to_vec();
        self.connection
            .call(move |db| -> Result<(), Error> {
                let transaction =
                    db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                for request in requests {
                    // A concurrent submission may already have settled this saved request.
                    if request.status().is_human_wait() {
                        continue;
                    }
                    transaction.execute("UPDATE requests SET status=?,data=?,execution_id=? WHERE id=? AND status NOT IN ('GREEN','RED','ERROR')", params![request.status(),serde_json::to_string(&request)?,request.execution_id,request.id])?;
                }
                super::run_control::save(&transaction, run)?;
                transaction.commit()?;
                Ok(())
            })
            .await
            .map_err(|e| e.to_string())
    }
}

pub(super) fn update_request(db: &rusqlite::Connection, request: &Request) -> Result<(), Error> {
    if db.execute(
        "UPDATE requests SET status=?,data=?,execution_id=? WHERE id=?",
        params![
            request.status(),
            serde_json::to_string(request)?,
            request.execution_id,
            request.id
        ],
    )? != 1
    {
        return Err(Error::Invalid("Review request not found.".into()));
    }
    Ok(())
}
