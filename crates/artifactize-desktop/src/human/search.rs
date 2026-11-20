//! Review workspace reads, independent of the Verify workspace's browse filters.
use artifactize_core::{monitor, review::search};
use serde::Serialize;
use specta::Type;

use super::HumanView;
use crate::{
    App, Failure,
    browse::{Completion, PathKey},
    ids::{NodeKey, RequestKey, RunKey},
    status::Mark,
};

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReviewTarget {
    pub run: RunKey,
    pub node: NodeKey,
}

impl From<artifactize_core::review::Target> for ReviewTarget {
    fn from(target: artifactize_core::review::Target) -> Self {
        Self {
            run: target.run.into(),
            node: target.node.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReviewMatch {
    pub id: RequestKey,
    pub target: ReviewTarget,
    pub title: String,
    pub eval: String,
    pub worktree: PathKey,
    pub project: String,
    pub branch: String,
    pub age: String,
    pub mark: Mark,
    pub claimant: Option<String>,
    pub mine: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReviewPage {
    pub reviews: Vec<ReviewMatch>,
    pub next: Option<u32>,
}

/// A page of Review results; `start` is the `next` cursor of the previous page of this query.
#[tauri::command]
#[specta::specta]
pub async fn human_search(
    app: tauri::State<'_, App>,
    query: String,
    start: Option<u32>,
) -> Result<ReviewPage, Failure> {
    let parsed = query.parse().map_err(Failure::Invalid)?;
    let found = search::search(
        &app.launch.state,
        &parsed,
        &app.launch.reviewer,
        u64::from(start.unwrap_or(0)),
    )
    .await
    .map_err(Failure::Read)?;
    let labels = app
        .browse
        .review_labels(found.requests.iter().map(|(view, _)| &view.request.run_id));
    let now = time::OffsetDateTime::now_utc();
    let reviews = tokio::task::spawn_blocking(move || {
        let mut fallback = std::collections::BTreeMap::new();
        found
            .requests
            .into_iter()
            .map(|(view, entry)| {
                let request = &view.request;
                let label = labels.get(&request.run_id).unwrap_or_else(|| {
                    fallback
                        .entry(entry.repository.clone())
                        .or_insert_with(|| monitor::live::Label::at(&entry.repository))
                });
                ReviewMatch {
                    id: request.id.clone().into(),
                    target: artifactize_core::review::Target::from(request.as_ref()).into(),
                    title: request.title.clone(),
                    eval: entry.eval,
                    worktree: PathKey::new(&entry.repository),
                    project: label.project.clone(),
                    branch: label.branch.clone(),
                    age: monitor::duration(now - request.created_at.time()),
                    mark: monitor::Status::from(entry.status).into(),
                    claimant: entry.claim.reviewer().map(str::to_owned),
                    mine: matches!(entry.claim, search::Claim::Mine(_)),
                }
            })
            .collect()
    })
    .await
    .map_err(|error| Failure::Read(error.to_string()))?;
    Ok(ReviewPage {
        reviews,
        next: found
            .next
            .map(u32::try_from)
            .transpose()
            .map_err(|error| Failure::Read(error.to_string()))?,
    })
}

/// Number of unclaimed Human reviews, including deferred sign-offs, for the Review mode badge.
#[tauri::command]
#[specta::specta]
pub async fn human_waiting_count(app: tauri::State<'_, App>) -> Result<u32, Failure> {
    let query = search::DEFAULT
        .parse()
        .expect("the default Review query parses");
    let count = search::count(&app.launch.state, &query, &app.launch.reviewer)
        .await
        .map_err(Failure::Read)?;
    u32::try_from(count).map_err(|error| Failure::Read(error.to_string()))
}

/// Complete the current Review query term using the library's grammar and suggestions.
#[tauri::command]
#[specta::specta]
pub async fn human_search_complete(
    app: tauri::State<'_, App>,
    query: String,
) -> Result<Completion, Failure> {
    let reply = search::suggest(&app.launch.state, &query, &app.launch.reviewer)
        .await
        .map_err(Failure::Invalid)?;
    Ok(Completion {
        keep: reply.keep,
        candidates: reply.candidates,
    })
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OpenReview {
    pub view: HumanView,
    pub outside_filter: bool,
    pub outside_reason: Option<String>,
}

/// Open the exact review ID even when the saved filter would not list it.
#[tauri::command]
#[specta::specta]
pub async fn human_open_reference(
    app: tauri::State<'_, App>,
    id: RequestKey,
    query: String,
) -> Result<OpenReview, Failure> {
    let id = id.into_inner();
    let query = query.parse().map_err(Failure::Invalid)?;
    let (number, opened) = app
        .human
        .reads
        .numbered(search::open(
            &app.launch.state,
            &id,
            &query,
            &app.launch.reviewer,
        ))
        .await;
    let opened = opened.map_err(Failure::Read)?;
    let outside_filter = opened.outside_filter();
    let view = super::loaded(&app, number, &id, opened.view).await;
    Ok(OpenReview {
        view: super::show(&app, number, &view),
        outside_filter,
        outside_reason: opened.outside_reason,
    })
}
