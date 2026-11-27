//! IPC types for the library-owned review draft: a verdict, the issues and the Points.
use artifactize_core::review;
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{Caller, Verdict, read};
use crate::{App, Failure, ids::RequestKey};

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReviewDraft {
    pub verdict: Option<Verdict>,
    /// What must change, oldest first, as typed.
    pub issues: Vec<String>,
    /// Whether each verdict can be submitted now, whichever is chosen.
    pub readiness: DraftReadiness,
    pub points: Vec<ReviewPoint>,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct DraftReadiness {
    pub green: Readiness,
    pub red: Readiness,
}

/// Whether a verdict's answer can be submitted; `reason` is the library's sentence.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Readiness {
    Ready,
    Blocked { reason: String },
}

impl From<review::Readiness> for Readiness {
    fn from(readiness: review::Readiness) -> Self {
        match readiness {
            review::Readiness::Ready => Self::Ready,
            review::Readiness::Blocked(reason) => Self::Blocked { reason },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ReviewPoint {
    pub text: String,
    pub source: String,
    pub at: String,
}

impl From<review::Point> for ReviewPoint {
    fn from(point: review::Point) -> Self {
        Self {
            text: point.text,
            source: point.source,
            at: point.at.to_string(),
        }
    }
}

impl TryFrom<ReviewPoint> for review::Point {
    type Error = Failure;

    fn try_from(point: ReviewPoint) -> Result<Self, Failure> {
        Ok(Self {
            text: point.text,
            source: point.source,
            at: point.at.parse().map_err(Failure::Invalid)?,
        })
    }
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DraftChange {
    Verdict {
        verdict: Verdict,
    },
    /// Append one issue; empty text is refused.
    AddIssue {
        text: String,
    },
    /// Replace issue `index`, refused if it no longer reads `previous`.
    EditIssue {
        index: u32,
        previous: String,
        text: String,
    },
    /// Remove issue `index`, refused if it no longer reads `previous`.
    RemoveIssue {
        index: u32,
        previous: String,
    },
}

impl From<DraftChange> for review::DraftEdit {
    fn from(change: DraftChange) -> Self {
        match change {
            DraftChange::Verdict { verdict } => Self::Verdict {
                verdict: verdict.into(),
            },
            DraftChange::AddIssue { text } => Self::AddIssue { text },
            DraftChange::EditIssue {
                index,
                previous,
                text,
            } => Self::EditIssue {
                index,
                previous,
                text,
            },
            DraftChange::RemoveIssue { index, previous } => Self::RemoveIssue { index, previous },
        }
    }
}

/// A draft changed in this app, including in another window or through the viewer bridge.
/// Treat this as invalidation: re-read instead of applying possibly out-of-order snapshots.
#[derive(Debug, Clone, Serialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct HumanDraftChanged {
    pub id: RequestKey,
}

fn view(draft: review::Draft) -> ReviewDraft {
    let readiness = DraftReadiness {
        green: draft.readiness(Verdict::Green.into()).into(),
        red: draft.readiness(Verdict::Red.into()).into(),
    };
    ReviewDraft {
        verdict: draft.verdict.map(Into::into),
        issues: draft.issues,
        readiness,
        points: draft.points.into_iter().map(Into::into).collect(),
    }
}

/// Read the saved draft. A released review retains its draft.
#[tauri::command]
#[specta::specta]
pub async fn human_draft(
    app: tauri::State<'_, App>,
    id: RequestKey,
) -> Result<ReviewDraft, Failure> {
    let request = read(&app, &id.into_inner()).await?.1.owner;
    let draft = review::read_draft(&app.launch.state, &request.request.id, &app.launch.reviewer)
        .await
        .map_err(Failure::Read)?
        .unwrap_or_default();
    Ok(view(draft))
}

/// Change an issue or the verdict of the current reviewer's saved draft. Empty issue text is
/// `invalid`; an issue another surface changed first is refused.
#[tauri::command]
#[specta::specta]
pub async fn human_draft_edit(
    app: tauri::State<'_, App>,
    id: RequestKey,
    change: DraftChange,
) -> Result<ReviewDraft, Failure> {
    let change = review::DraftEdit::from(change);
    change.check().map_err(Failure::Invalid)?;
    let id = id.into_inner();
    let turn = super::draft_turns(&app, std::slice::from_ref(&id)).await?;
    let _claim = app.human.claim_gate.lock().await;
    let request = read(&app, &turn.owners[0]).await?.1.owner;
    let draft = review::edit_draft(
        &app.launch.state,
        &request.request.id,
        &app.launch.reviewer,
        change,
    )
    .await
    .map_err(Failure::Action)?;
    Ok(view(draft))
}

/// Remove a Point from the current reviewer's draft without submitting it.
#[tauri::command]
#[specta::specta]
pub async fn human_draft_remove_point(
    app: tauri::State<'_, App>,
    id: RequestKey,
    point: ReviewPoint,
) -> Result<ReviewDraft, Failure> {
    let id = id.into_inner();
    let turn = super::draft_turns(&app, std::slice::from_ref(&id)).await?;
    let _claim = app.human.claim_gate.lock().await;
    let request = read(&app, &turn.owners[0]).await?.1.owner;
    let draft = review::remove_point(
        &app.launch.state,
        &request.request.id,
        &app.launch.reviewer,
        point.try_into()?,
    )
    .await
    .map_err(Failure::Action)?;
    Ok(view(draft))
}

/// Submit the current reviewer's saved verdict and issues.
#[tauri::command]
#[specta::specta]
pub async fn human_draft_submit(
    app: tauri::State<'_, App>,
    caller: Caller,
    id: RequestKey,
    job: crate::ids::JobId,
) -> Result<super::Submitted, Failure> {
    let id = id.into_inner();
    let work = app
        .human
        .jobs
        .register(caller, job, super::jobs::Action::Finish)?;
    let turn = work
        .prepare(super::draft_turns(&app, std::slice::from_ref(&id)))
        .await?;
    let request = super::guard(&app, &turn.owners[0], review::review_refusal).await?;
    super::submit(
        &app,
        work,
        id,
        request.request.id.clone(),
        review::Answer::SavedDraft,
        turn,
    )
    .await
}
