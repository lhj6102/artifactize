//! The pane's live branches and waiting reviews, and the Run the Main area shows: the
//! library's [`live::Loader`] snapshot, kept as the one UI snapshot every view is drawn from.
//! Clocks redraw from it without a read; a rescan asks Git which worktrees exist now.
mod view;

pub use view::{Attention, BranchKey, BrowseView, Completion, PathKey};
pub(crate) use view::{NodeKind, Segment, TreeNode, row};

use crate::{App, Failure};
use artifactize_core::{
    changes::{Change, Changed},
    monitor::{
        self, Node, Origin,
        live::{self, Branch, Loader, Place, Query, Waiting},
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
    /// This view's share in the Run's memory; last, so it goes after what it covers.
    _footprint: artifactize_core::platform::Footprint,
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
    launch: Option<Place>,
    waiting: Vec<Waiting>,
    /// The number of the request read that read `waiting` ([`crate::human::Reads`]).
    waiting_read: u32,
    unreadable: Vec<Unreadable>,
    /// The pane's search.
    query: Search,
    /// The Run the Main area shows.
    opened: Opened,
    opened_branch: Option<Place>,
    reference: bool,
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

impl Shown {
    fn chip_limit(&self, branch: &Branch) -> usize {
        let selected = self
            .opened
            .id()
            .and_then(|id| branch.timeline().position(|chip| &chip.id == id));
        self.chips
            .get(&branch.place)
            .copied()
            .unwrap_or(CHIPS)
            .max(selected.map_or(0, |at| at + 1))
    }
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
    /// What the change hints since the last load named; the next load reads it again.
    changed: Mutex<Changed>,
}

/// A step of the pane and how to read the state for it.
#[derive(Debug, Clone, Default, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Navigate {
    /// Which Run the Main area shows after this step.
    pub run: RunStep,
    /// The pane's search, when it changed.
    pub query: Option<String>,
    /// Reach further back in this branch's Runs.
    pub older: Option<BranchKey>,
    /// Star a branch, or take its star away: starred branches are listed first.
    pub star: Option<Star>,
    /// Read the waiting reviews again: refreshes and change hints; navigation reuses them.
    pub full: bool,
    /// Read everything again, after a resync hint or a manual refresh.
    pub resync: bool,
}

/// What a step does to the Run shown in the Main area.
#[derive(Debug, Clone, Default, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RunStep {
    /// Leave the shown Run as it is.
    #[default]
    Keep,
    /// Show this Run.
    Open { id: crate::ids::RunKey },
    /// Follow a link, showing this Run's branch even outside the saved search.
    Reference { id: crate::ids::RunKey },
    /// Show no Run: the Main area shows a Current, a review or Settings.
    Close,
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
                launch: None,
                waiting: Vec::new(),
                waiting_read: 0,
                unreadable: Vec::new(),
                query: Search::new(live::DEFAULT.into()),
                opened: Opened::None,
                opened_branch: None,
                reference: false,
                chips: BTreeMap::new(),
                starred: None,
                unstarred: None,
                error: None,
                refreshed: false,
                replaced: false,
                revision: 0,
                shapes: crate::ids::ShapeKey::NONE,
            }),
            changed: Mutex::default(),
        }
    }

    /// Keep what a change hint names for the next load.
    pub fn invalidate(&self, change: &Change) {
        self.changed.lock().expect("browse hints lock").add(change);
    }

    /// What the hints since the last load named, now for this load to read.
    fn take_changed(&self) -> Changed {
        std::mem::take(&mut *self.changed.lock().expect("browse hints lock"))
    }

    /// Give back what a load took and did not use.
    fn restore_changed(&self, changed: Changed) {
        self.changed
            .lock()
            .expect("browse hints lock")
            .merge(changed);
    }

    fn shown(&self) -> std::sync::MutexGuard<'_, Shown> {
        self.shown.lock().expect("browse state lock")
    }

    /// Apply `step` to the shown selection; a star is kept in `state` at once.
    fn navigate(&self, state: &Path, step: &Navigate) {
        let mut shown = self.shown();
        if shown.starred.is_none() || step.resync {
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
        match &step.run {
            RunStep::Keep => {}
            RunStep::Open { id } | RunStep::Reference { id } => {
                shown.reference = matches!(step.run, RunStep::Reference { .. });
                let id: RunId = id.clone().into_inner();
                if shown.opened.id() != Some(&id) {
                    shown.opened = Opened::Loading(id);
                }
            }
            RunStep::Close => {
                shown.reference = false;
                shown.opened = Opened::None;
            }
        }
        if let Some(query) = &step.query
            && *query != shown.query.text
        {
            shown.query = Search::new(query.clone());
            // A reference in the same request takes precedence over the new filter.
            if !matches!(step.run, RunStep::Reference { .. }) {
                shown.reference = false;
            }
        }
        if let Some(branch) = &step.older {
            let place = branch.to_place();
            let visible = shown
                .branches
                .iter()
                .find(|branch| branch.place == place)
                .map_or(CHIPS, |branch| shown.chip_limit(branch));
            let more = visible.saturating_add(CHIPS);
            shown.chips.insert(place, more);
        }
        shown.revision += 1;
    }

    /// Open the named Run without waiting for the Pane's next catalog or Review refresh.
    async fn open_reference(&self, state: &Path, id: RunId) -> Result<Option<Origin>, Failure> {
        let mut loader = self.loader.lock().await;
        let worker = std::mem::take(&mut *loader);
        let path = state.to_owned();
        let wanted = id.clone();
        let task = tokio::task::spawn_blocking(move || {
            let mut worker = worker;
            let read = worker.open(&path, &wanted).map(|reference| {
                let opened = reference.opened.map(|opened| {
                    let (run, requests, tree, footprint) = *opened;
                    let graph = monitor::graph(&tree);
                    (run, requests, tree, graph, footprint)
                });
                (reference.origin, opened, reference.branch)
            });
            (worker, read)
        });
        let (worker, read) = crate::failure::joined(task.await)?;
        *loader = worker;
        let mut shown = self.shown();
        if shown.opened.id() != Some(&id) {
            return Ok(None);
        }
        let (origin, opened, branch) = match read {
            Ok(read) => read,
            Err(error) => {
                shown.error = Some(format!("read failed: {error}"));
                return Err(Failure::Read(error));
            }
        };
        if origin.replaces(&shown.origin) {
            shown.replaced = true;
        }
        shown.origin = origin.clone();
        shown.opened_branch = branch;
        shown.opened = if let Some((run, requests, tree, graph, footprint)) = opened {
            let shape = shown
                .opened
                .loaded()
                .filter(|kept| kept.graph.same_shape(&graph))
                .map(|kept| kept.shape)
                .unwrap_or_else(|| shown.shapes.next());
            Opened::Loaded(Arc::new(Selected {
                run,
                requests,
                tree,
                graph,
                shape,
                _footprint: footprint,
            }))
        } else {
            Opened::Missing(id)
        };
        shown.error = None;
        shown.revision += 1;
        Ok(Some(origin))
    }

    /// Read a snapshot for the shown selection on a blocking thread and accept it, unless the
    /// shown Run changed meanwhile; that navigation's own load reads it then. `full` reads the
    /// waiting reviews again and `rescan` asks Git which worktrees exist. Returns the accepted
    /// snapshot's origin, or `None` when no snapshot was taken.
    async fn load(
        &self,
        state: &Path,
        reviewer: &artifactize_core::types::ReviewerId,
        reads: &crate::human::Reads,
        read: Read,
    ) -> Result<Option<Origin>, Failure> {
        let mut loader = self.loader.lock().await;
        let mut changed = self.take_changed();
        if read.resync {
            changed = Changed::Everything;
        }
        let (open, input, before) = {
            let shown = self.shown();
            let input = live::Input {
                state: state.to_path_buf(),
                launch: self.launch.clone(),
                open: shown.opened.id().cloned(),
                force: shown.opened.loaded().is_none() || shown.error.is_some(),
                rescan: read.rescan || !shown.initialized,
                changed: changed.clone(),
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
            let result = worker.load(input).map(|snapshot| {
                snapshot.map_open_run(|(run, requests, tree, footprint)| {
                    let graph = monitor::graph(&tree);
                    // The same Run with the same Artifacts and edges keeps its shape.
                    let kept = before
                        .as_ref()
                        .filter(|before| {
                            before.run.run.id == run.run.id && before.graph.same_shape(&graph)
                        })
                        .map(|before| before.shape);
                    (run, requests, tree, graph, kept, footprint)
                })
            });
            (worker, result)
        })
        .await;
        let (worker, result) = crate::failure::joined(joined)?;
        *loader = worker;
        let snapshot = match result {
            Ok(read) => read,
            Err(error) => {
                self.restore_changed(changed);
                self.shown().error = Some(format!("read failed: {error}"));
                return Err(Failure::Read(error));
            }
        };
        // The waiting reviews' requests are read in turn with the reviews' own reads, so the
        // views they freshen are ordered with them.
        let (waiting, waiting_error) = if read.full || !self.shown().initialized {
            let (number, waiting) = reads
                .numbered(live::waiting(state, reviewer, &snapshot.labels))
                .await;
            match waiting {
                Ok(waiting) => (Some((number, waiting)), None),
                Err(error) => (
                    None,
                    Some(format!("Waiting reviews could not be read: {error}")),
                ),
            }
        } else {
            (None, None)
        };
        let mut shown = self.shown();
        if shown.opened.id() != open.as_ref() {
            // The newly shown Run's own load reads it; what this one read stays cached.
            drop(shown);
            self.restore_changed(changed);
            return Ok(None);
        }
        let changed = read.full
            || !shown.initialized
            || matches!(snapshot.open_run, live::OpenRun::Loaded(_))
            || (matches!(snapshot.open_run, live::OpenRun::Absent)
                && open.is_some()
                && !matches!(shown.opened, Opened::Missing(_)))
            || shown.branches != snapshot.branches;
        if snapshot.origin.replaces(&shown.origin) {
            if let Some(id) = &open {
                shown.opened = Opened::Loading(id.clone());
            }
            shown.replaced = true;
        }
        let origin = snapshot.origin.clone();
        shown.origin = snapshot.origin;
        shown.opened_branch = open.as_ref().and_then(|id| {
            snapshot
                .branches
                .iter()
                .find(|branch| branch.runs.iter().any(|chip| &chip.id == id))
                .map(|branch| branch.place.clone())
        });
        shown.branches = snapshot.branches;
        shown.launch = snapshot.launch;
        shown.unreadable = snapshot.unreadable;
        if let Some((number, waiting)) = waiting {
            shown.waiting = waiting;
            shown.waiting_read = number;
        }
        match snapshot.open_run {
            live::OpenRun::Loaded(run) => {
                let (run, requests, tree, graph, kept, footprint) = *run;
                let shape = kept.unwrap_or_else(|| shown.shapes.next());
                shown.opened = Opened::Loaded(Arc::new(Selected {
                    run,
                    requests,
                    tree,
                    graph,
                    shape,
                    _footprint: footprint,
                }));
            }
            live::OpenRun::Absent => {
                let unreadable = open.as_ref().is_some_and(|id| {
                    shown
                        .unreadable
                        .iter()
                        .any(|record| record.kind == "run" && record.id == id.as_str())
                });
                if !unreadable || shown.opened.loaded().is_none() {
                    shown.opened = open.map_or(Opened::None, Opened::Missing);
                }
            }
            live::OpenRun::Unchanged => {}
        }
        shown.initialized = true;
        shown.error = waiting_error;
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

    /// The waiting reviews' requests as the snapshot read them, and that read's number.
    fn waiting_views(&self) -> (u32, Vec<RequestView>) {
        let shown = self.shown();
        let views = shown
            .waiting
            .iter()
            .map(|waiting| waiting.view.clone())
            .collect();
        (shown.waiting_read, views)
    }

    /// The pane's completion of `query`'s last term, from what the snapshot lists.
    fn complete(&self, query: &str) -> artifactize_core::review::search::Completion {
        let shown = self.shown();
        live::complete(query, &shown.branches, &[])
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
    if navigate.resync {
        crate::watch::reconcile_runs(&app).await;
    }
    app.browse.navigate(&app.launch.state, &navigate);
    let read = Read {
        full: navigate.full,
        resync: navigate.resync,
        // A refresh scans the branches again too.
        rescan: navigate.resync,
    };
    let accepted = match navigate.run {
        RunStep::Open { id } | RunStep::Reference { id } if !read.full && !read.resync => {
            let id = id.into_inner();
            let origin = app
                .browse
                .open_reference(&app.launch.state, id.clone())
                .await?;
            let needs_catalog = {
                let shown = app.browse.shown();
                matches!(shown.opened, Opened::Missing(_))
                    || shown.opened_branch.as_ref().is_none_or(|place| {
                        !shown.branches.iter().any(|branch| {
                            &branch.place == place && branch.runs.iter().any(|chip| chip.id == id)
                        })
                    })
            };
            if needs_catalog {
                app.browse
                    .load(
                        &app.launch.state,
                        &app.launch.reviewer,
                        &app.human.reads,
                        Read {
                            rescan: true,
                            ..Read::default()
                        },
                    )
                    .await?
                    .or(origin)
            } else {
                origin
            }
        }
        _ => {
            app.browse
                .load(
                    &app.launch.state,
                    &app.launch.reviewer,
                    &app.human.reads,
                    read,
                )
                .await?
        }
    };
    if let Some(origin) = accepted {
        let generation = app.observe(origin);
        // A review opened from the Waiting stack shows the claims and states it lists, unless
        // it read them since or a newer snapshot replaced this state.
        app.generation.if_current(generation, || {
            let (number, views) = app.browse.waiting_views();
            app.human.remember(generation, number, views);
        });
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
    if let Some(origin) = app
        .browse
        .load(
            &app.launch.state,
            &app.launch.reviewer,
            &app.human.reads,
            read,
        )
        .await?
    {
        app.observe(origin);
    }
    Ok(app.browse.shown().revision != before)
}
