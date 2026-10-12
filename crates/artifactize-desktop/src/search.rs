//! The review screen's request search: [`artifactize::review::search`] runs the query, pages
//! and completion; this keeps the last page for clock redraws, and opens the read-only Detail
//! of a request that is not a Human review.
use crate::{
    App, Failure,
    detail::{self, DetailView, Profile},
    human,
    status::Mark,
};
use artifactize::{
    monitor::{self, NodeId},
    review::search::{self, Entry, Query},
    store::{self, RequestView},
};
use serde::Serialize;
use specta::Type;
use std::sync::Mutex;
use time::OffsetDateTime;

/// The last page read: its query, where it starts, its state generation and its requests.
#[derive(Default)]
pub struct Searches {
    last: Mutex<Option<Page>>,
}

#[derive(Clone)]
struct Page {
    query: String,
    start: u32,
    generation: u32,
    found: Vec<(RequestView, Entry)>,
    next: Option<u32>,
}

/// One request the search found.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Listed {
    pub id: String,
    pub eval: String,
    pub title: String,
    pub kind: Profile,
    pub mark: Mark,
    /// `-`, the claimant, or `name (you)`.
    pub claim: String,
    pub mine: bool,
    pub age: String,
    pub repository: String,
}

/// A page of what the query found.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Results {
    /// The state generation it was read from; a newer one drops open reviews and drafts.
    pub generation: u32,
    pub rows: Vec<Listed>,
    /// Where the next (older) page starts, when there is one.
    pub next: Option<u32>,
    /// Why the query is invalid; it then lists nothing.
    pub error: Option<String>,
    /// Only waiting Human reviews can match, as with the default query.
    pub waiting: bool,
}

/// The completion of a query's last term.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Completion {
    /// The query before the completed term; a candidate replaces the rest.
    pub keep: String,
    pub candidates: Vec<String>,
}

fn row(view: &RequestView, entry: &Entry, now: OffsetDateTime) -> Listed {
    let request = &view.request;
    Listed {
        id: entry.request.clone(),
        eval: entry.eval.clone(),
        title: request.title.clone(),
        kind: entry.kind.into(),
        mark: monitor::Status::from(entry.status).into(),
        claim: match (&entry.reviewer, entry.mine) {
            (None, _) => "-".into(),
            (Some(reviewer), true) => format!("{reviewer} (you)"),
            (Some(reviewer), false) => reviewer.clone(),
        },
        mine: entry.mine,
        age: monitor::duration((now - request.created_at.time()).whole_seconds()),
        repository: entry.repository.clone(),
    }
}

/// Requests matching `query` from position `start` (0 for the newest). `reload: false`
/// redraws the last page of the same query and start without reading the state.
#[tauri::command]
#[specta::specta]
pub async fn review_search(
    app: tauri::State<'_, App>,
    query: String,
    start: u32,
    reload: bool,
) -> Result<Results, Failure> {
    let parsed = query.parse::<Query>();
    let cached = app.searches.last.lock().expect("search lock").clone();
    let page = match cached {
        Some(page) if !reload && page.query == query && page.start == start => page,
        _ => {
            let generation = human::current(&app).await?;
            let found = match &parsed {
                Ok(parsed) => search::search(
                    &app.launch.state,
                    parsed,
                    &app.launch.reviewer,
                    u64::from(start),
                )
                .await
                .map_err(Failure::Read)?,
                Err(_) => search::Found::default(),
            };
            app.human
                .remember(found.requests.iter().map(|(view, _)| view.clone()));
            let page = Page {
                query: query.clone(),
                start,
                generation,
                next: found
                    .next
                    .map(|next| u32::try_from(next).unwrap_or(u32::MAX)),
                found: found.requests,
            };
            *app.searches.last.lock().expect("search lock") = Some(page.clone());
            page
        }
    };
    let now = OffsetDateTime::now_utc();
    Ok(Results {
        generation: page.generation,
        rows: page
            .found
            .iter()
            .map(|(view, entry)| row(view, entry, now))
            .collect(),
        next: page.next,
        error: parsed.as_ref().err().cloned(),
        waiting: parsed.as_ref().is_ok_and(Query::waiting_human),
    })
}

/// Completion candidates for the query's last term, from the requests the terms before it
/// find.
#[tauri::command]
#[specta::specta]
pub async fn review_complete(
    app: tauri::State<'_, App>,
    query: String,
) -> Result<Completion, Failure> {
    let completion = search::suggest(&app.launch.state, &query, &app.launch.reviewer)
        .await
        .map_err(Failure::Read)?;
    Ok(Completion {
        keep: completion.keep,
        candidates: completion.candidates,
    })
}

/// The read-only Detail of request `id` in its Run, as the monitor shows it.
#[tauri::command]
#[specta::specta]
pub async fn review_detail(app: tauri::State<'_, App>, id: String) -> Result<DetailView, Failure> {
    let view = store::read_request(&app.launch.state, &id)
        .await
        .map_err(Failure::Read)?;
    let (run, requests) = store::read_run_requests(&app.launch.state, &view.request.run_id)
        .await
        .map_err(Failure::Read)?;
    let node = NodeId::eval(view.request.eval_id.clone());
    Ok(detail::view(&app.launch.state, &run, &requests, &node))
}
