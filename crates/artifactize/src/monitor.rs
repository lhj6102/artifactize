//! Repository/worktree → Run → artifacts/evals, with shared single-request Human review jobs.
mod catalog;
pub(crate) mod input;
mod modal;
mod model;
#[cfg(test)]
mod pane_tests;
#[cfg(test)]
mod pty_fixture_tests;
#[cfg(test)]
mod redesign_tests;
mod session;
#[cfg(test)]
mod session_tests;
mod terminal;
#[cfg(test)]
pub(crate) mod tests;
mod view;
pub use catalog::{Repository, Scope};
#[cfg(test)]
pub(crate) use modal::original as test_original;
pub use model::{
    Detail, Node, Progress, RunRow, Target, detail, duration, glyph, progress, run_rows, tree,
};
pub use terminal::run;
pub(crate) use terminal::{repaint, suspend};
pub(crate) use view::clock;

use crate::{
    config::ProfileKind,
    review::{self, Review},
    store::{self, RequestView, RunSummary, RunView},
    types::{RequestId, RunId},
    workspace::canonical_target,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::TableState;
use std::{path::PathBuf, time::Duration};
use time::OffsetDateTime;
use tui_tree_widget::TreeState;

/// Cached clock/duration redraw, independent of state invalidation and SQLite reconciliation.
const REFRESH: Duration = Duration::from_secs(1);
/// Animate cancellable Human jobs without increasing database polling.
const SPIN: Duration = Duration::from_millis(100);
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Repositories,
    Runs,
    Artifacts,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalPane {
    Summary,
    Evidence,
    Tools,
    Fields,
}

pub struct Modal {
    run_id: RunId,
    target: Target,
    request: Option<RequestId>,
    evidence: modal::Evidence,
    evidence_stamp: Option<modal::EvidenceStamp>,
    /// Snapshot remains attached to this modal if the selected Run is pruned or shifts pages.
    detail: Detail,
    review: Option<Review>,
    focus: ModalPane,
    scroll: [u16; 2],
    live: Option<session::Live>,
}

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
    tree: TreeState<String>,
    modal: Option<Modal>,
    modal_serial: u64,
    reviews: std::collections::BTreeMap<RequestId, Review>,
    mouse_capture: bool,
    hits: input::Hits,
    last_click: Option<input::Click>,
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
            modal: None,
            modal_serial: 0,
            reviews: std::collections::BTreeMap::new(),
            mouse_capture: true,
            hits: input::Hits::default(),
            last_click: None,
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
                self.error = None;
            }
            Err(error) => self.error = Some(format!("read failed at {}: {error}", clock(now))),
        }
    }
    async fn reload(&mut self) -> Result<(), String> {
        let catalog = store::read_catalog(&self.state).await?;
        self.catalog.update(&catalog, self.initial.as_deref());
        if !self.initialized {
            self.scope = self.catalog.initial(self.initial.as_deref());
            self.initialized = true;
        }
        let selected = self.repositories.selected().filter(|index| {
            self.catalog
                .rows
                .get(*index)
                .is_some_and(|row| row.scope == self.scope)
        });
        self.repositories.select(selected.or_else(|| {
            self.catalog
                .rows
                .iter()
                .position(|row| row.scope == self.scope)
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
        if let Some(modal) = &mut self.modal {
            if let Some((run, requests)) = &self.run
                && run.run.id == modal.run_id
                && modal
                    .request
                    .as_ref()
                    .is_none_or(|id| requests.iter().any(|view| &view.request.id == id))
            {
                modal.detail =
                    model::detail(run, requests, &modal.target, OffsetDateTime::now_utc());
                if modal.review.is_none()
                    && let Some(view) = modal
                        .request
                        .as_ref()
                        .and_then(|id| requests.iter().find(|view| &view.request.id == id))
                {
                    let stamp = modal::EvidenceStamp::new(view);
                    if modal.evidence_stamp.as_ref() != Some(&stamp)
                        || view.request.profile.kind() == ProfileKind::Agent
                    {
                        Self::load_evidence(&self.state, self.modal_serial, modal, view).await;
                        modal.evidence_stamp = Some(stamp);
                    }
                }
            }
            if let Some(review) = &mut modal.review {
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
            let nodes = tree(&run, &requests, OffsetDateTime::now_utc());
            for node in &nodes {
                if !node.id.starts_with("f:") {
                    self.tree.open(vec![node.id.clone()]);
                }
            }
            if let Some(node) = nodes.first() {
                self.tree.select(vec![node.id.clone()]);
            }
        }
        self.run = Some((run, requests));
    }
    fn selected_run(&self) -> Option<&RunSummary> {
        self.list.selected().and_then(|index| self.runs.get(index))
    }
    pub fn target(&self) -> Option<Target> {
        self.tree.selected().last().and_then(|id| Target::parse(id))
    }
    pub fn waiting(&self) -> Option<&str> {
        let Target::Eval(eval) = self.target()? else {
            return None;
        };
        let (_, requests) = self.run.as_ref()?;
        let view = requests.iter().find(|view| view.request.eval_id == eval)?;
        (view.request.status == crate::types::RequestStatus::WaitingHuman)
            .then_some(view.request.id.as_str())
    }
    fn select_scope(&mut self, index: usize) -> Action {
        let Some(row) = self.catalog.rows.get(index) else {
            return Action::None;
        };
        self.repositories.select(Some(index));
        if self.scope == row.scope {
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
        self.modal_serial += 1;
        let mut modal = Modal {
            run_id: run.run.id.clone(),
            target,
            detail: saved_detail,
            request: view.as_ref().map(|view| view.request.id.clone()),
            evidence: modal::Evidence::default(),
            evidence_stamp: view.as_ref().map(modal::EvidenceStamp::new),
            review: None,
            focus: ModalPane::Evidence,
            scroll: [0; 2],
            live: None,
        };
        if let Some(view) = view {
            if view.request.profile.kind() == ProfileKind::Human {
                let resolved = if view.request.status == crate::types::RequestStatus::WaitingHuman {
                    modal::original(&self.state, &view).await
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
                                        Some(view.request.id.to_string()),
                                    )
                                });
                            modal.request = Some(view.request.id.clone());
                            review.load_single(view);
                            modal.review = Some(review);
                            modal.focus = ModalPane::Fields;
                        }
                        Err(error) => self.notice = Some(error),
                    },
                    Err(error) => self.notice = Some(error),
                }
            } else {
                Self::load_evidence(&self.state, self.modal_serial, &mut modal, &view).await;
            }
        }
        self.modal = Some(modal);
    }
    async fn load_evidence(
        state: &std::path::Path,
        serial: u64,
        modal: &mut Modal,
        view: &RequestView,
    ) {
        use crate::agent::session::live::{self, Resolution};
        if view.request.profile.kind() != ProfileKind::Agent {
            modal.evidence = modal::evidence(state, view);
            return;
        }
        modal.evidence.title = "Agent session".into();
        match live::resolve(state, view).await {
            Ok(Resolution::Local(source)) => {
                let same = modal
                    .live
                    .as_ref()
                    .is_some_and(|live| live.source.reference == source.reference);
                if !same {
                    modal.live = Some(session::Live::new(serial, source));
                } else if let Some(live) = &mut modal.live {
                    live.source = source;
                    if let Some(reader) = &mut live.reader {
                        reader.source = live.source.clone();
                    }
                    live.invalidate();
                }
                modal.evidence.text = "Loading session…".into();
            }
            Ok(Resolution::Unavailable(text)) => {
                modal.live = None;
                modal.evidence.text = text;
            }
            Err(error) => {
                modal.live = None;
                modal.evidence.text = format!("Conversation unavailable: {error}");
            }
        }
    }
    fn close_modal(&mut self) {
        if let Some(modal) = self.modal.take()
            && let (Some(id), Some(review)) = (modal.request, modal.review)
        {
            self.reviews.insert(id, review);
        }
    }
    pub fn key(&mut self, key: KeyEvent) -> Action {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            if let Some(review) = self.modal.as_mut().and_then(|modal| modal.review.as_mut())
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
        if let Some(modal) = &mut self.modal {
            if key.code == KeyCode::Esc {
                if let Some(review) = &mut modal.review
                    && (review.busy() || review.confirming())
                {
                    return Action::Review(review.control(review::Control::Cancel));
                }
                self.close_modal();
                return Action::None;
            }
            if let Some(review) = &mut modal.review {
                if key.code == KeyCode::BackTab
                    || key.code == KeyCode::Tab
                        && (modal.focus == ModalPane::Tools || !review.editing())
                {
                    modal.focus = if modal.focus == ModalPane::Tools {
                        ModalPane::Fields
                    } else {
                        ModalPane::Tools
                    };
                    return Action::None;
                }
                return Action::Review(review.key_single(key, modal.focus == ModalPane::Tools));
            }
            if modal.focus == ModalPane::Evidence
                && let Some(live) = &mut modal.live
            {
                use crate::agent::session::document::Move;
                if matches!(key.code, KeyCode::Enter | KeyCode::Char(' ')) {
                    live.toggle_visible();
                    return Action::None;
                }
                let movement = match key.code {
                    KeyCode::Up => Some(Move::Up(1)),
                    KeyCode::Down => Some(Move::Down(1)),
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
            let index = usize::from(modal.focus == ModalPane::Evidence);
            match key.code {
                KeyCode::Tab => {
                    modal.focus = if modal.focus == ModalPane::Summary {
                        ModalPane::Evidence
                    } else {
                        ModalPane::Summary
                    }
                }
                KeyCode::Down => modal.scroll[index] = modal.scroll[index].saturating_add(1),
                KeyCode::Up => modal.scroll[index] = modal.scroll[index].saturating_sub(1),
                KeyCode::PageDown => {
                    modal.scroll[index] = modal.scroll[index].saturating_add(input::SCROLL_PAGE)
                }
                KeyCode::PageUp => {
                    modal.scroll[index] = modal.scroll[index].saturating_sub(input::SCROLL_PAGE)
                }
                _ => {}
            }
            return Action::None;
        }
        self.notice = None;
        if key.code == KeyCode::Char('q')
            || key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c')
        {
            return Action::Quit;
        }
        match key.code {
            KeyCode::Left => {
                self.focus = match self.focus {
                    Pane::Repositories => Pane::Repositories,
                    Pane::Runs => Pane::Repositories,
                    Pane::Artifacts => Pane::Runs,
                };
                return Action::None;
            }
            KeyCode::Right => {
                let previous = self.focus;
                self.focus = match self.focus {
                    Pane::Repositories => Pane::Runs,
                    Pane::Runs => Pane::Artifacts,
                    Pane::Artifacts => Pane::Artifacts,
                };
                // Keep the former Runs→Artifacts refresh without dispatching to the tree.
                return if previous == Pane::Runs {
                    Action::Refresh
                } else {
                    Action::None
                };
            }
            KeyCode::Char('r') => return Action::Refresh,
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Pane::Repositories => Pane::Runs,
                    Pane::Runs => Pane::Artifacts,
                    Pane::Artifacts => Pane::Repositories,
                }
            }
            KeyCode::BackTab => {
                self.focus = match self.focus {
                    Pane::Repositories => Pane::Artifacts,
                    Pane::Runs => Pane::Repositories,
                    Pane::Artifacts => Pane::Runs,
                }
            }
            KeyCode::Esc => {
                if self.focus == Pane::Runs {
                    return Action::Quit;
                }
                self.focus = Pane::Runs;
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
                KeyCode::Char('h') => {
                    self.tree.key_left();
                }
                KeyCode::Char('l') => {
                    self.tree.key_right();
                }
                KeyCode::Char(' ') => {
                    self.tree.toggle_selected();
                }
                KeyCode::Enter | KeyCode::Char('o') => return Action::OpenDetail,
                _ => {}
            },
        }
        Action::None
    }
    pub fn paste(&mut self, text: &str) {
        if let Some(modal) = &mut self.modal
            && modal.focus == ModalPane::Fields
            && let Some(review) = &mut modal.review
        {
            review.paste_single(text);
        }
    }
}
