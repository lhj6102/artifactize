//! The pane's live branches and waiting reviews, and the Run the Main area shows: the
//! library's [`live::Loader`] snapshot, kept as the one UI snapshot every view is drawn from.
//! Clocks redraw from it without a read; a rescan asks Git which worktrees exist now.
mod view;

pub use view::{BranchKey, BrowseView, Completion, PathKey};
pub(crate) use view::{NodeKind, Segment, TreeNode};

use crate::{App, Failure};
use artifactize_core::{
    monitor::{
        Node, Origin,
        live::{self, Branch, Labels, Loader, Place, Waiting},
    },
    store::{RequestView, RunView, Unreadable},
    types::RunId,
};
use serde::Deserialize;
use specta::Type;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use time::OffsetDateTime;

/// The shown Run with its requests and projected tree.
pub type Selected = (RunView, Vec<RequestView>, Arc<Vec<Node>>);

/// Chips a branch's row holds at first, and how many more each reach of its oldest end adds:
/// a row of years of Runs stays a small view.
const CHIPS: usize = 20;

/// What the last accepted snapshot shows.
struct Shown {
    initialized: bool,
    origin: Origin,
    branches: Vec<Branch>,
    labels: Labels,
    launch: Option<Place>,
    waiting: Vec<Waiting>,
    unreadable: Vec<Unreadable>,
    /// The pane's search, as typed.
    query: String,
    /// The Run the Main area shows, and its records once read.
    open: Option<RunId>,
    run: Option<Selected>,
    /// The shown Run no longer exists or cannot be read.
    missing: bool,
    /// Chips shown of each branch whose row reached further back than [`CHIPS`].
    chips: BTreeMap<Place, usize>,
    error: Option<String>,
    refreshed: bool,
    /// The state was replaced since the frontend last drew; it drops Details and drafts.
    replaced: bool,
    /// Counts every change of what is shown, so the frontend can order responses and know when
    /// views derived from it are stale.
    revision: u32,
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
    /// Read the waiting reviews again: refreshes and change hints; navigation reuses them.
    pub full: bool,
    /// Drop every cache first, after a resync hint or a manual refresh.
    pub resync: bool,
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
                query: live::DEFAULT.into(),
                open: None,
                run: None,
                missing: false,
                chips: BTreeMap::new(),
                error: None,
                refreshed: false,
                replaced: false,
                revision: 0,
            }),
        }
    }

    fn shown(&self) -> std::sync::MutexGuard<'_, Shown> {
        self.shown.lock().expect("browse state lock")
    }

    /// Apply `step` to the shown selection.
    fn navigate(&self, step: &Navigate) {
        let mut shown = self.shown();
        if let Some(id) = step.run.clone() {
            let id: RunId = id.into_inner();
            if shown.open.as_ref() != Some(&id) {
                shown.open = Some(id);
                shown.run = None;
                shown.missing = false;
            }
        } else if step.close {
            shown.open = None;
            shown.run = None;
            shown.missing = false;
        }
        if let Some(query) = &step.query {
            shown.query.clone_from(query);
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
        let (open, input) = {
            let shown = self.shown();
            let input = live::Input {
                state: state.to_path_buf(),
                launch: self.launch.clone(),
                open: shown.open.clone(),
                force: shown.run.is_none(),
                rescan: read.rescan || !shown.initialized,
            };
            (shown.open.clone(), input)
        };
        let worker = std::mem::take(&mut *loader);
        let joined = tokio::task::spawn_blocking(move || {
            let mut worker = worker;
            let result = worker.load(input);
            (worker, result)
        })
        .await;
        let (worker, result) = crate::failure::joined(joined)?;
        *loader = worker;
        let snapshot = match result {
            Ok(snapshot) => snapshot,
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
        if shown.open != open {
            loader.reset();
            return Ok(None);
        }
        let changed = read.full
            || !shown.initialized
            || snapshot.opened.is_some()
            || shown.branches != snapshot.branches;
        if snapshot.origin.replaces(&shown.origin) {
            shown.run = None;
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
        match (snapshot.opened, snapshot.open) {
            (Some(opened), _) => shown.run = Some(opened),
            (None, false) => shown.run = None,
            (None, true) => {}
        }
        shown.missing = shown.open.is_some() && !snapshot.open;
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

    /// The shown Run's tree, as the shown snapshot projected it.
    pub fn with_tree<T>(&self, run: &RunId, read: impl FnOnce(&[Node]) -> T) -> Result<T, Failure> {
        let shown = self.shown();
        match &shown.run {
            Some((view, _, nodes)) if &view.run.id == run => Ok(read(nodes)),
            _ => Err(Failure::Missing(format!(
                "Run {run} is no longer shown; choose it again."
            ))),
        }
    }

    /// The shown Run's requests and target, for a Detail of it.
    pub fn with_run<T>(
        &self,
        run: &RunId,
        read: impl FnOnce(&RunView, &[RequestView]) -> T,
    ) -> Result<T, Failure> {
        let shown = self.shown();
        match &shown.run {
            Some((view, requests, _)) if &view.run.id == run => Ok(read(view, requests)),
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
    app.browse.navigate(&navigate);
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
