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
    let start = start.unwrap_or(0);
    let parsed = query
        .parse::<search::Query>()
        .map_err(Failure::Invalid)?
        .for_places()
        .for_live_repositories();
    let page = app
        .human
        .searches
        .read(query, start, search_page(&app, &parsed, start))
        .await?;
    app.human.searches.watch(&page);
    Ok(page)
}

async fn search_page(app: &App, parsed: &search::Query, start: u32) -> Result<ReviewPage, Failure> {
    let found = search::search(
        &app.launch.state,
        parsed,
        &app.launch.reviewer,
        u64::from(start),
    )
    .await
    .map_err(Failure::Read)?;
    let now = time::OffsetDateTime::now_utc();
    let reviews = found
        .requests
        .into_iter()
        .map(|(view, entry)| {
            let request = &view.request;
            let label = entry.label.unwrap_or_default();
            ReviewMatch {
                id: request.id.clone().into(),
                target: artifactize_core::review::Target::from(request.as_ref()).into(),
                title: request.title.clone(),
                eval: entry.eval,
                worktree: PathKey::new(&entry.repository),
                project: label.project,
                branch: label.branch,
                age: monitor::duration(now - request.created_at.time()),
                mark: monitor::Status::from(entry.status).into(),
                claimant: entry.claim.reviewer().map(str::to_owned),
                mine: matches!(entry.claim, search::Claim::Mine(_)),
            }
        })
        .collect();
    Ok(ReviewPage {
        reviews,
        next: found
            .next
            .map(u32::try_from)
            .transpose()
            .map_err(|error| Failure::Read(error.to_string()))?,
    })
}

/// Number of pending Human verdicts, claimed or not, for the Review mode badge.
#[tauri::command]
#[specta::specta]
pub async fn human_waiting_count(app: tauri::State<'_, App>) -> Result<u32, Failure> {
    let query = "type:human status:waiting_human"
        .parse::<search::Query>()
        .expect("the pending-review query parses")
        .for_live_repositories();
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
    let reply = search::suggest_places(&app.launch.state, &query, &app.launch.reviewer)
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
    let query = query
        .parse::<search::Query>()
        .map(|query| query.for_places().for_live_repositories());
    let generation = app.generation.current();
    let (number, opened) = app
        .human
        .reads
        .numbered(async {
            let exact = || {
                artifactize_core::store::read_request_as(
                    &app.launch.state,
                    &id,
                    artifactize_core::store::Shown::Requests,
                )
            };
            let (exact, reason) = match query {
                Ok(query) => {
                    match search::open(&app.launch.state, &id, &query, &app.launch.reviewer).await {
                        Ok(opened) => (opened.view, opened.outside_reason),
                        Err(error) => (
                            exact().await?,
                            Some(format!("The current filter could not be checked: {error}")),
                        ),
                    }
                }
                Err(_) => (
                    exact().await?,
                    Some("The current filter is incomplete or invalid.".into()),
                ),
            };
            Ok::<_, String>((super::resolve(&app, generation, exact).await?, reason))
        })
        .await;
    let (views, outside_reason) = opened.map_err(Failure::Read)?;
    let view = super::loaded(&app, number, views).await?;
    Ok(OpenReview {
        view: super::show(&app, number, &view),
        outside_filter: outside_reason.is_some(),
        outside_reason,
    })
}
