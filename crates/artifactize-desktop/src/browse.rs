//! Scope → Runs → Run tree: the monitor's snapshot, read through its [`Loader`] and kept as
//! the one UI snapshot every view is drawn from. Clocks redraw from it without a read.
mod view;

pub use view::{BrowseView, RepositoryKey, ScopeKey};
pub(crate) use view::{NodeKind, TreeNode};

use crate::{App, Failure};
use artifactize_core::{
    monitor::{self, Catalog, Input, Loader, Node, Origin, Page, Scope},
    store::{RequestView, RunSummary, RunView},
    types::RunId,
};
use serde::Deserialize;
use specta::Type;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use time::OffsetDateTime;

/// The selected Run with its requests and projected tree.
pub type Selected = (RunView, Vec<RequestView>, Arc<Vec<Node>>);

/// What the last accepted snapshot shows and where navigation points next.
struct Shown {
    catalog: Catalog,
    initialized: bool,
    origin: Origin,
    page: Page,
    runs: Vec<RunSummary>,
    run: Option<Selected>,
    error: Option<String>,
    refreshed: bool,
    /// The state was replaced since the frontend last drew; it drops Details and drafts.
    replaced: bool,
    /// Counts every change of what is shown (navigation, accepted reads, folds), so the
    /// frontend can order responses and know when views derived from it are stale.
    revision: u32,
}

/// The snapshot worker and the shown snapshot. Loads are serialized by the loader lock;
/// the shown snapshot is only locked for short, non-blocking updates.
pub struct Browse {
    initial: Option<PathBuf>,
    loader: tokio::sync::Mutex<Loader>,
    shown: Mutex<Shown>,
}

/// Which way to page through the selected scope's Runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Paging {
    Older,
    Newer,
}

/// A navigation step and how to read the state for it.
#[derive(Debug, Clone, Default, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Navigate {
    /// Select this scope; its Runs start again from the newest page.
    pub scope: Option<ScopeKey>,
    /// Select this Run of the shown page.
    pub run: Option<crate::ids::RunKey>,
    pub paging: Option<Paging>,
    /// Read the catalog afresh: refreshes and change hints; navigation reuses it.
    pub full: bool,
    /// Drop every cache first, after a resync hint.
    pub resync: bool,
}

impl Browse {
    pub fn new(initial: Option<PathBuf>) -> Self {
        Self {
            initial,
            loader: tokio::sync::Mutex::new(Loader::default()),
            shown: Mutex::new(Shown {
                catalog: Catalog::default(),
                initialized: false,
                origin: Origin::default(),
                page: Page {
                    scope: Scope::All,
                    open: None,
                    offset: 0,
                    tail: false,
                    limit: monitor::PAGE,
                },
                runs: Vec::new(),
                run: None,
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

    /// Apply `step` to the shown selection, as the monitor's keys do.
    fn navigate(&self, step: &Navigate) -> Result<(), Failure> {
        let mut shown = self.shown();
        let before = (shown.page.clone(), shown.runs.len());
        let result = Self::step(&mut shown, step);
        if (shown.page.clone(), shown.runs.len()) != before {
            shown.revision += 1;
        }
        result
    }

    fn step(shown: &mut Shown, step: &Navigate) -> Result<(), Failure> {
        if let Some(key) = &step.scope {
            let scope = key.to_scope();
            if shown.page.scope != scope {
                shown.page = Page {
                    scope,
                    open: None,
                    offset: 0,
                    tail: false,
                    limit: monitor::PAGE,
                };
                shown.runs.clear();
                shown.run = None;
            }
        }
        if let Some(id) = step.run.clone() {
            let id: RunId = id.into_inner();
            let readable = shown
                .runs
                .iter()
                .find(|run| run.id == id)
                .is_some_and(|run| run.readable());
            shown.page.open = readable.then_some(id);
        }
        let limit = shown.page.limit;
        match step.paging {
            Some(Paging::Older) => {
                let Some(offset) =
                    monitor::older_offset(shown.page.offset, limit, shown.runs.len() as u32)
                else {
                    return Ok(());
                };
                shown.page.offset = offset;
                shown.page.tail = false;
            }
            Some(Paging::Newer) => {
                let Some(offset) = monitor::newer_offset(shown.page.offset, limit) else {
                    return Ok(());
                };
                shown.page.offset = offset;
                shown.page.tail = true;
            }
            None => return Ok(()),
        }
        shown.runs.clear();
        shown.page.open = None;
        shown.run = None;
        Ok(())
    }

    /// Read a snapshot for the shown selection on a blocking thread and accept it, unless
    /// navigation moved on meanwhile; the newer navigation's own load reads it then.
    /// Returns the accepted snapshot's origin, or `None` when no snapshot was taken.
    async fn load(
        &self,
        state: &Path,
        full: bool,
        resync: bool,
    ) -> Result<Option<Origin>, Failure> {
        let mut loader = self.loader.lock().await;
        if resync {
            loader.reset();
        }
        let (page, input) = {
            let shown = self.shown();
            let input = Input {
                state: state.to_path_buf(),
                initial: self.initial.clone(),
                catalog: shown.catalog.clone(),
                initialized: shown.initialized,
                page: shown.page.clone(),
                full,
                displayed: shown.run.as_ref().map(|(run, _, _)| run.run.id.clone()),
                runs: (!full && !shown.runs.is_empty()).then(|| shown.runs.clone()),
            };
            (shown.page.clone(), input)
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
        let mut shown = self.shown();
        if shown.page != page {
            loader.reset();
            return Ok(None);
        }
        let snapshot = match result {
            Ok(snapshot) => snapshot,
            Err(error) => {
                shown.error = Some(format!("read failed: {error}"));
                return Err(Failure::Read(error));
            }
        };
        if snapshot.origin.replaces(&shown.origin) {
            shown.run = None;
            shown.replaced = true;
        }
        let origin = snapshot.origin.clone();
        shown.origin = snapshot.origin;
        shown.catalog = snapshot.catalog;
        shown.page.scope = snapshot.scope;
        shown.initialized = true;
        shown.page.tail = false;
        shown.runs = snapshot.runs;
        shown.page.open = snapshot
            .selected_id
            .filter(|id| shown.runs.iter().any(|run| &run.id == id && run.readable()));
        match (snapshot.selected, &shown.page.open) {
            (Some(selected), _) => shown.run = Some(selected),
            (None, None) => shown.run = None,
            (None, Some(_)) => {}
        }
        let unreadable: Vec<_> = shown
            .runs
            .iter()
            .flat_map(|run| &run.unreadable)
            .map(ToString::to_string)
            .collect();
        shown.error = (!unreadable.is_empty()).then(|| unreadable.join("; "));
        shown.refreshed = true;
        shown.revision += 1;
        Ok(Some(origin))
    }

    /// The shown snapshot as of `now`. Taking it clears the replaced flag.
    fn view(&self, now: OffsetDateTime) -> BrowseView {
        let mut shown = self.shown();
        let view = view::build(&shown, now);
        shown.replaced = false;
        view
    }

    /// The selected Run's tree, as the shown snapshot projected it.
    pub fn with_tree<T>(
        &self,
        run: &artifactize_core::types::RunId,
        read: impl FnOnce(&[Node]) -> T,
    ) -> Result<T, Failure> {
        let shown = self.shown();
        match &shown.run {
            Some((view, _, nodes)) if &view.run.id == run => Ok(read(nodes)),
            _ => Err(Failure::Missing(format!(
                "Run {run} is no longer selected; select it again."
            ))),
        }
    }

    /// The selected Run's requests and target, for a Detail of it.
    pub fn with_run<T>(
        &self,
        run: &artifactize_core::types::RunId,
        read: impl FnOnce(&RunView, &[RequestView]) -> T,
    ) -> Result<T, Failure> {
        let shown = self.shown();
        match &shown.run {
            Some((view, requests, _)) if &view.run.id == run => Ok(read(view, requests)),
            _ => Err(Failure::Missing(format!(
                "Run {run} is no longer selected; select it again."
            ))),
        }
    }
}

/// Navigate, read the state and return what to show.
#[tauri::command]
#[specta::specta]
pub async fn browse(app: tauri::State<'_, App>, navigate: Navigate) -> Result<BrowseView, Failure> {
    app.browse.navigate(&navigate)?;
    let accepted = app
        .browse
        .load(&app.launch.state, navigate.full, navigate.resync)
        .await?;
    if let Some(origin) = accepted {
        app.observe(origin);
    }
    Ok(app.browse.view(OffsetDateTime::now_utc()))
}

/// Redraw ages and elapsed times from the shown snapshot, without reading the state.
#[tauri::command]
#[specta::specta]
pub fn redraw(app: tauri::State<'_, App>) -> BrowseView {
    app.browse.view(OffsetDateTime::now_utc())
}

/// Show or hide a repository's worktrees without Runs; no state read.
#[tauri::command]
#[specta::specta]
pub fn toggle_worktrees(app: tauri::State<'_, App>, repository: RepositoryKey) -> BrowseView {
    {
        let mut shown = app.browse.shown();
        shown.catalog.toggle(&repository.to_repository());
        shown.catalog.rebuild();
        shown.revision += 1;
    }
    app.browse.view(OffsetDateTime::now_utc())
}
