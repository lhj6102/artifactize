//! Review progress TUI over read-only state queries.

mod model;
#[cfg(test)]
mod tests;
mod view;

pub use model::{
    Detail, Node, Progress, RunRow, Target, detail, duration, glyph, progress, run_rows, tree,
};

use std::{path::PathBuf, time::Duration};

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures_util::StreamExt;
use ratatui::widgets::TableState;
use time::OffsetDateTime;
use tokio_util::sync::CancellationToken;
use tui_tree_widget::TreeState;

use crate::store::{self, RequestView, RunSummary, RunView};

const REFRESH: Duration = Duration::from_secs(1);
const PAGE: u32 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    Refresh,
    Quit,
}

/// Screen state over saved data; every load opens the state database read-only.
pub struct Monitor {
    state: PathBuf,
    repo: Option<PathBuf>,
    limit: u32,
    runs: Vec<RunSummary>,
    list: TableState,
    /// The open Run, if any, and its last successfully loaded data.
    open: Option<String>,
    run: Option<(RunView, Vec<RequestView>)>,
    tree: TreeState<String>,
    scroll: u16,
    refreshed: Option<OffsetDateTime>,
    error: Option<String>,
}

impl Monitor {
    /// `repo: None` shows Runs from every repository in the state database.
    pub fn new(state: PathBuf, repo: Option<PathBuf>) -> Self {
        Self {
            state,
            repo,
            limit: PAGE,
            runs: Vec::new(),
            list: TableState::default(),
            open: None,
            run: None,
            tree: TreeState::default(),
            scroll: 0,
            refreshed: None,
            error: None,
        }
    }

    /// Reload the visible screen; on failure keep the last-known data and record the error.
    pub async fn refresh(&mut self) {
        let result = match self.open.clone() {
            None => store::read_runs(&self.state, self.repo.as_deref(), self.limit, 0)
                .await
                .map(|runs| self.set_runs(runs)),
            Some(id) => match store::read_run(&self.state, &id).await {
                Ok(run) => store::read_requests(&self.state, Some(&id))
                    .await
                    .map(|requests| self.set_run(run, requests)),
                Err(error) => Err(error),
            },
        };
        let now = OffsetDateTime::now_utc();
        match result {
            Ok(()) => {
                self.refreshed = Some(now);
                self.error = None;
            }
            Err(error) => {
                self.error = Some(format!("read failed at {}: {error}", view::clock(now)));
            }
        }
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
            self.scroll = 0;
        }
        self.run = Some((run, requests));
    }

    fn selected_run(&self) -> Option<&RunSummary> {
        self.list.selected().and_then(|index| self.runs.get(index))
    }

    /// The detail target for the selected tree node.
    pub fn target(&self) -> Option<Target> {
        self.tree.selected().last().and_then(|id| Target::parse(id))
    }

    pub fn key(&mut self, key: KeyEvent) -> Action {
        if key.code == KeyCode::Char('q')
            || key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c')
        {
            return Action::Quit;
        }
        if key.code == KeyCode::Char('r') {
            return Action::Refresh;
        }
        if self.open.is_none() {
            match key.code {
                KeyCode::Esc => return Action::Quit,
                KeyCode::Down | KeyCode::Char('j') => {
                    let next = self.list.selected().map_or(0, |index| index + 1);
                    if next < self.runs.len() {
                        self.list.select(Some(next));
                    } else if self.runs.len() as u32 == self.limit {
                        self.limit += PAGE;
                        return Action::Refresh;
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => self.list.select_previous(),
                KeyCode::Enter => {
                    if let Some(run) = self.selected_run() {
                        self.open = Some(run.id.clone());
                        self.run = None;
                        return Action::Refresh;
                    }
                }
                _ => {}
            }
            return Action::None;
        }
        let moved = match key.code {
            KeyCode::Esc | KeyCode::Backspace => {
                self.open = None;
                self.run = None;
                return Action::Refresh;
            }
            KeyCode::Down | KeyCode::Char('j') => self.tree.key_down(),
            KeyCode::Up | KeyCode::Char('k') => self.tree.key_up(),
            KeyCode::Left | KeyCode::Char('h') => self.tree.key_left(),
            KeyCode::Right | KeyCode::Char('l') => self.tree.key_right(),
            KeyCode::Enter | KeyCode::Char(' ') => self.tree.toggle_selected(),
            KeyCode::PageDown => {
                self.scroll = self.scroll.saturating_add(10);
                false
            }
            KeyCode::PageUp => {
                self.scroll = self.scroll.saturating_sub(10);
                false
            }
            _ => false,
        };
        if moved {
            self.scroll = 0;
        }
        Action::None
    }
}

/// Run the terminal UI until q, Esc on the Run list, Ctrl-C, or a signal.
pub async fn run(
    state: PathBuf,
    repo: Option<PathBuf>,
    cancellation: CancellationToken,
) -> Result<(), String> {
    // ratatui's panic hook restores the terminal before a panic message is printed.
    let mut terminal = ratatui::try_init().map_err(|e| {
        let _ = crossterm::terminal::disable_raw_mode();
        format!("cannot start the monitor: {e}")
    })?;
    let result = watch(&mut terminal, Monitor::new(state, repo), cancellation).await;
    drop(terminal);
    ratatui::try_restore().map_err(|e| e.to_string())?;
    result
}

async fn watch(
    terminal: &mut ratatui::DefaultTerminal,
    mut monitor: Monitor,
    cancellation: CancellationToken,
) -> Result<(), String> {
    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(REFRESH);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        terminal
            .draw(|frame| monitor.draw(frame))
            .map_err(|e| e.to_string())?;
        tokio::select! {
            _ = cancellation.cancelled() => return Ok(()),
            _ = tick.tick() => monitor.refresh().await,
            event = events.next() => match event {
                Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => match monitor.key(key) {
                    Action::Quit => return Ok(()),
                    Action::Refresh => {
                        monitor.refresh().await;
                        tick.reset();
                    }
                    Action::None => {}
                },
                Some(Ok(_)) => {}
                Some(Err(error)) => return Err(error.to_string()),
                None => return Ok(()),
            },
        }
    }
}
