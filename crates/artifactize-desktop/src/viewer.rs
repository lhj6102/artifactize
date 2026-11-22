//! Host-only IPC for viewer windows. The untrusted page never invokes these commands directly.

use std::{
    collections::BTreeMap,
    future::Future,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};

use artifactize_core::{
    config::{Target, plugins},
    review::Outcome,
    store,
    tools::human::Content,
    types::{ArtifactName, RequestId},
    viewer,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

use crate::{App, Failure, generation::Generation, ids::RequestKey};

type ActiveEvent = Box<dyn Fn(ActiveReviewChanged) + Send + Sync>;

#[derive(Default)]
pub(crate) struct Sessions(Mutex<SessionsInner>, OnceLock<ActiveEvent>);

#[derive(Default)]
struct SessionsInner {
    last: u32,
    live: BTreeMap<u32, Arc<viewer::Session>>,
    /// What each live session shows: an Artifact of a worktree in one viewer.
    places: BTreeMap<u32, Place>,
    active: Option<ActiveReview>,
    activation: u64,
}

/// The review as shown, and the original request whose claim, Artifact and draft it uses.
#[derive(Clone, PartialEq, Eq)]
struct ActiveReview {
    shown: RequestId,
    owner: RequestId,
    address: Option<Address>,
}

impl ActiveReview {
    async fn claimed(&self, app: &App) -> Result<bool, String> {
        let view = store::read_request_as(
            &app.launch.state,
            self.owner.as_str(),
            store::Shown::Requests,
        )
        .await?;
        Ok(artifactize_core::review::review_refusal(Some(&view), &app.launch.reviewer).is_none())
    }
}

/// What a viewer session shows. Two sessions of the same place show the same thing, whatever
/// their titles: a window already showing it is brought to the front instead of a second one.
#[derive(Debug, PartialEq, Eq)]
struct Place {
    address: Address,
    viewer: String,
    scope: viewer::Scope,
}

/// Where the Artifact's scope points, not just the name by which it was found. A checkout can
/// change the resolved target without changing the worktree or the Artifact's name.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Address {
    worktree: PathBuf,
    artifact: ArtifactName,
    target: Target,
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
            address: Address {
                worktree,
                artifact: context.artifact.clone(),
                target: context.target.clone(),
            },
            viewer: context.type_name.to_string(),
            scope: viewer.scope(),
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

    /// Sessions `one` and `other` show the same current Artifact scope and viewer.
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

    fn active(&self) -> Option<ActiveReview> {
        self.0.lock().expect("viewer sessions").active.clone()
    }

    /// Reserve the selection before its first read. Lookup completion order never changes
    /// which request may publish, even when both requests use the same state generation.
    async fn activate(
        &self,
        state: &Generation,
        read: impl Future<Output = Result<Option<ActiveReview>, Failure>>,
    ) -> Result<(), Failure> {
        let activation = {
            let mut sessions = self.0.lock().expect("viewer sessions");
            sessions.advance_activation()
        };
        let generation = state.current();
        let review = read.await;
        state
            .if_current(generation, || {
                let mut sessions = self.0.lock().expect("viewer sessions");
                if sessions.activation != activation {
                    return Ok(());
                }
                let review = review?;
                if sessions.active != review {
                    let request = review.as_ref().map(|review| review.shown.clone().into());
                    sessions.active = review;
                    self.notify(request);
                }
                Ok(())
            })
            .ok_or_else(|| {
                Failure::Read("The state was replaced while reading the review; reopen it.".into())
            })?
    }

    pub(crate) fn reset(&self) {
        let mut sessions = self.0.lock().expect("viewer sessions");
        sessions.advance_activation();
        sessions.active = None;
        sessions.live.clear();
        sessions.places.clear();
    }
}

impl SessionsInner {
    fn advance_activation(&mut self) -> u64 {
        self.activation = self
            .activation
            .checked_add(1)
            .expect("unique viewer activation");
        self.activation
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
    /// This session shows the Artifact target recorded by the active review in its original worktree.
    pub pet_match: bool,
}

#[tauri::command]
#[specta::specta]
pub async fn viewer_context(
    app: tauri::State<'_, App>,
    session: u32,
) -> Result<ViewerContext, Failure> {
    let viewer = app.viewers.get(session)?;
    let active = active(&app).await;
    let (review, pet_match) = {
        let sessions = app.viewers.0.lock().expect("viewer sessions");
        let active = active.filter(|review| sessions.active.as_ref() == Some(review));
        active.map_or((None, false), |review| {
            let matches = sessions
                .places
                .get(&session)
                .is_some_and(|place| review.address.as_ref() == Some(&place.address));
            (Some(review.shown.into()), matches)
        })
    };
    let context = viewer.context(None);
    Ok(ViewerContext {
        artifact: context.artifact.to_string(),
        type_name: context.type_name.to_string(),
        title: context.title.to_owned(),
        target: context.target.to_string(),
        target_kind: context.target_kind.into(),
        review,
        pet_match,
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

/// Append to the review that was active when Point began, never to its replacement.
#[tauri::command]
#[specta::specta]
pub async fn viewer_point(
    app: tauri::State<'_, App>,
    session: u32,
    review: RequestKey,
    text: String,
) -> Result<(), Failure> {
    let viewer = app.viewers.get(session)?;
    let expected = review.into_inner();
    let active = active(&app)
        .await
        .filter(|review| review.shown == expected && app.viewers.active().as_ref() == Some(review))
        .ok_or_else(|| Failure::Action("The active review changed. Start Point again.".into()))?;
    let owner = viewer::Review {
        id: active.owner,
        reviewer: app.launch.reviewer.clone(),
    };
    viewer
        .pointing(&app.launch.state, &owner, &text)
        .await
        .map(|_| ())
        .map_err(Failure::Action)
}

/// The last shown claimed review changed; open viewers read their context again.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct ActiveReviewChanged {
    pub request: Option<RequestKey>,
}

async fn active(app: &App) -> Option<ActiveReview> {
    let review = app.viewers.active()?;
    review.claimed(app).await.ok()?.then_some(review)
}

pub(crate) fn settled(app: &App, id: &RequestId) {
    let mut sessions = app.viewers.0.lock().expect("viewer sessions");
    if sessions
        .active
        .as_ref()
        .is_some_and(|review| &review.shown == id || &review.owner == id)
    {
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
    app.viewers
        .activate(&app.generation, async {
            let Some(request) = request else {
                return Ok(None);
            };
            let state = &app.launch.state;
            let shown = request.into_inner();
            let exact = store::read_request_as(state, shown.as_str(), store::Shown::Requests)
                .await
                .map_err(Failure::Read)?;
            let owner = if exact.request.status().is_human_wait() {
                store::read_original_as(state, &exact, store::Shown::Requests)
                    .await
                    .map_err(Failure::Read)?
            } else {
                exact
            };
            if let Some(error) =
                artifactize_core::review::review_refusal(Some(&owner), &app.launch.reviewer)
            {
                return Err(Failure::Action(error));
            }
            let artifact = &owner.request.target;
            let target = owner
                .request
                .human_definition
                .as_ref()
                .and_then(|definition| definition.artifacts.value())
                .and_then(|artifacts| artifacts.get(artifact))
                .and_then(|saved| saved.path.value());
            let address = if let Some(target) = target {
                let worktree = store::read_request_repo(state, owner.request.id.as_str())
                    .await
                    .map_err(Failure::Read)?;
                Some(Address {
                    worktree,
                    artifact: artifact.clone(),
                    target: target.clone(),
                })
            } else {
                None
            };
            Ok(Some(ActiveReview {
                shown,
                owner: owner.request.id.clone(),
                address,
            }))
        })
        .await
}

/// A submission or release from another surface must also hide pointing. Watch hints never
/// decide the state; only a successfully read, no-longer-active review does.
pub(crate) async fn refresh_active(app: &App) {
    if let Some(review) = app.viewers.active()
        && let Ok(false) = review.claimed(app).await
    {
        let mut sessions = app.viewers.0.lock().expect("viewer sessions");
        if sessions.active.as_ref() == Some(&review) {
            sessions.active = None;
            app.viewers.notify(None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_late_lookup_cannot_publish_an_earlier_review_selection() {
        let sessions = Sessions::default();
        let generation = Generation::default();
        let applied = Arc::new(Mutex::new(Vec::new()));
        let observed = applied.clone();
        assert!(
            sessions
                .1
                .set(Box::new(move |event| {
                    observed
                        .lock()
                        .unwrap()
                        .push(event.request.unwrap().into_inner());
                }))
                .is_ok()
        );
        let first = "run-first-1".parse::<RequestId>().unwrap();
        let latest = "run-latest-1".parse::<RequestId>().unwrap();
        let review = |id: &RequestId| {
            Some(ActiveReview {
                shown: id.clone(),
                owner: id.clone(),
                address: None,
            })
        };
        let (entered, wait_for_older) = tokio::sync::oneshot::channel();
        let (finish, wait_for_newer) = tokio::sync::oneshot::channel();
        let older = sessions.activate(&generation, async {
            entered.send(()).unwrap();
            wait_for_newer.await.unwrap();
            Ok(review(&first))
        });
        let newer = async {
            wait_for_older.await.unwrap();
            sessions
                .activate(&generation, std::future::ready(Ok(review(&latest))))
                .await
                .unwrap();
            finish.send(()).unwrap();
        };
        let (stale, ()) = tokio::join!(older, newer);
        stale.unwrap();
        assert_eq!(sessions.active().unwrap().shown, latest);
        assert_eq!(applied.lock().unwrap().as_slice(), &[latest]);
    }

    fn place(worktree: &str, target: &str, viewer: &str) -> Place {
        Place {
            address: Address {
                worktree: PathBuf::from(worktree),
                artifact: "checkout".parse().unwrap(),
                target: serde_json::from_value(serde_json::json!(target)).unwrap(),
            },
            viewer: viewer.into(),
            scope: viewer::Scope::default(),
        }
    }

    #[test]
    fn a_session_shows_the_same_place_only_for_the_same_worktree_target_and_viewer() {
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

    #[test]
    fn opening_a_different_target_creates_a_window_instead_of_reusing_the_old_one() {
        use tauri::Manager;

        let mock = tauri::test::mock_app();
        let app = App::new(crate::Launch {
            mode: crate::Mode::Monitor,
            state: PathBuf::new(),
            repo: None,
            reviewer: "alice".parse().unwrap(),
            request: None,
        });
        let first = app
            .viewers
            .remember(place("/work/one", "checkout", "files"));
        let same = app
            .viewers
            .remember(place("/work/one", "checkout", "files"));
        let new = app
            .viewers
            .remember(place("/work/one", "alternate", "files"));
        for session in [first, same, new] {
            crate::windows::viewer(mock.handle(), &app, session, "Checkout".into()).unwrap();
        }
        assert!(
            mock.get_webview_window(&format!("viewer-{first}"))
                .is_some()
        );
        assert!(mock.get_webview_window(&format!("viewer-{same}")).is_none());
        assert!(mock.get_webview_window(&format!("viewer-{new}")).is_some());
        for window in mock.webview_windows().into_values() {
            window.destroy().unwrap();
        }
    }
}
