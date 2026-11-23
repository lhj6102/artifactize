//! Host-only IPC for viewer windows. The untrusted page never invokes these commands directly.

use std::{
    collections::BTreeMap,
    future::Future,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};

use artifactize_core::{
    config::{Target, plugins},
    review::{Outcome, log},
    store,
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
    launching: BTreeMap<u32, PendingLaunch>,
    /// What each live session shows: an Artifact of a worktree in one viewer.
    places: BTreeMap<u32, Place>,
    selected: Option<RequestId>,
    active: Option<ActiveReview>,
    activation: u64,
}

/// A pending Viewer keeps its admission until the native window opens or is cancelled.
pub(crate) struct PendingLaunch {
    job: crate::human::Lease,
    review: Option<RequestId>,
    log: log::Run,
}

/// The review as shown, and the original request whose claim, Artifact and draft it uses.
#[derive(Clone, PartialEq, Eq)]
struct ActiveReview {
    shown: RequestId,
    owner: RequestId,
    addresses: Vec<Address>,
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
    checkout: artifactize_core::repository::Identity,
    artifact: ArtifactName,
    target: Target,
}

impl Address {
    fn same_artifact(&self, other: &Self) -> bool {
        self.worktree == other.worktree
            && self.checkout == other.checkout
            && self.artifact == other.artifact
    }
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

    fn keep(
        &self,
        viewer: viewer::Session,
        worktree: PathBuf,
        pending: PendingLaunch,
    ) -> Option<ViewerLaunch> {
        let context = viewer.context(None);
        let title = format!("{} — {}", context.artifact, context.title);
        let place = Place {
            address: Address {
                worktree,
                checkout: viewer.checkout().clone(),
                artifact: context.artifact.clone(),
                target: context.target.clone(),
            },
            viewer: context.type_name.to_string(),
            scope: viewer.scope(),
        };
        let mut sessions = self.0.lock().expect("viewer sessions");
        if pending.job.cancel.is_cancelled() {
            return None;
        }
        let session = sessions.remember(place);
        sessions.live.insert(session, Arc::new(viewer));
        sessions.launching.insert(session, pending);
        Some(ViewerLaunch { session, title })
    }

    #[cfg(test)]
    fn remember(&self, place: Place) -> u32 {
        self.0.lock().expect("viewer sessions").remember(place)
    }

    /// Sessions `one` and `other` show the same current Artifact scope and viewer.
    pub(crate) fn same_place(&self, one: u32, other: u32) -> bool {
        let sessions = self.0.lock().expect("viewer sessions");
        match (sessions.places.get(&one), sessions.places.get(&other)) {
            (Some(one), Some(other)) => one == other,
            _ => false,
        }
    }

    /// The same viewer for this Artifact and checkout, even after it was retargeted.
    pub(crate) fn same_artifact(&self, one: u32, other: u32) -> bool {
        let sessions = self.0.lock().expect("viewer sessions");
        match (sessions.places.get(&one), sessions.places.get(&other)) {
            (Some(one), Some(other)) => {
                one.address.same_artifact(&other.address) && one.viewer == other.viewer
            }
            _ => false,
        }
    }

    pub(crate) fn get(&self, session: u32) -> Result<Arc<viewer::Session>, Failure> {
        self.0
            .lock()
            .expect("viewer sessions")
            .live
            .get(&session)
            .cloned()
            .ok_or_else(|| Failure::Missing("Artifact Viewer session closed.".into()))
    }

    pub(crate) fn pending_review(&self, session: u32) -> Option<RequestId> {
        self.0
            .lock()
            .expect("viewer sessions")
            .launching
            .get(&session)
            .and_then(|launch| launch.review.clone())
    }

    pub(crate) fn take_launch(&self, session: u32) -> Option<PendingLaunch> {
        self.0
            .lock()
            .expect("viewer sessions")
            .launching
            .remove(&session)
    }

    pub(crate) fn cancel_job(&self, caller: &crate::human::Caller, id: crate::ids::JobId) {
        self.cancel_where(|pending| pending.job.belongs_to(caller, id));
    }

    pub(crate) fn cancel_review(&self, review: &RequestId) {
        self.cancel_where(|pending| pending.review.as_ref() == Some(review));
    }

    fn cancel_where(&self, matches: impl Fn(&PendingLaunch) -> bool) {
        let mut sessions = self.0.lock().expect("viewer sessions");
        let cancelled: Vec<_> = sessions
            .launching
            .iter()
            .filter_map(|(id, launch)| matches(launch).then_some(*id))
            .collect();
        for id in cancelled {
            sessions.launching.remove(&id);
            sessions.live.remove(&id);
            sessions.places.remove(&id);
        }
    }

    /// End viewer session `session`: its window closed, or another shows the same Artifact.
    pub(crate) fn drop_session(&self, session: u32) {
        let mut sessions = self.0.lock().expect("viewer sessions");
        sessions.launching.remove(&session);
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
        shown: Option<&RequestId>,
        read: impl Future<Output = Result<Option<ActiveReview>, Failure>>,
    ) -> Result<(), Failure> {
        let activation = {
            let mut sessions = self.0.lock().expect("viewer sessions");
            let activation = sessions.advance_activation();
            sessions.selected = shown.cloned();
            if sessions
                .active
                .as_ref()
                .is_some_and(|active| Some(&active.shown) != shown)
            {
                sessions.active = None;
                self.notify(None);
            }
            activation
        };
        let generation = state.current();
        let review = read.await;
        state
            .if_current(generation, || {
                let mut sessions = self.0.lock().expect("viewer sessions");
                if sessions.activation != activation {
                    return Ok(());
                }
                let active = review.as_ref().ok().and_then(Option::as_ref);
                if sessions.active.as_ref() != active {
                    self.notify(active.map(|review| review.shown.clone().into()));
                    sessions.active = active.cloned();
                }
                review.map(|_| ())
            })
            .ok_or_else(|| {
                Failure::Read("The state was replaced while reading the review; reopen it.".into())
            })?
    }

    pub(crate) fn reset(&self) {
        let mut sessions = self.0.lock().expect("viewer sessions");
        sessions.advance_activation();
        sessions.selected = None;
        sessions.active = None;
        sessions.launching.clear();
        sessions.live.clear();
        sessions.places.clear();
    }
}

impl SessionsInner {
    fn remember(&mut self, place: Place) -> u32 {
        self.last = self.last.checked_add(1).expect("unique viewer session");
        self.places.insert(self.last, place);
        self.last
    }

    fn advance_activation(&mut self) -> u64 {
        self.activation = self
            .activation
            .checked_add(1)
            .expect("unique viewer activation");
        self.activation
    }
}

/// Transfer the admitted library viewer and retain the job until the native window responds.
pub(crate) fn launch(
    app: &App,
    outcome: &mut Outcome,
    review: Option<RequestId>,
    job: crate::human::Lease,
) -> Option<ViewerLaunch> {
    let Outcome::Ran {
        result: Ok(result), ..
    } = outcome
    else {
        return None;
    };
    let (viewer, log) = result.take_viewer_launch()?;
    let worktree = viewer.workspace().to_owned();
    app.viewers
        .keep(*viewer, worktree, PendingLaunch { job, review, log })
}

impl PendingLaunch {
    pub(crate) fn open(self, open: impl FnOnce() -> Result<(), Failure>) -> Result<(), Failure> {
        if self.job.cancel.is_cancelled() {
            return Err(Failure::Action("Viewer launch cancelled.".into()));
        }
        finish_launch(Some(self.log), open())
    }
}

/// Report the platform result, not the success of preparing the handoff.
pub(crate) fn finish_launch<T>(
    pending: Option<log::Run>,
    result: Result<T, Failure>,
) -> Result<T, Failure> {
    if let Some(run) = pending {
        run.end(match &result {
            Ok(_) => log::End::Launched,
            Err(error) => log::End::Failed(error.to_string()),
        });
    }
    result
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
    let checked = viewer.clone();
    let checkout =
        crate::failure::joined(tokio::task::spawn_blocking(move || checked.check_scope()).await)?
            .is_ok();
    let active = if checkout { active(&app).await } else { None };
    let (review, pet_match) = {
        let sessions = app.viewers.0.lock().expect("viewer sessions");
        let active = active.filter(|review| sessions.active.as_ref() == Some(review));
        active.map_or((None, false), |review| {
            let matches = sessions.places.get(&session).is_some_and(|place| {
                review
                    .addresses
                    .iter()
                    .any(|recorded| recorded.same_artifact(&place.address))
            });
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
    let expected = review.into_inner();
    let _turn = crate::human::draft_turns(&app, std::slice::from_ref(&expected)).await?;
    let _claim = app.human.claim_gate.lock().await;
    let viewer = app.viewers.get(session)?;
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
    if app.viewers.active().is_none() {
        refresh_active(app).await;
    }
    let review = app.viewers.active()?;
    review.claimed(app).await.ok()?.then_some(review)
}

pub(crate) async fn pet_is_claimed(app: &App, id: &RequestId) -> bool {
    active(app).await.is_some_and(|review| &review.shown == id)
}

pub(crate) fn pet_is_current(app: &App, id: &RequestId) -> bool {
    app.viewers
        .active()
        .is_some_and(|review| &review.shown == id)
}

pub(crate) fn settled(app: &App, id: &RequestId) {
    let mut sessions = app.viewers.0.lock().expect("viewer sessions");
    if sessions
        .active
        .as_ref()
        .is_some_and(|review| &review.shown == id || &review.owner == id)
    {
        sessions.advance_activation();
        sessions.active = None;
        app.viewers.notify(None);
        drop(sessions);
        app.windows.hide_pet(app);
    }
}

#[tauri::command]
#[specta::specta]
pub async fn active_review_set(
    app: tauri::State<'_, App>,
    request: Option<RequestKey>,
) -> Result<(), Failure> {
    let shown = request.map(RequestKey::into_inner);
    app.viewers
        .activate(
            &app.generation,
            shown.as_ref(),
            read_active(&app, shown.clone()),
        )
        .await
}

async fn read_active(app: &App, shown: Option<RequestId>) -> Result<Option<ActiveReview>, Failure> {
    let Some(shown) = shown else { return Ok(None) };
    let state = &app.launch.state;
    let exact = store::read_request_as(state, shown.as_str(), store::Shown::Requests)
        .await
        .map_err(Failure::Read)?;
    let owner = store::read_review_owner(state, &exact)
        .await
        .map_err(Failure::Read)?;
    if artifactize_core::review::review_refusal(Some(&owner), &app.launch.reviewer).is_some() {
        return Ok(None);
    }
    let mut addresses = Vec::new();
    if let Some(address) = review_address(state, &owner, None).await {
        addresses.push(address);
    }
    if exact.request.id != owner.request.id {
        let target =
            store::read_request_as(state, exact.request.id.as_str(), store::Shown::Definitions)
                .await
                .ok()
                .and_then(|view| {
                    view.definition
                        .and_then(|definition| definition.artifact().path.value().cloned())
                });
        if let Some(address) = review_address(state, &exact, target).await {
            addresses.push(address);
        }
    }
    Ok(Some(ActiveReview {
        shown,
        owner: owner.request.id.clone(),
        addresses,
    }))
}

async fn review_address(
    state: &std::path::Path,
    request: &store::RequestView,
    recorded_target: Option<Target>,
) -> Option<Address> {
    let artifact = &request.request.target;
    let target = recorded_target.or_else(|| {
        request
            .request
            .human_definition
            .as_ref()
            .and_then(|definition| definition.artifacts.value())
            .and_then(|artifacts| artifacts.get(artifact))
            .and_then(|saved| saved.path.value())
            .cloned()
    })?;
    let (worktree, checkout) = store::viewer_checkout(state, &request.request.run_id)
        .await
        .ok()?;
    Some(Address {
        worktree,
        checkout,
        artifact: artifact.clone(),
        target,
    })
}

/// Claims from any surface can activate or deactivate the most recently shown Review.
pub(crate) async fn refresh_active(app: &App) {
    let (shown, revision) = {
        let mut sessions = app.viewers.0.lock().expect("viewer sessions");
        (sessions.selected.clone(), sessions.advance_activation())
    };
    let Some(shown) = shown else { return };
    if let Ok(active) = read_active(app, Some(shown)).await {
        let mut sessions = app.viewers.0.lock().expect("viewer sessions");
        if sessions.activation == revision && sessions.active != active {
            app.viewers
                .notify(active.as_ref().map(|review| review.shown.clone().into()));
            let hidden = active.is_none();
            sessions.active = active;
            drop(sessions);
            if hidden {
                app.windows.hide_pet(app);
            }
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
                addresses: Vec::new(),
            })
        };
        let (entered, wait_for_older) = tokio::sync::oneshot::channel();
        let (finish, wait_for_newer) = tokio::sync::oneshot::channel();
        let older = sessions.activate(&generation, Some(&first), async {
            entered.send(()).unwrap();
            wait_for_newer.await.unwrap();
            Ok(review(&first))
        });
        let newer = async {
            wait_for_older.await.unwrap();
            sessions
                .activate(
                    &generation,
                    Some(&latest),
                    std::future::ready(Ok(review(&latest))),
                )
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
                checkout: Default::default(),
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

#[cfg(test)]
mod launch_tests {
    use super::*;

    #[test]
    fn a_cancelled_viewer_handoff_never_opens_a_native_window() {
        let (sink, mut events) = log::channel();
        let review: RequestId = "run-1-1".parse().unwrap();
        let run = sink.start(&review, "view", "notes");
        let reviews = crate::human::Reviews::default();
        let job = reviews
            .start_artifact(
                crate::human::Caller::test_main(),
                serde_json::from_str("1").unwrap(),
            )
            .unwrap();
        job.cancel.cancel();
        let pending = PendingLaunch {
            log: run,
            review: Some(review),
            job,
        };
        assert!(pending.open(|| panic!("cancelled before opening")).is_err());
        let start = events.try_recv().unwrap();
        let end = events.try_recv().unwrap();
        assert_eq!(end.run, start.run);
        assert_eq!(end.kind, log::Kind::End(log::End::Cancelled));
    }

    #[test]
    fn a_viewer_handoff_reports_the_native_launch_result_on_the_same_log_run() {
        let (sink, mut events) = log::channel();
        let id: RequestId = "run-1-1".parse().unwrap();
        let failure = Failure::Action("Window could not be created.".into());
        for (result, end) in [
            (Err(failure.clone()), log::End::Failed(failure.to_string())),
            (Ok(()), log::End::Launched),
        ] {
            let run = sink.start(&id, "view", "notes");
            let started = events.try_recv().unwrap();
            assert!(matches!(started.kind, log::Kind::Start { .. }));
            assert!(
                events.try_recv().is_err(),
                "preparation never logs a launch"
            );
            assert_eq!(finish_launch(Some(run), result.clone()), result);
            let ended = events.try_recv().unwrap();
            assert_eq!(ended.run, started.run);
            assert_eq!(ended.kind, log::Kind::End(end));
        }
    }
}
