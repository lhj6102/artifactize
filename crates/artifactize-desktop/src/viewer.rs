//! Host-only IPC for viewer windows. The untrusted page never invokes these commands directly.

use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};

use artifactize_core::{config::plugins, review::Outcome, store, tools::human::Content, viewer};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

use crate::{App, Failure, ids::RequestKey};
use artifactize_core::types::RequestId;

type ActiveEvent = Box<dyn Fn(ActiveReviewChanged) + Send + Sync>;

#[derive(Default)]
pub(crate) struct Sessions(Mutex<SessionsInner>, OnceLock<ActiveEvent>);

#[derive(Default)]
struct SessionsInner {
    last: u32,
    live: BTreeMap<u32, Arc<viewer::Session>>,
    /// What each live session shows: an Artifact of a worktree in one viewer.
    places: BTreeMap<u32, Place>,
    active: Option<RequestId>,
}

/// What a viewer session shows. Two sessions of the same place show the same thing, whatever
/// their titles: a window already showing it is brought to the front instead of a second one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Place {
    worktree: PathBuf,
    artifact: String,
    viewer: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ViewerLaunch {
    pub session: u32,
    pub title: String,
}

impl Sessions {
    pub(crate) fn forward<R: tauri::Runtime>(&self, handle: tauri::AppHandle<R>) {
        let _ = self.1.set(Box::new(move |event| {
            let _ = event.emit(&handle);
        }));
    }

    fn notify(&self, request: Option<RequestKey>) {
        if let Some(emit) = self.1.get() {
            emit(ActiveReviewChanged { request });
        }
    }

    fn keep(&self, viewer: viewer::Session, worktree: PathBuf) -> ViewerLaunch {
        let context = viewer.context(None);
        let title = format!("{} — {}", context.artifact, context.title);
        let place = Place {
            worktree,
            artifact: context.artifact.to_string(),
            viewer: context.type_name.to_string(),
        };
        let session = self.remember(place);
        self.0
            .lock()
            .expect("viewer sessions")
            .live
            .insert(session, Arc::new(viewer));
        ViewerLaunch { session, title }
    }

    /// Number a new session showing `place`.
    fn remember(&self, place: Place) -> u32 {
        let mut sessions = self.0.lock().expect("viewer sessions");
        sessions.last = sessions.last.checked_add(1).expect("unique viewer session");
        let session = sessions.last;
        sessions.places.insert(session, place);
        session
    }

    /// Sessions `one` and `other` show the same Artifact of the same worktree in the same viewer.
    pub(crate) fn same_place(&self, one: u32, other: u32) -> bool {
        let sessions = self.0.lock().expect("viewer sessions");
        match (sessions.places.get(&one), sessions.places.get(&other)) {
            (Some(one), Some(other)) => one == other,
            _ => false,
        }
    }

    fn get(&self, session: u32) -> Result<Arc<viewer::Session>, Failure> {
        self.0
            .lock()
            .expect("viewer sessions")
            .live
            .get(&session)
            .cloned()
            .ok_or_else(|| Failure::Missing("Artifact Viewer session closed.".into()))
    }

    /// End viewer session `session`: its window closed, or another shows the same Artifact.
    pub(crate) fn drop_session(&self, session: u32) {
        let mut sessions = self.0.lock().expect("viewer sessions");
        sessions.live.remove(&session);
        sessions.places.remove(&session);
    }

    /// The page of viewer session `session` when its viewer is a plugin's web page.
    pub(crate) fn web_page(&self, session: u32) -> Option<String> {
        let viewer = self
            .0
            .lock()
            .expect("viewer sessions")
            .live
            .get(&session)
            .cloned()?;
        match viewer.entry() {
            plugins::ViewerEntry::Web { html } => Some(html.to_string()),
            plugins::ViewerEntry::Builtin { .. } => None,
        }
    }

    pub(crate) fn reset(&self) {
        let mut sessions = self.0.lock().expect("viewer sessions");
        sessions.active = None;
        sessions.live.clear();
        sessions.places.clear();
    }
}

pub(crate) enum Source {
    /// The original request which owns the tool, even when opened through a follower.
    Review(RequestId),
    Workspace(PathBuf),
}

/// A viewer is minted only when the library has run an admitted viewer Human tool.
pub(crate) async fn launch(
    app: &App,
    outcome: &Outcome,
    source: Source,
) -> Result<Option<ViewerLaunch>, Failure> {
    let Outcome::Ran {
        result: Ok(result), ..
    } = outcome
    else {
        return Ok(None);
    };
    let [Content::Viewer { artifact }] = result.content() else {
        return Ok(None);
    };
    let workspace = match source {
        Source::Review(id) => store::read_request_repo(&app.launch.state, id.as_str())
            .await
            .map_err(Failure::Read)?,
        Source::Workspace(workspace) => workspace,
    };
    let artifact = artifact.clone();
    let worktree = workspace.clone();
    let open = tokio::task::spawn_blocking(move || viewer::Session::open(&workspace, &artifact));
    let viewer = crate::failure::joined(open.await)?.map_err(Failure::Read)?;
    Ok(Some(app.viewers.keep(viewer, worktree)))
}

#[derive(Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ViewerContext {
    pub artifact: String,
    #[serde(rename = "type")]
    pub type_name: String,
    pub title: String,
    pub target: String,
    pub target_kind: crate::definitions::DefinitionKind,
    pub review: Option<RequestKey>,
}

#[tauri::command]
#[specta::specta]
pub async fn viewer_context(
    app: tauri::State<'_, App>,
    session: u32,
) -> Result<ViewerContext, Failure> {
    let viewer = app.viewers.get(session)?;
    let review = active(&app).await;
    let context = viewer.context(review.as_ref());
    let review = context.review.cloned().map(RequestKey::from);
    Ok(ViewerContext {
        artifact: context.artifact.to_string(),
        type_name: context.type_name.to_string(),
        title: context.title.to_owned(),
        target: context.target.to_string(),
        target_kind: context.target_kind.into(),
        review,
    })
}

#[derive(Serialize, Type)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ViewerEntry {
    Builtin { viewer: BuiltinViewer },
    Web { html: String },
}

#[derive(Serialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum BuiltinViewer {
    Files,
    Markdown,
    Artifactize,
}

#[tauri::command]
#[specta::specta]
pub fn viewer_entry(app: tauri::State<'_, App>, session: u32) -> Result<ViewerEntry, Failure> {
    use plugins::BuiltinViewer as Core;
    Ok(match app.viewers.get(session)?.entry() {
        plugins::ViewerEntry::Builtin { viewer } => ViewerEntry::Builtin {
            viewer: match viewer {
                Core::Files => BuiltinViewer::Files,
                Core::Markdown => BuiltinViewer::Markdown,
                Core::Artifactize => BuiltinViewer::Artifactize,
            },
        },
        plugins::ViewerEntry::Web { html } => ViewerEntry::Web {
            html: html.to_string(),
        },
    })
}

#[derive(Serialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum ViewerFileKind {
    File,
    Directory,
}

#[derive(Serialize, Type)]
pub struct ViewerFile {
    pub name: String,
    pub path: String,
    pub kind: ViewerFileKind,
}

#[tauri::command]
#[specta::specta]
pub async fn viewer_files(
    app: tauri::State<'_, App>,
    session: u32,
    path: String,
) -> Result<Vec<ViewerFile>, Failure> {
    let path = path
        .parse()
        .map_err(|error: artifactize_tools::scope::ScopeError| {
            Failure::Invalid(error.to_string())
        })?;
    let viewer = app.viewers.get(session)?;
    let list = tokio::task::spawn_blocking(move || viewer.files(&path));
    Ok(crate::failure::joined(list.await)?
        .map_err(Failure::Read)?
        .into_iter()
        .map(|entry| ViewerFile {
            name: entry.name,
            path: entry.path.to_string(),
            kind: match entry.kind {
                viewer::EntryKind::File => ViewerFileKind::File,
                viewer::EntryKind::Directory => ViewerFileKind::Directory,
            },
        })
        .collect())
}

#[tauri::command]
#[specta::specta]
pub async fn viewer_read_text(
    app: tauri::State<'_, App>,
    session: u32,
    path: String,
) -> Result<String, Failure> {
    let path = path
        .parse()
        .map_err(|error: artifactize_tools::scope::ScopeError| {
            Failure::Invalid(error.to_string())
        })?;
    let viewer = app.viewers.get(session)?;
    let read = tokio::task::spawn_blocking(move || viewer.read_text(&path));
    crate::failure::joined(read.await)?.map_err(Failure::Read)
}

/// The exact scoped graph wire shape used by the library. Serialize at the IPC edge.
#[tauri::command]
#[specta::specta]
pub async fn viewer_declarations(
    app: tauri::State<'_, App>,
    session: u32,
) -> Result<String, Failure> {
    let viewer = app.viewers.get(session)?;
    let read = tokio::task::spawn_blocking(move || viewer.declarations());
    let graph = crate::failure::joined(read.await)?.map_err(Failure::Read)?;
    Ok(serde_json::to_string(&graph).expect("declared graph is JSON"))
}

#[tauri::command]
#[specta::specta]
pub fn viewer_close(app: tauri::State<'_, App>, session: u32) {
    app.viewers.drop_session(session);
}

/// Append to the shared draft and return the updated plain-text response.
#[tauri::command]
#[specta::specta]
pub async fn viewer_point(
    app: tauri::State<'_, App>,
    session: u32,
    text: String,
) -> Result<String, Failure> {
    let viewer = app.viewers.get(session)?;
    let review = active(&app)
        .await
        .ok_or_else(|| Failure::Action("Pointing needs an active Human review.".into()))?;
    viewer
        .pointing(&app.launch.state, &review, &text)
        .await
        .map(|draft| draft.notes)
        .map_err(Failure::Action)
}

/// The last shown claimed review changed; open viewers read their context again.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct ActiveReviewChanged {
    pub request: Option<RequestKey>,
}

async fn active(app: &App) -> Option<viewer::Review> {
    let id = app
        .viewers
        .0
        .lock()
        .expect("viewer sessions")
        .active
        .clone()?;
    let view = store::read_request(&app.launch.state, id.as_str())
        .await
        .ok()?;
    artifactize_core::review::review_refusal(Some(&view), &app.launch.reviewer)
        .is_none()
        .then(|| viewer::Review {
            id,
            reviewer: app.launch.reviewer.clone(),
        })
}

pub(crate) fn settled(app: &App, id: &RequestId) {
    let mut sessions = app.viewers.0.lock().expect("viewer sessions");
    if sessions.active.as_ref() == Some(id) {
        sessions.active = None;
        app.viewers.notify(None);
    }
}

#[tauri::command]
#[specta::specta]
pub async fn active_review_set(
    app: tauri::State<'_, App>,
    request: Option<RequestKey>,
) -> Result<(), Failure> {
    let id = if let Some(request) = &request {
        let id = request.clone().into_inner();
        let view = store::read_request(&app.launch.state, id.as_str())
            .await
            .map_err(Failure::Read)?;
        if let Some(error) =
            artifactize_core::review::review_refusal(Some(&view), &app.launch.reviewer)
        {
            return Err(Failure::Action(error));
        }
        Some(id)
    } else {
        None
    };
    let mut sessions = app.viewers.0.lock().expect("viewer sessions");
    if sessions.active != id {
        sessions.active = id;
        app.viewers.notify(request);
    }
    Ok(())
}

/// A submission or release from another surface must also hide pointing. Watch hints never
/// decide the state; only a successfully read, no-longer-active review does.
pub(crate) async fn refresh_active(app: &App) {
    let id = app
        .viewers
        .0
        .lock()
        .expect("viewer sessions")
        .active
        .clone();
    if let Some(id) = id
        && let Ok(view) = store::read_request(&app.launch.state, id.as_str()).await
        && artifactize_core::review::review_refusal(Some(&view), &app.launch.reviewer).is_some()
    {
        settled(app, &id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(worktree: &str, artifact: &str, viewer: &str) -> Place {
        Place {
            worktree: PathBuf::from(worktree),
            artifact: artifact.into(),
            viewer: viewer.into(),
        }
    }

    #[test]
    fn a_session_shows_the_same_place_only_for_the_same_worktree_artifact_and_viewer() {
        let sessions = Sessions::default();
        let first = sessions.remember(place("/work/one", "checkout", "files"));
        let other_worktree = sessions.remember(place("/work/two", "checkout", "files"));
        let again = sessions.remember(place("/work/one", "checkout", "files"));
        let other_viewer = sessions.remember(place("/work/one", "checkout", "markdown"));
        assert!(
            !sessions.same_place(first, other_worktree),
            "a same-named Artifact of another worktree is another place"
        );
        assert!(sessions.same_place(first, again));
        assert!(!sessions.same_place(first, other_viewer));
        sessions.drop_session(again);
        assert!(
            !sessions.same_place(first, again),
            "an ended session shows nothing"
        );
    }
}
