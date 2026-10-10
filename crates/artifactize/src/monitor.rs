//! Repository/worktree → Run → artifacts/evals → Detail, as reactive panes, with shared
//! single-request Human review jobs.
mod catalog;
mod evidence;
mod fold;
pub(crate) mod input;
pub(crate) mod layout;
mod model;
#[cfg(test)]
mod pane_tests;
#[cfg(test)]
mod pty_fixture_tests;
#[cfg(test)]
mod redesign_tests;
mod rows;
mod session;
#[cfg(test)]
mod session_tests;
mod terminal;
#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
mod tree_tests;
mod view;
pub use catalog::{Repository, Scope};
#[cfg(test)]
pub(crate) use evidence::original as test_original;
pub use model::{
    Activity, Busy, Completion, Detail, EvalView, Kind, Node, NotRun, Progress, Queue, RunRow,
    Section, Segment, Source, Strip, Target, Tone, Upstream, Waits, Weight, detail, duration,
    glyph, progress, run_rows, strip, tree, upstream_index,
};
pub(crate) use rows::{fit, plain, width};
pub use terminal::run;
pub(crate) use terminal::{InputGuard, repaint, suspend};
pub(crate) use view::{clock, crumbs};

use crate::{
    config::ProfileKind,
    review::{self, Review},
    store::{self, RequestView, RunSummary, RunView},
    types::{RequestId, RequestStatus, RunId},
    workspace::canonical_target,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
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
const SESSION_PROBE: Duration = Duration::from_secs(5);
/// Bound retained Run pages while allowing the selected scope to page older records.
const PAGE: u32 = 100;

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    None,
    Refresh,
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

/// The opened Detail of one tree node: sections, evidence, an Agent session or a Human review.
pub struct DetailPane {
    run_id: RunId,
    target: Target,
    request: Option<RequestId>,
    evidence: evidence::Evidence,
    evidence_stamp: Option<evidence::EvidenceStamp>,
    /// Snapshot remains attached to this Detail if the selected Run is pruned or shifts pages.
    detail: Detail,
    review: Option<Review>,
    focus: DetailArea,
    scroll: [u16; 2],
    live: Option<session::Live>,
    show_details: bool,
    /// The folded Technical section is shown.
    technical: bool,
    /// The pending evals under `Waits for` are shown.
    pending: bool,
}

/// The tree-focused peek of a running Agent: only its last transcript line is read.
struct Peek {
    request: RequestId,
    /// The request's session facts when it was resolved; a change resolves it again.
    stamp: evidence::EvidenceStamp,
    live: Option<session::Live>,
    /// Why there is no session to read; a refresh resolves it again.
    text: String,
}

/// A shared, cached tree.
type Rows = std::sync::Arc<Vec<Node>>;

/// Semantic selection/drafts live here, independently of render geometry.
pub struct Monitor {
    state: PathBuf,
    initial: Option<PathBuf>,
    catalog: catalog::Catalog,
    pub scope: Scope,
    pub focus: Pane,
    initialized: bool,
    limit: u32,
    runs: Vec<RunSummary>,
    repositories: TableState,
    list: TableState,
    open: Option<RunId>,
    run: Option<(RunView, Vec<RequestView>)>,
    tree: TreeState<model::NodeId>,
    folds: fold::Folds,
    /// The cached tree it was built from, and that tree with the Run row above it.
    framed: Option<(Rows, Rows)>,
    detail: Option<DetailPane>,
    serial: u64,
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
}

impl Monitor {
    /// cwd/--repo/--all determines only the initial selection; catalog always covers all state.
    pub fn new(state: PathBuf, repo: Option<PathBuf>) -> Self {
        let initial = repo.map(|path| canonical_target(&path).unwrap_or(path));
        Self {
            state,
            initial,
            catalog: catalog::Catalog::default(),
            scope: Scope::All,
            focus: Pane::Runs,
            initialized: false,
            limit: PAGE,
            runs: Vec::new(),
            repositories: TableState::default(),
            list: TableState::default(),
            open: None,
            run: None,
            tree: TreeState::default(),
            folds: fold::Folds::default(),
            framed: None,
            detail: None,
            serial: 0,
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
        }
    }

    pub async fn refresh(&mut self) {
        let result = self.reload().await;
        let now = OffsetDateTime::now_utc();
        match result {
            Ok(()) => {
                self.refreshed = Some(now);
                let unreadable = self
                    .runs
                    .iter()
                    .flat_map(|run| &run.unreadable)
                    .map(ToString::to_string)
                    .collect::<Vec<_>>();
                self.error = (!unreadable.is_empty()).then(|| unreadable.join("; "));
            }
            Err(error) => self.error = Some(format!("read failed at {}: {error}", clock(now))),
        }
    }
    async fn reload(&mut self) -> Result<(), String> {
        let catalog = store::read_catalog(&self.state).await?;
        // A selected fold row stays selected when its worktrees are shown or hidden.
        let fold = self
            .repositories
            .selected()
            .and_then(|index| self.catalog.rows.get(index))
            .and_then(|row| row.fold.clone());
        if self.initialized {
            self.catalog.selected = Some(self.scope.clone());
        }
        self.catalog.update(&catalog, self.initial.as_deref());
        if !self.initialized {
            self.scope = self.catalog.initial(self.initial.as_deref());
            self.initialized = true;
            // The initial scope stays listed even when its worktree has no Runs yet.
            self.catalog.selected = Some(self.scope.clone());
            self.catalog.update(&catalog, self.initial.as_deref());
        }
        let selected = match fold {
            Some(fold) => self
                .catalog
                .rows
                .iter()
                .position(|row| row.fold.as_ref() == Some(&fold)),
            None => self.repositories.selected().filter(|index| {
                self.catalog
                    .rows
                    .get(*index)
                    .is_some_and(|row| row.fold.is_none() && row.scope == self.scope)
            }),
        };
        self.repositories.select(selected.or_else(|| {
            self.catalog
                .rows
                .iter()
                .position(|row| row.fold.is_none() && row.scope == self.scope)
        }));
        let paths = self.catalog.paths(&self.scope);
        let selected = self.selected_run().map(|run| run.id.clone());
        let mut runs =
            store::read_scoped_runs(&self.state, paths.as_deref(), self.limit, 0).await?;
        // Newly inserted Runs may push a selected row out of its page; grow only as needed.
        while selected
            .as_ref()
            .is_some_and(|id| !runs.iter().any(|run| &run.id == id))
            && runs.len() as u32 == self.limit
        {
            self.limit = self.limit.saturating_add(PAGE);
            runs = store::read_scoped_runs(&self.state, paths.as_deref(), self.limit, 0).await?;
        }
        self.set_runs(runs);
        if let Some(id) = self.open.clone() {
            let run = store::read_run(&self.state, &id).await?;
            let requests = store::read_requests(&self.state, Some(&id)).await?;
            self.set_run(run, requests);
        }
        // A peek without a session (unavailable or failed) resolves again after a refresh.
        self.peek = self.peek.take().filter(|peek| peek.live.is_some());
        if let Some(live) = self.peek.as_mut().and_then(|peek| peek.live.as_mut()) {
            live.invalidate();
        }
        if let Some(pane) = &mut self.detail {
            if let Some((run, requests)) = &self.run
                && run.run.id == pane.run_id
                && pane
                    .request
                    .as_ref()
                    .is_none_or(|id| requests.iter().any(|view| &view.request.id == id))
            {
                pane.detail = model::detail(run, requests, &pane.target, OffsetDateTime::now_utc());
                if pane.review.is_none()
                    && let Some(view) = pane
                        .request
                        .as_ref()
                        .and_then(|id| requests.iter().find(|view| &view.request.id == id))
                {
                    let stamp = evidence::EvidenceStamp::new(view);
                    if pane.evidence_stamp.as_ref() != Some(&stamp)
                        || view.request.profile.kind() == ProfileKind::Agent
                    {
                        Self::load_evidence(&self.state, self.serial, pane, view).await;
                        pane.evidence_stamp = Some(stamp);
                    }
                }
            }
            if let Some(review) = &mut pane.review {
                let _ = review.refresh().await;
            }
        }
        Ok(())
    }
    fn set_runs(&mut self, runs: Vec<RunSummary>) {
        let selected = self.selected_run().map(|run| run.id.clone());
        let index = selected
            .and_then(|id| runs.iter().position(|run| run.id == id))
            .or(self.list.selected());
        self.runs = runs;
        self.list.select(
            index
                .map(|index| index.min(self.runs.len().saturating_sub(1)))
                .or(Some(0))
                .filter(|_| !self.runs.is_empty()),
        );
        self.open = self.selected_run().map(|run| run.id.clone());
        if self.open.is_none() {
            self.run = None;
        }
    }
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
    pub fn waiting(&self) -> Option<&str> {
        let view = self.selected_request()?;
        (view.request.status() == RequestStatus::WaitingHuman).then_some(view.request.id.as_str())
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
        self.runs.clear();
        self.list.select(None);
        self.open = None;
        self.run = None;
        Action::Refresh
    }
    fn select_run(&mut self, index: usize) -> Action {
        if index >= self.runs.len() {
            return Action::None;
        }
        self.list.select(Some(index));
        self.open = self.selected_run().map(|run| run.id.clone());
        Action::Refresh
    }
    pub async fn open_detail(&mut self) {
        let Some(target) = self.target() else {
            return;
        };
        let Some((run, requests)) = &self.run else {
            return;
        };
        let view = match &target {
            Target::Eval(eval) => requests
                .iter()
                .find(|view| &view.request.eval_id == eval)
                .cloned(),
            _ => None,
        };
        let saved_detail = model::detail(run, requests, &target, OffsetDateTime::now_utc());
        self.serial += 1;
        let mut pane = DetailPane {
            run_id: run.run.id.clone(),
            target,
            detail: saved_detail,
            request: view.as_ref().map(|view| view.request.id.clone()),
            evidence: evidence::Evidence::default(),
            evidence_stamp: view.as_ref().map(evidence::EvidenceStamp::new),
            review: None,
            // Sections first; an Agent session moves focus to its transcript.
            focus: DetailArea::Summary,
            scroll: [0; 2],
            // The peek's session keeps its reader and position.
            live: self
                .peek
                .take()
                .filter(|peek| pane_request(&view) == Some(&peek.request))
                .and_then(|peek| peek.live),
            show_details: false,
            technical: false,
            pending: false,
        };
        if let Some(view) = view {
            if view.request.profile.kind() == ProfileKind::Human {
                let resolved = if view.request.status() == RequestStatus::WaitingHuman {
                    evidence::original(&self.state, &view).await
                } else {
                    Ok(view)
                };
                match resolved {
                    Ok(view) => match crate::human::default_reviewer() {
                        Ok(reviewer) => {
                            let mut review =
                                self.reviews.remove(&view.request.id).unwrap_or_else(|| {
                                    Review::new(
                                        self.state.clone(),
                                        None,
                                        reviewer,
                                        Some(view.request.id.clone()),
                                    )
                                });
                            pane.request = Some(view.request.id.clone());
                            review.load_single(view);
                            review.resolve_commands().await;
                            pane.review = Some(review);
                        }
                        Err(error) => self.notice = Some(error),
                    },
                    Err(error) => self.notice = Some(error),
                }
            } else {
                Self::load_evidence(&self.state, self.serial, &mut pane, &view).await;
            }
        }
        self.detail = Some(pane);
        self.focus = Pane::Detail;
    }
    async fn load_evidence(
        state: &std::path::Path,
        serial: u64,
        pane: &mut DetailPane,
        view: &RequestView,
    ) {
        use crate::agent::session::live::{self, Resolution};
        if view.request.profile.kind() != ProfileKind::Agent {
            pane.evidence = evidence::evidence(state, view);
            return;
        }
        pane.evidence.title = "Agent session".into();
        match live::resolve(state, view).await {
            Ok(Resolution::Local(source)) => {
                let same = pane
                    .live
                    .as_ref()
                    .is_some_and(|live| live.source.reference == source.reference);
                if !same {
                    pane.live = Some(session::Live::new(serial, source));
                } else if let Some(live) = &mut pane.live {
                    live.source = source;
                    if let Some(reader) = &mut live.reader {
                        reader.source = live.source.clone();
                    }
                    live.invalidate();
                }
                pane.evidence.text = "Loading session…".into();
            }
            Ok(Resolution::Unavailable(text)) => {
                pane.live = None;
                pane.evidence.text = text;
            }
            Err(error) => {
                pane.live = None;
                pane.evidence.text = format!("Conversation unavailable: {error}");
            }
        }
    }
    /// Close the Detail and step back to the tree; Human drafts survive for the next open.
    fn leave_detail(&mut self) {
        if let Some(pane) = self.detail.take()
            && let (Some(id), Some(review)) = (pane.request, pane.review)
        {
            self.reviews.insert(id, review);
        }
        if self.focus == Pane::Detail {
            self.focus = Pane::Artifacts;
        }
    }
    /// Load the peek's Agent session when the tree selects a running Agent and the peek shows.
    pub async fn sync_peek(&mut self) {
        use crate::agent::session::live::{self, Resolution};
        let visible = self.focus == Pane::Artifacts
            && self.detail.is_none()
            && layout::plan(self.size, self.focus, layout::Hints::default()).density(Pane::Detail)
                == layout::Density::Preview;
        let view = visible
            .then(|| self.selected_request())
            .flatten()
            .filter(|view| {
                view.request.profile.kind() == ProfileKind::Agent
                    && view.request.status() == RequestStatus::Running
            })
            .cloned();
        let Some(view) = view else {
            self.peek = None;
            return;
        };
        let stamp = evidence::EvidenceStamp::new(&view);
        // A resolved session is read incrementally; only a new request or new session facts
        // resolve it again, so drawing never queries the state.
        if let Some(peek) = &self.peek
            && peek.request == view.request.id
            && peek.stamp == stamp
        {
            return;
        }
        let kept = self
            .peek
            .take()
            .filter(|peek| peek.request == view.request.id)
            .and_then(|peek| peek.live);
        let (live, text) = match live::resolve(&self.state, &view).await {
            Ok(Resolution::Local(source)) => match kept {
                Some(mut live) if live.source.reference == source.reference => {
                    live.source = source;
                    if let Some(reader) = &mut live.reader {
                        reader.source = live.source.clone();
                    }
                    live.invalidate();
                    (Some(live), String::new())
                }
                _ => {
                    self.serial += 1;
                    (Some(session::Live::new(self.serial, source)), String::new())
                }
            },
            Ok(Resolution::Unavailable(text)) => (None, text),
            Err(error) => (None, format!("Conversation unavailable: {error}")),
        };
        self.peek = Some(Peek {
            request: view.request.id.clone(),
            stamp,
            live,
            text,
        });
    }
    /// The Agent session the driver reads for: the open Detail's, or else the peek's.
    fn live_mut(&mut self) -> Option<&mut session::Live> {
        match &mut self.detail {
            Some(pane) => pane.live.as_mut(),
            None => self.peek.as_mut().and_then(|peek| peek.live.as_mut()),
        }
    }
    fn review_mut(&mut self) -> Option<&mut Review> {
        self.detail.as_mut().and_then(|pane| pane.review.as_mut())
    }
    /// A Human review that is working or editing keeps focus in Detail.
    fn locked(&self) -> bool {
        self.detail
            .as_ref()
            .and_then(|pane| pane.review.as_ref())
            .is_some_and(owns_keys)
    }
    /// Select the next ERROR, RED or waiting Human eval after the selection, wrapping around.
    fn next_attention(&mut self) -> bool {
        fn walk(
            nodes: &[Node],
            parent: &[model::NodeId],
            order: &mut Vec<(Vec<model::NodeId>, bool)>,
        ) {
            for node in nodes {
                let mut path = parent.to_vec();
                path.push(node.id.clone());
                let attention = matches!(
                    node.kind,
                    Kind::Eval(EvalView::Failed { .. } | EvalView::InProgress(Activity::Human(_)))
                );
                order.push((path.clone(), attention));
                walk(&node.children, &path, order);
            }
        }
        if self.run.is_none() {
            self.notice = Some("Open a Run to find what needs attention.".into());
            return false;
        }
        let mut order = Vec::new();
        walk(&self.nodes(), &[], &mut order);
        let start = order
            .iter()
            .position(|(path, _)| path.as_slice() == self.tree.selected())
            .map_or(0, |index| index + 1);
        let Some(index) = (0..order.len())
            .map(|offset| (start + offset) % order.len())
            .find(|index| order[*index].1)
        else {
            self.notice = Some("Nothing in this Run needs attention.".into());
            return false;
        };
        let path = order[index].0.clone();
        for end in 1..path.len() {
            self.tree.open(path[..end].to_vec());
        }
        self.tree.select(path);
        self.focus = Pane::Artifacts;
        true
    }
    pub fn key(&mut self, key: KeyEvent) -> Action {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            if let Some(review) = self.review_mut()
                && review.busy()
            {
                return Action::Review(review.control(review::Control::Cancel));
            }
            return Action::Quit;
        }
        if key.code == KeyCode::F(2) {
            self.mouse_capture = !self.mouse_capture;
            return Action::Capture(self.mouse_capture);
        }
        if self.help {
            self.help = false;
            return Action::None;
        }
        if self.focus == Pane::Detail {
            if self.detail.is_some() {
                return self.detail_key(key);
            }
            self.focus = Pane::Artifacts;
        }
        self.notice = None;
        match key.code {
            KeyCode::Char('q') => return Action::Quit,
            KeyCode::Char('?') => {
                self.help = true;
                return Action::None;
            }
            KeyCode::Char('!') => {
                self.next_attention();
                return Action::None;
            }
            // Back one level; the first level stays, and only q or Ctrl-C quit.
            KeyCode::Left | KeyCode::Esc => {
                self.focus = layout::back(self.focus);
                return Action::None;
            }
            KeyCode::Right => {
                return match self.focus {
                    Pane::Repositories => {
                        self.focus = Pane::Runs;
                        Action::None
                    }
                    // Keep the former Runs→Artifacts refresh without dispatching to the tree.
                    Pane::Runs => {
                        self.focus = Pane::Artifacts;
                        Action::Refresh
                    }
                    Pane::Artifacts | Pane::Detail => Action::OpenDetail,
                };
            }
            KeyCode::Char('r') => return Action::Refresh,
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Pane::Repositories => Pane::Runs,
                    Pane::Runs => Pane::Artifacts,
                    Pane::Artifacts | Pane::Detail => Pane::Repositories,
                }
            }
            KeyCode::BackTab => {
                self.focus = match self.focus {
                    Pane::Repositories => Pane::Artifacts,
                    Pane::Runs => Pane::Repositories,
                    Pane::Artifacts | Pane::Detail => Pane::Runs,
                }
            }
            _ => {}
        }
        match self.focus {
            Pane::Repositories => match key.code {
                KeyCode::Down | KeyCode::Char('j') => {
                    return self
                        .select_scope(self.repositories.selected().unwrap_or(0).saturating_add(1));
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    return self
                        .select_scope(self.repositories.selected().unwrap_or(0).saturating_sub(1));
                }
                KeyCode::Char(' ') => {
                    let fold = self
                        .repositories
                        .selected()
                        .and_then(|index| self.catalog.rows.get(index))
                        .and_then(|row| row.fold.clone());
                    if let Some(repository) = fold {
                        self.catalog.toggle(&repository);
                        return Action::Refresh;
                    }
                }
                KeyCode::Enter => self.focus = Pane::Runs,
                _ => {}
            },
            Pane::Runs => match key.code {
                KeyCode::Down | KeyCode::Char('j') => {
                    let next = self.list.selected().map_or(0, |index| index + 1);
                    if next < self.runs.len() {
                        return self.select_run(next);
                    }
                    if self.runs.len() as u32 == self.limit {
                        self.limit = self.limit.saturating_add(PAGE);
                        return Action::Refresh;
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    return self.select_run(self.list.selected().unwrap_or(0).saturating_sub(1));
                }
                KeyCode::Enter => {
                    self.focus = Pane::Artifacts;
                    return Action::Refresh;
                }
                _ => {}
            },
            Pane::Artifacts => match key.code {
                KeyCode::Down | KeyCode::Char('j') => {
                    self.tree.key_down();
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.tree.key_up();
                }
                KeyCode::Char('h' | 'l' | ' ') => {
                    let before = self.tree.opened().clone();
                    match key.code {
                        KeyCode::Char('h') => self.tree.key_left(),
                        KeyCode::Char('l') => self.tree.key_right(),
                        _ => self.tree.toggle_selected(),
                    };
                    self.touched(&before);
                }
                KeyCode::Char('b') => self.jump_blocker(),
                KeyCode::Backspace => self.jump_back(),
                KeyCode::Enter | KeyCode::Char('o') => return Action::OpenDetail,
                _ => {}
            },
            Pane::Detail => {}
        }
        Action::None
    }
    /// Keys while Detail has focus. A Human review owns them through the shared component.
    fn detail_key(&mut self, key: KeyEvent) -> Action {
        let Some(pane) = &mut self.detail else {
            return Action::None;
        };
        if let Some(review) = &mut pane.review {
            match review.key_detail(key) {
                review::Handled::Action(action) => return Action::Review(action),
                review::Handled::Back => {
                    self.leave_detail();
                    return Action::None;
                }
                review::Handled::Quit => return Action::Quit,
                // `?` and `!` outside a form act as in any Detail; leaving keeps the draft.
                review::Handled::Pass => {}
            }
        }
        if key.code == KeyCode::Esc {
            if pane.show_details {
                pane.show_details = false;
                pane.focus = DetailArea::Evidence;
                return Action::None;
            }
            self.leave_detail();
            return Action::None;
        }
        // A Human review passes only `?` and `!`, outside a form or job.
        match key.code {
            KeyCode::Char('q') => return Action::Quit,
            KeyCode::Char('?') => {
                self.help = true;
                return Action::None;
            }
            KeyCode::Left => {
                self.leave_detail();
                return Action::None;
            }
            // The next attention item opens in Detail, so causes read one after another.
            KeyCode::Char('!') => {
                self.leave_detail();
                return if self.next_attention() {
                    Action::OpenDetail
                } else {
                    Action::None
                };
            }
            _ => {}
        }
        let Some(pane) = &mut self.detail else {
            return Action::None;
        };
        match key.code {
            KeyCode::Char('t') if pane.live.is_none() || pane.show_details => {
                pane.technical = !pane.technical;
                return Action::None;
            }
            KeyCode::Char('w') if pane.live.is_none() || pane.show_details => {
                pane.pending = !pane.pending;
                return Action::None;
            }
            _ => {}
        }
        if pane.live.is_some() && key.code == KeyCode::Char('d') {
            pane.show_details = !pane.show_details;
            pane.focus = if pane.show_details {
                DetailArea::Summary
            } else {
                DetailArea::Evidence
            };
            return Action::None;
        }
        if pane.live.is_some() && !pane.show_details {
            pane.focus = DetailArea::Evidence;
        }
        if pane.focus == DetailArea::Evidence
            && let Some(live) = &mut pane.live
        {
            use crate::agent::session::document::Move;
            if matches!(key.code, KeyCode::Enter | KeyCode::Char(' ')) {
                live.toggle_visible();
                return Action::None;
            }
            let movement = match key.code {
                KeyCode::Up | KeyCode::Char('k') => Some(Move::Up(1)),
                KeyCode::Down | KeyCode::Char('j') => Some(Move::Down(1)),
                KeyCode::PageUp => Some(Move::Up(live.scroll.height.max(1))),
                KeyCode::PageDown => Some(Move::Down(live.scroll.height.max(1))),
                KeyCode::Home => Some(Move::Top),
                KeyCode::End => Some(Move::Bottom),
                _ => None,
            };
            if let Some(movement) = movement {
                live.movement(movement);
                return Action::None;
            }
        }
        let index = usize::from(pane.focus == DetailArea::Evidence);
        match key.code {
            KeyCode::Tab | KeyCode::BackTab => {
                pane.focus = if pane.focus == DetailArea::Summary && !pane.evidence.is_empty() {
                    DetailArea::Evidence
                } else {
                    DetailArea::Summary
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                pane.scroll[index] = pane.scroll[index].saturating_add(1)
            }
            KeyCode::Up | KeyCode::Char('k') => {
                pane.scroll[index] = pane.scroll[index].saturating_sub(1)
            }
            KeyCode::PageDown => {
                pane.scroll[index] = pane.scroll[index].saturating_add(input::SCROLL_PAGE)
            }
            KeyCode::PageUp => {
                pane.scroll[index] = pane.scroll[index].saturating_sub(input::SCROLL_PAGE)
            }
            KeyCode::Home => pane.scroll[index] = 0,
            _ => {}
        }
        Action::None
    }
    pub fn paste(&mut self, text: &str) {
        if self.focus == Pane::Detail
            && let Some(review) = self.review_mut()
            && review.area() == review::Area::Fields
        {
            review.paste_single(text);
        }
    }
}

/// A Human review takes every key while it works, or while it edits a request
/// that still waits; a request settled elsewhere leaves its form behind without keys.
fn owns_keys(review: &Review) -> bool {
    review.busy() || !review.settled() && review.editing()
}

fn pane_request(view: &Option<RequestView>) -> Option<&RequestId> {
    view.as_ref().map(|view| &view.request.id)
}
