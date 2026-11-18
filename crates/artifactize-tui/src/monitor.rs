//! The terminal Monitor: Repository/worktree -> Run -> artifacts/evals -> Detail, as reactive
//! panes, with embedded single-request Human review jobs.
#[cfg(test)]
mod catalog_tests;
mod detail;
mod folds;
mod help;
pub(crate) mod input;
mod keys;
pub(crate) mod layout;
#[cfg(test)]
mod layout_parity_tests;
mod loading;
#[cfg(test)]
mod loading_tests;
#[cfg(test)]
mod pane_tests;
#[cfg(test)]
mod pty_fixture_tests;
#[cfg(test)]
mod redesign_tests;
mod rows;
#[cfg(test)]
pub(crate) mod session_tests;
mod terminal;
#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
mod tree_tests;
mod view;
pub use artifactize_core::monitor::*;
pub(crate) use rows::{fit, width};
pub use terminal::run;
pub(crate) use view::{clock, crumbs, section_lines, transcript_lines};

#[cfg(test)]
use self::detail::Content;
pub use self::detail::DetailPane;
use self::detail::Peek;
pub(crate) use self::detail::SessionView;
use crate::{
    review::{self, Review},
    store::{RequestView, RunSummary, RunView},
    types::{RequestId, RunId},
};
// The unit tests reach the store through this module.
#[cfg(test)]
use crate::store;
use ratatui::{layout::Rect, widgets::TableState};
use std::{path::PathBuf, time::Duration};
use time::OffsetDateTime;
use tui_tree_widget::TreeState;

/// Cached clock/duration redraw, independent of state invalidation and SQLite reconciliation.
const REFRESH: Duration = Duration::from_secs(1);
/// Animate cancellable Human jobs without increasing database polling.
const SPIN: Duration = Duration::from_millis(100);
/// Check an open Agent conversation for new events: often enough that a live session
/// follows within seconds, rarely enough that reading the session file stays cheap.
pub(crate) const SESSION_PROBE: Duration = Duration::from_secs(5);
/// Bound retained Run pages while allowing the selected scope to page older records.
pub const PAGE: u32 = 100;

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    None,
    Refresh,
    Resync,
    Quit,
    OpenDetail,
    Capture(bool),
    Review(review::Action),
}
/// The four drill-down levels, left to right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Repositories,
    Runs,
    Artifacts,
    Detail,
}
/// Areas inside a non-Human Detail pane; a Human review keeps its own sub-areas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailArea {
    Summary,
    Evidence,
}

/// The Full Detail is present exactly while it is the focused pane.
enum Focus {
    Repositories,
    Runs,
    Artifacts,
    Detail(Box<DetailPane>),
}

/// A shared, cached tree.
type Rows = std::sync::Arc<Vec<Node>>;

/// Semantic selection/drafts live here, independently of render geometry.
pub struct Monitor {
    state: PathBuf,
    initial: Option<PathBuf>,
    catalog: catalog::Catalog,
    pub scope: Scope,
    focus: Focus,
    initialized: bool,
    limit: u32,
    offset: u32,
    page_tail: bool,
    load_revision: u64,
    navigation: bool,
    loader: Option<loader::Loader>,
    /// What the change hints since the last read named.
    changed: crate::changes::Changed,
    origin: loader::Origin,
    runs: Vec<RunSummary>,
    repositories: TableState,
    list: TableState,
    open: Option<RunId>,
    run: Option<(RunView, Vec<RequestView>)>,
    tree: TreeState<model::NodeId>,
    folds: folds::Folds,
    /// The cached tree it was built from, and that tree with the Run row above it.
    framed: Option<(Rows, Rows)>,
    serial: crate::monitor::session::ReaderId,
    peek: Option<Peek>,
    reviews: std::collections::BTreeMap<RequestId, Review>,
    help: bool,
    mouse_capture: bool,
    hits: input::Hits,
    last_click: Option<input::Click>,
    /// The most recently drawn frame; the peek is loaded only when it is visible.
    size: Rect,
    refreshed: Option<OffsetDateTime>,
    error: Option<String>,
    notice: Option<String>,
    /// The shown Run's share in its memory, replaced after the Run and its tree so the memory
    /// of a Run no longer shown goes back to the system.
    footprint: Option<artifactize_core::platform::Footprint>,
}

impl Monitor {
    /// cwd/--repo/--all determines only the initial selection; catalog always covers all state.
    pub fn new(state: PathBuf, repo: Option<PathBuf>) -> Self {
        Self {
            state,
            initial: initial(repo),
            catalog: catalog::Catalog::default(),
            scope: Scope::All,
            focus: Focus::Runs,
            initialized: false,
            limit: PAGE,
            offset: 0,
            page_tail: false,
            load_revision: 0,
            navigation: false,
            loader: Some(Default::default()),
            changed: Default::default(),
            origin: loader::Origin::default(),
            runs: Vec::new(),
            repositories: TableState::default(),
            list: TableState::default(),
            open: None,
            run: None,
            tree: TreeState::default(),
            folds: folds::Folds::default(),
            framed: None,
            serial: crate::monitor::session::ReaderId::default(),
            peek: None,
            reviews: std::collections::BTreeMap::new(),
            help: false,
            mouse_capture: true,
            hits: input::Hits::default(),
            last_click: None,
            size: Rect::default(),
            refreshed: None,
            error: None,
            notice: None,
            footprint: None,
        }
    }

    #[cfg(test)]
    fn set_run(&mut self, run: RunView, requests: Vec<RequestView>) {
        let first = self
            .run
            .as_ref()
            .is_none_or(|(old, _)| old.run.id != run.run.id);
        if first {
            self.tree = TreeState::default();
        }
        self.run = Some((run, requests));
        self.sync_tree(first);
    }
    fn selected_run(&self) -> Option<&RunSummary> {
        self.list.selected().and_then(|index| self.runs.get(index))
    }
    pub fn target(&self) -> Option<Target> {
        self.tree.selected().last().map(Target::from)
    }
    /// The saved request of the selected eval node.
    fn selected_request(&self) -> Option<&RequestView> {
        let Target::Eval(eval) = self.target()? else {
            return None;
        };
        let (_, requests) = self.run.as_ref()?;
        requests.iter().find(|view| view.request.eval_id == eval)
    }
    pub fn waiting(&self) -> Option<&RequestId> {
        let view = self.selected_request()?;
        (view.request.status().is_human_wait()).then_some(&view.request.id)
    }
    pub fn focus(&self) -> Pane {
        match self.focus {
            Focus::Repositories => Pane::Repositories,
            Focus::Runs => Pane::Runs,
            Focus::Artifacts => Pane::Artifacts,
            Focus::Detail(_) => Pane::Detail,
        }
    }

    fn focus_on(&mut self, pane: Pane) {
        let next = match pane {
            Pane::Repositories => Focus::Repositories,
            Pane::Runs => Focus::Runs,
            Pane::Artifacts => Focus::Artifacts,
            // Opening the Preview must load a Detail before it can be focused.
            Pane::Detail => return,
        };
        self.leave_detail();
        self.focus = next;
    }

    fn select_scope(&mut self, index: usize) -> Action {
        let Some(row) = self.catalog.rows.get(index) else {
            return Action::None;
        };
        self.repositories.select(Some(index));
        // A fold row only lists worktrees; Space shows them.
        if row.fold.is_some() || self.scope == row.scope {
            return Action::None;
        }
        self.scope = row.scope.clone();
        self.limit = PAGE;
        self.offset = 0;
        self.page_tail = false;
        self.load_revision += 1;
        self.navigation = true;
        self.runs.clear();
        self.list.select(None);
        self.open = None;
        self.run = None;
        self.footprint = None;
        Action::Refresh
    }
    fn select_run(&mut self, index: usize) -> Action {
        if index >= self.runs.len() {
            return Action::None;
        }
        self.list.select(Some(index));
        self.load_revision += 1;
        self.navigation = true;
        self.open = self
            .selected_run()
            .filter(|run| run.readable())
            .map(|run| run.id.clone());
        Action::Refresh
    }
}
