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
    pub notes: String,
    pub green: FormView,
    pub red: FormView,
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
    AddItem {
        verdict: Verdict,
        name: String,
    },
    RemoveItem {
        verdict: Verdict,
        name: String,
        item: u32,
    },
    Notes {
        previous: String,
        text: String,
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
            DraftChange::Notes { previous, text } => Self::Notes { previous, text },
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
        .form(request, Verdict::Green.into())
        .map_err(Failure::Invalid)?;
    let red = draft
        .form(request, Verdict::Red.into())
        .map_err(Failure::Invalid)?;
    Ok(ReviewDraft {
        verdict: draft.verdict.map(Into::into),
        notes: draft.notes,
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

/// Change one field, verdict or the common notes of the current reviewer's saved draft.
#[tauri::command]
#[specta::specta]
pub async fn human_draft_edit(
    app: tauri::State<'_, App>,
    id: RequestKey,
    change: DraftChange,
) -> Result<ReviewDraft, Failure> {
    let request = read(&app, &id.into_inner()).await?.1.owner;
    let draft = review::edit_draft(
        &app.launch.state,
        &request.request.id,
        &app.launch.reviewer,
        change.into(),
    )
    .await
    .map_err(Failure::Action)?;
    view(&request.request, draft)
}

/// Append a line to the current reviewer's draft without submitting it.
#[tauri::command]
#[specta::specta]
pub async fn human_draft_append(
    app: tauri::State<'_, App>,
    id: RequestKey,
    text: String,
) -> Result<ReviewDraft, Failure> {
    let request = read(&app, &id.into_inner()).await?.1.owner;
    let draft = review::append_draft(
        &app.launch.state,
        &request.request.id,
        &app.launch.reviewer,
        &text,
    )
    .await
    .map_err(Failure::Action)?;
    view(&request.request, draft)
}

/// Submit the current reviewer's saved fields and notes.
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
    let request = super::guard(&app, &id, review::review_refusal).await?;
    super::submit(
        &app,
        work,
        id,
        request.request.id.clone(),
        review::Answer::SavedDraft,
    )
    .await
}
