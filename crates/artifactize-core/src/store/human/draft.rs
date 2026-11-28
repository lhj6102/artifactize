use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use super::{Error, Receipts, Request, claimant, waiting};
use crate::{
    review::{Draft, Point},
    types::{RequestId, ReviewerId},
};

#[derive(serde::Deserialize)]
struct SavedDraft {
    #[serde(flatten)]
    draft: Draft,
    #[serde(default)]
    notes: String,
    /// What drafts saved before 0.27 called their issues.
    reasons: Option<Vec<String>>,
}

impl From<SavedDraft> for Draft {
    fn from(saved: SavedDraft) -> Self {
        let mut draft = saved.draft;
        if !saved.notes.is_empty() {
            draft.points.insert(0, Point::earlier_notes(saved.notes));
        }
        if let Some(reasons) = saved.reasons {
            draft.issues = reasons;
        }
        draft
    }
}

pub(super) fn read(
    db: &Connection,
    id: &RequestId,
    reviewer: &ReviewerId,
) -> Result<Option<Draft>, Error> {
    let data: Option<String> = db
        .query_row(
            "SELECT data FROM review_drafts WHERE request_id=? AND reviewer=?",
            params![id, reviewer],
            |row| row.get(0),
        )
        .optional()?;
    data.map(|data| {
        serde_json::from_str::<SavedDraft>(&data)
            .map(Draft::from)
            .map_err(Error::from)
    })
    .transpose()
}

pub(super) fn clear(db: &Connection, id: &RequestId) -> Result<(), Error> {
    db.execute("DELETE FROM review_drafts WHERE request_id=?", [id])?;
    Ok(())
}

impl Receipts {
    pub(crate) async fn read_review_draft(
        &self,
        id: &RequestId,
        reviewer: &ReviewerId,
    ) -> Result<Option<Draft>, String> {
        let id = id.clone();
        let reviewer = reviewer.clone();
        self.connection.call(move |db| {
            // Use the original request while the execution waits, as claim, release and
            // submission do. A released draft is still readable by the reviewer who wrote it.
            let transaction = db.transaction()?;
            let original: Option<RequestId> = transaction.query_row(
                "SELECT json_extract(e.data,'$.provenance.requestId') FROM requests q JOIN executions e ON e.id=q.execution_id WHERE q.id=? AND q.status IN ('WAITING_HUMAN','DEFERRED') AND e.status='WAITING_HUMAN' AND json_valid(e.data)",
                [&id],
                |row| row.get(0),
            ).optional()?.flatten();
            let draft = read(&transaction, original.as_ref().unwrap_or(&id), &reviewer)?;
            transaction.commit()?;
            Ok::<_, Error>(draft)
        }).await.map_err(|error| error.to_string())
    }

    pub(crate) async fn change_review_draft(
        &self,
        id: &RequestId,
        reviewer: &ReviewerId,
        change: impl FnOnce(&Request, &mut Draft) -> Result<(), String> + Send + 'static,
    ) -> Result<Draft, String> {
        let id = id.clone();
        let reviewer = reviewer.clone();
        self.connection.call(move |db| {
            let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let (request, _) = waiting(&transaction, &id)?;
            claimant(&transaction, &request.id, &reviewer)?;
            let mut draft = read(&transaction, &request.id, &reviewer)?.unwrap_or_default();
            change(&request, &mut draft).map_err(Error::Invalid)?;
            let data = serde_json::to_string(&draft)?;
            // Use the Human result's bound for partial input as well.
            if data.len() > crate::human::MAX_RESULT_BYTES {
                return Err(Error::Invalid(format!("Review draft exceeds {} bytes.", crate::human::MAX_RESULT_BYTES)));
            }
            transaction.execute(
                "INSERT INTO review_drafts(request_id,reviewer,data) VALUES (?,?,?) ON CONFLICT(request_id,reviewer) DO UPDATE SET data=excluded.data",
                params![request.id, reviewer, data],
            )?;
            transaction.commit()?;
            Ok(draft)
        }).await.map_err(|error| error.to_string())
    }
}
