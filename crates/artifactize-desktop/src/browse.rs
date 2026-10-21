//! The pane's live branches and waiting reviews, and the Run the Main area shows: the
//! library's [`live::Loader`] snapshot, kept as the one UI snapshot every view is drawn from.
//! Clocks redraw from it without a read; a rescan asks Git which worktrees exist now.
mod view;

pub use view::{BranchKey, BrowseView, Completion, PathKey};
pub(crate) use view::{NodeKind, Segment, TreeNode, row};

use crate::{App, Failure};
use artifactize_core::{
    monitor::{
        self, Node, Origin,
        live::{self, Branch, Labels, Loader, Place, Query, Waiting},
    },
    store::{RequestView, RunView, Unreadable},
    types::RunId,
};
use serde::Deserialize;
use specta::Type;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use time::OffsetDateTime;

/// The shown Run with its requests, projected tree and graph, shared by every view drawn
/// from it so that none holds the shown snapshot's lock while it builds.
pub struct Selected {
    pub run: RunView,
    pub requests: Vec<RequestView>,
    pub tree: Arc<Vec<Node>>,
    pub graph: monitor::Graph,
    /// Names the graph's shape: it stays while only lights change.
    pub shape: crate::ids::ShapeKey,
}

/// Chips a branch's row holds at first, and how many more each reach of its oldest end adds:
/// a row of years of Runs stays a small view.
const CHIPS: usize = 20;

/// The Run the Main area shows: none, chosen and not read yet, read, or gone.
enum Opened {
    None,
    Loading(RunId),
    Loaded(Arc<Selected>),
    /// The Run no longer exists or cannot be read.
    Missing(RunId),
}

impl Opened {
    fn id(&self) -> Option<&RunId> {
        match self {
            Self::None => None,
            Self::Loading(id) | Self::Missing(id) => Some(id),
            Self::Loaded(selected) => Some(&selected.run.run.id),
        }
    }

    fn loaded(&self) -> Option<&Arc<Selected>> {
        match self {
            Self::Loaded(selected) => Some(selected),
            _ => None,
        }
    }
}

/// What the last accepted snapshot shows.
struct Shown {
    initialized: bool,
    origin: Origin,
    branches: Vec<Branch>,
    labels: Labels,
    launch: Option<Place>,
    waiting: Vec<Waiting>,
    unreadable: Vec<Unreadable>,
    /// The pane's search.
    query: Search,
    /// The Run the Main area shows.
    opened: Opened,
    /// Chips shown of each branch whose row reached further back than [`CHIPS`].
    chips: BTreeMap<Place, usize>,
    /// The starred branches, once read from the state directory.
    starred: Option<BTreeSet<Place>>,
    /// Why the starred branches could not be read or kept.
    unstarred: Option<String>,
    error: Option<String>,
    refreshed: bool,
    /// The state was replaced since the frontend last drew; it drops Details and drafts.
    replaced: bool,
    /// Counts every change of what is shown, so the frontend can order responses and know when
    /// views derived from it are stale.
    revision: u32,
    /// The last graph shape handed out.
    shapes: crate::ids::ShapeKey,
}

/// The pane's search: its text as typed, and the query it parses to or why it does not, parsed
/// once when navigation accepts it.
struct Search {
    text: String,
    parsed: Result<Query, String>,
}

impl Search {
    fn new(text: String) -> Self {
        let parsed = text.parse();
        Self { text, parsed }
    }
}

/// The snapshot worker and the shown snapshot. Loads are serialized by the loader lock; the
/// shown snapshot is only locked for short, non-blocking updates.
pub struct Browse {
    launch: Option<PathBuf>,
    loader: tokio::sync::Mutex<Loader>,
    shown: Mutex<Shown>,
}

/// A step of the pane and how to read the state for it.
#[derive(Debug, Clone, Default, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Navigate {
    /// Show this Run in the Main area.
    pub run: Option<crate::ids::RunKey>,
    /// Show no Run: the Main area shows a Current, a review or Settings.
    pub close: bool,
    /// The pane's search, when it changed.
    pub query: Option<String>,
    /// Reach further back in this branch's Runs.
    pub older: Option<BranchKey>,
    /// Star a branch, or take its star away: starred branches are listed first.
    pub star: Option<Star>,
    /// Read the waiting reviews again: refreshes and change hints; navigation reuses them.
    pub full: bool,
    /// Drop every cache first, after a resync hint or a manual refresh.
    pub resync: bool,
}

/// A branch starred, or its star taken away.
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Star {
    pub branch: BranchKey,
    pub on: bool,
}

impl Browse {
    pub fn new(launch: Option<PathBuf>) -> Self {
        Self {
            launch,
            loader: tokio::sync::Mutex::new(Loader::default()),
            shown: Mutex::new(Shown {
                initialized: false,
                origin: Origin::default(),
                branches: Vec::new(),
                labels: Labels::new(),
                launch: None,
                waiting: Vec::new(),
                unreadable: Vec::new(),
                query: Search::new(live::DEFAULT.into()),
                opened: Opened::None,
                chips: BTreeMap::new(),
                starred: None,
                unstarred: None,
                error: None,
                refreshed: false,
                replaced: false,
                revision: 0,
                shapes: crate::ids::ShapeKey::NONE,
            }),
        }
    }

    fn shown(&self) -> std::sync::MutexGuard<'_, Shown> {
        self.shown.lock().expect("browse state lock")
    }

    /// Apply `step` to the shown selection; a star is kept in `state` at once.
    fn navigate(&self, state: &Path, step: &Navigate) {
        let mut shown = self.shown();
        if shown.starred.is_none() {
            let read = live::starred(Some(state));
            shown.unstarred = read.as_ref().err().cloned();
            shown.starred = Some(read.unwrap_or_default());
        }
        if let Some(star) = &step.star {
            match live::star(Some(state), &star.branch.to_place(), star.on) {
                Ok(kept) => {
                    shown.starred = Some(kept);
                    shown.unstarred = None;
                }
                Err(error) => shown.unstarred = Some(error),
            }
        }
        if let Some(id) = step.run.clone() {
            let id: RunId = id.into_inner();
            if shown.opened.id() != Some(&id) {
                shown.opened = Opened::Loading(id);
            }
        } else if step.close {
            shown.opened = Opened::None;
        }
        if let Some(query) = &step.query
            && *query != shown.query.text
        {
            shown.query = Search::new(query.clone());
        }
        if let Some(branch) = &step.older {
            let place = branch.to_place();
            let more = shown.chips.get(&place).copied().unwrap_or(CHIPS) + CHIPS;
            shown.chips.insert(place, more);
        }
        shown.revision += 1;
    }

    /// Read a snapshot for the shown selection on a blocking thread and accept it, unless the
    /// shown Run changed meanwhile; that navigation's own load reads it then. `full` reads the
    /// waiting reviews again and `rescan` asks Git which worktrees exist. Returns the accepted
    /// snapshot's origin, or `None` when no snapshot was taken.
    async fn load(
        &self,
        state: &Path,
        reviewer: &artifactize_core::types::ReviewerId,
        read: Read,
    ) -> Result<Option<Origin>, Failure> {
        let mut loader = self.loader.lock().await;
        if read.resync {
            loader.reset();
        }
        let (open, input, before) = {
            let shown = self.shown();
            let input = live::Input {
                state: state.to_path_buf(),
                launch: self.launch.clone(),
                open: shown.opened.id().cloned(),
                force: shown.opened.loaded().is_none(),
                rescan: read.rescan || !shown.initialized,
            };
            (
                shown.opened.id().cloned(),
                input,
                shown.opened.loaded().cloned(),
            )
        };
        let worker = std::mem::take(&mut *loader);
        let joined = tokio::task::spawn_blocking(move || {
            let mut worker = worker;
            // The graph is drawn from every accepted snapshot: it is built here, off the
            // window's thread, along with the tree.
            let result = worker.load(input).map(|mut snapshot| {
                let selected = snapshot.opened.take().map(|(run, requests, tree)| {
                    let graph = monitor::graph(&tree);
                    // The same Run with the same Artifacts and edges keeps its shape.
                    let kept = before
                        .as_ref()
                        .filter(|before| {
                            before.run.run.id == run.run.id && before.graph.same_shape(&graph)
                        })
                        .map(|before| before.shape);
                    (run, requests, tree, graph, kept)
                });
                (snapshot, selected)
            });
            (worker, result)
        })
        .await;
        let (worker, result) = crate::failure::joined(joined)?;
        *loader = worker;
        let (snapshot, selected) = match result {
            Ok(read) => read,
            Err(error) => {
                self.shown().error = Some(format!("read failed: {error}"));
                return Err(Failure::Read(error));
            }
        };
        let waiting = if read.full || !self.shown().initialized {
            Some(
                live::waiting(state, reviewer, &snapshot.labels)
                    .await
                    .map_err(Failure::Read)?,
            )
        } else {
            None
        };
        let mut shown = self.shown();
        if shown.opened.id() != open.as_ref() {
            loader.reset();
            return Ok(None);
        }
        let changed = read.full
            || !shown.initialized
            || selected.is_some()
            || shown.branches != snapshot.branches;
        if snapshot.origin.replaces(&shown.origin) {
            if let Some(id) = &open {
                shown.opened = Opened::Loading(id.clone());
            }
            shown.replaced = true;
        }
        let origin = snapshot.origin.clone();
        shown.origin = snapshot.origin;
        shown.branches = snapshot.branches;
        shown.labels = snapshot.labels;
        shown.launch = snapshot.launch;
        shown.unreadable = snapshot.unreadable;
        if let Some(waiting) = waiting {
            shown.waiting = waiting;
        }
        match (selected, snapshot.open, open) {
            (Some((run, requests, tree, graph, kept)), _, _) => {
                let shape = kept.unwrap_or_else(|| shown.shapes.next());
                shown.opened = Opened::Loaded(Arc::new(Selected {
                    run,
                    requests,
                    tree,
                    graph,
                    shape,
                }));
            }
            (None, false, Some(id)) => shown.opened = Opened::Missing(id),
            (None, false, None) => shown.opened = Opened::None,
            (None, true, _) => {}
        }
        shown.initialized = true;
        shown.error = None;
        shown.refreshed = true;
        if changed {
            shown.revision += 1;
        }
        Ok(Some(origin))
    }

    /// The shown snapshot as of `now`. Taking it clears the replaced flag.
    fn view(&self, now: OffsetDateTime) -> BrowseView {
        let mut shown = self.shown();
        let view = view::build(&shown, now);
        shown.replaced = false;
        view
    }

    /// The shown Run `run` as the shown snapshot holds it; views build from it without
    /// holding the snapshot's lock.
    pub fn selected(&self, run: &RunId) -> Result<Arc<Selected>, Failure> {
        match self.shown().opened.loaded() {
            Some(selected) if &selected.run.run.id == run => Ok(selected.clone()),
            _ => Err(Failure::Missing(format!(
                "Run {run} is no longer shown; choose it again."
            ))),
        }
    }

    /// The listed branch at `place`: where its Current is scanned and its tools run.
    pub fn branch(&self, place: &Place) -> Result<Branch, Failure> {
        self.shown()
            .branches
            .iter()
            .find(|branch| &branch.place == place)
            .cloned()
            .ok_or_else(|| Failure::Missing("This branch no longer exists; choose another.".into()))
    }

    /// The waiting reviews' requests as the snapshot read them.
    fn waiting_views(&self) -> Vec<RequestView> {
        let shown = self.shown();
        shown
            .waiting
            .iter()
            .map(|waiting| waiting.view.clone())
            .collect()
    }

    /// The pane's completion of `query`'s last term, from what the snapshot lists.
    fn complete(&self, query: &str) -> artifactize_core::review::search::Completion {
        let shown = self.shown();
        live::complete(query, &shown.branches, &shown.waiting)
    }
}

/// How one load reads the state.
#[derive(Debug, Clone, Copy, Default)]
struct Read {
    full: bool,
    resync: bool,
    rescan: bool,
}

/// Navigate, read the state and return what to show.
#[tauri::command]
#[specta::specta]
pub async fn browse(app: tauri::State<'_, App>, navigate: Navigate) -> Result<BrowseView, Failure> {
    app.browse.navigate(&app.launch.state, &navigate);
    let read = Read {
        full: navigate.full,
        resync: navigate.resync,
        // A refresh scans the branches again too.
        rescan: navigate.resync,
    };
    let accepted = app
        .browse
        .load(&app.launch.state, &app.launch.reviewer, read)
        .await?;
    if let Some(origin) = accepted {
        app.observe(origin);
        // A review opened from the Waiting stack shows the claims and states it lists.
        app.human.remember(app.browse.waiting_views());
    }
    Ok(app.browse.view(OffsetDateTime::now_utc()))
}

/// Redraw ages and running times from the shown snapshot, without reading the state.
#[tauri::command]
#[specta::specta]
pub fn redraw(app: tauri::State<'_, App>) -> BrowseView {
    app.browse.view(OffsetDateTime::now_utc())
}

/// Completion candidates for the pane search's last term.
#[tauri::command]
#[specta::specta]
pub fn pane_complete(app: tauri::State<'_, App>, query: String) -> Completion {
    let completion = app.browse.complete(&query);
    Completion {
        keep: completion.keep,
        candidates: completion.candidates,
    }
}

/// Ask Git which worktrees exist now: whether the branches the pane lists changed.
pub(crate) async fn rescan(app: &App) -> Result<bool, Failure> {
    let before = app.browse.shown().revision;
    let read = Read {
        rescan: true,
        ..Read::default()
    };
    app.browse
        .load(&app.launch.state, &app.launch.reviewer, read)
        .await?;
    Ok(app.browse.shown().revision != before)
}
