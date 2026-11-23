//! IPC types for the library-owned review draft.
use artifactize_core::review;
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{Caller, FormView, Verdict, form::FieldValue, read};
use crate::{App, Failure, ids::RequestKey};

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReviewDraft {
    pub verdict: Option<Verdict>,
    /// Why, oldest first, independent of the verdict.
    pub reasons: Vec<String>,
    /// Whether each verdict can be submitted now, whichever is chosen.
    pub readiness: DraftReadiness,
    pub points: Vec<ReviewPoint>,
    pub green: FormView,
    pub red: FormView,
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
    Field {
        verdict: Verdict,
        name: String,
        value: FieldValue,
    },
    GroupField {
        verdict: Verdict,
        name: String,
        item: u32,
        field: String,
        value: FieldValue,
    },
    AddItem {
        verdict: Verdict,
        name: String,
    },
    RemoveItem {
        verdict: Verdict,
        name: String,
        item: u32,
    },
    /// Append one reason; empty text is refused.
    AddReason {
        text: String,
    },
    /// Replace reason `index`, refused if it no longer reads `previous`.
    EditReason {
        index: u32,
        previous: String,
        text: String,
    },
    /// Remove reason `index`, refused if it no longer reads `previous`.
    RemoveReason {
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
            DraftChange::Field {
                verdict,
                name,
                value,
            } => Self::Field {
                verdict: verdict.into(),
                name,
                value: value.into(),
            },
            DraftChange::GroupField {
                verdict,
                name,
                item,
                field,
                value,
            } => Self::GroupField {
                verdict: verdict.into(),
                name,
                item,
                field,
                value: value.into(),
            },
            DraftChange::AddItem { verdict, name } => Self::AddItem {
                verdict: verdict.into(),
                name,
            },
            DraftChange::RemoveItem {
                verdict,
                name,
                item,
            } => Self::RemoveItem {
                verdict: verdict.into(),
                name,
                item,
            },
            DraftChange::AddReason { text } => Self::AddReason { text },
            DraftChange::EditReason {
                index,
                previous,
                text,
            } => Self::EditReason {
                index,
                previous,
                text,
            },
            DraftChange::RemoveReason { index, previous } => Self::RemoveReason { index, previous },
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

fn view(
    request: &artifactize_core::store::Request,
    draft: review::Draft,
) -> Result<ReviewDraft, Failure> {
    let green = draft
        .owner_form(request, Verdict::Green.into())
        .map_err(Failure::Invalid)?;
    let red = draft
        .owner_form(request, Verdict::Red.into())
        .map_err(Failure::Invalid)?;
    let readiness = DraftReadiness {
        green: draft.readiness(request, Verdict::Green.into()).into(),
        red: draft.readiness(request, Verdict::Red.into()).into(),
    };
    Ok(ReviewDraft {
        verdict: draft.verdict.map(Into::into),
        reasons: draft.reasons,
        readiness,
        points: draft.points.into_iter().map(Into::into).collect(),
        green: super::form::project(Verdict::Green, &green),
        red: super::form::project(Verdict::Red, &red),
    })
}

/// Read the saved response and both verdict forms. A released review retains its draft.
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
    view(&request.request, draft)
}

/// Change one field, reason or the verdict of the current reviewer's saved draft. Empty
/// reason text is `invalid`; a reason another surface changed first is refused.
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
    view(&request.request, draft)
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
    view(&request.request, draft)
}

/// Submit the current reviewer's saved form fields.
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
