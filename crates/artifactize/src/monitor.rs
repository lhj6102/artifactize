//! Review progress TUI over read-only state queries.

use std::{
    io::{self, IsTerminal},
    path::PathBuf,
    time::Duration,
};

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures_util::StreamExt;
use ratatui::widgets::TableState;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio_util::sync::CancellationToken;

use crate::store::{self, RunSummary, RunView};

#[cfg(test)]
mod tests;
mod ui;
mod view;

const PAGE_SIZE: u32 = 50;
const REFRESH_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, PartialEq)]
enum Screen {
    Runs,
    Progress(String),
}

struct App {
    state: PathBuf,
    repo: Option<PathBuf>,
    screen: Screen,
    runs: Vec<RunSummary>,
    progress: Option<RunView>,
    offset: u32,
    more: bool,
    selection: TableState,
    scroll: u16,
    last_refresh: Option<String>,
    error: Option<String>,
}

impl App {
    fn new(state: PathBuf, repo: Option<PathBuf>) -> Self {
        Self {
            state,
            repo,
            screen: Screen::Runs,
            runs: Vec::new(),
            progress: None,
            offset: 0,
            more: false,
            selection: TableState::default(),
            scroll: 0,
            last_refresh: None,
            error: None,
        }
    }

    fn refreshed(&mut self, result: Result<(), String>) {
        match result {
            Ok(()) => {
                self.last_refresh = Some(
                    OffsetDateTime::now_utc()
                        .format(&Rfc3339)
                        .expect("UTC timestamp"),
                );
                self.error = None;
            }
            Err(error) => self.error = Some(view::text(&error)),
        }
    }

    async fn load_runs(&mut self, offset: u32) -> Result<(), String> {
        let mut runs =
            store::read_runs(&self.state, self.repo.as_deref(), PAGE_SIZE + 1, offset).await?;
        let selected = if self.offset == offset {
            self.selection
                .selected()
                .and_then(|index| self.runs.get(index))
                .and_then(|selected| {
                    runs.iter()
                        .take(PAGE_SIZE as usize)
                        .position(|run| run.id == selected.id)
                })
                .unwrap_or_default()
        } else {
            0
        };
        self.more = runs.len() > PAGE_SIZE as usize;
        runs.truncate(PAGE_SIZE as usize);
        if self.offset != offset {
            self.selection = TableState::default();
        }
        self.selection
            .select((!runs.is_empty()).then_some(selected));
        self.runs = runs;
        self.offset = offset;
        Ok(())
    }

    async fn refresh(&mut self) {
        let result = match &self.screen {
            Screen::Runs => self.load_runs(self.offset).await,
            Screen::Progress(id) => store::read_run(&self.state, id).await.map(|progress| {
                self.progress = Some(progress);
            }),
        };
        self.refreshed(result);
    }

    async fn open_selected(&mut self) {
        let Some(run) = self
            .selection
            .selected()
            .and_then(|index| self.runs.get(index))
        else {
            return;
        };
        let result = store::read_run(&self.state, &run.id).await.map(|progress| {
            self.screen = Screen::Progress(progress.run.id.clone());
            self.progress = Some(progress);
            self.scroll = 0;
        });
        self.refreshed(result);
    }

    async fn key(&mut self, key: KeyEvent) -> bool {
        if key.kind == KeyEventKind::Release {
            return false;
        }
        if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)
            || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
        {
            return true;
        }
        match key.code {
            KeyCode::Char('r') => self.refresh().await,
            KeyCode::Enter if self.screen == Screen::Runs => self.open_selected().await,
            KeyCode::Char('b') | KeyCode::Backspace | KeyCode::Left
                if self.screen != Screen::Runs =>
            {
                let result = self.load_runs(self.offset).await;
                if result.is_ok() {
                    self.screen = Screen::Runs;
                }
                self.refreshed(result);
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if self.screen == Screen::Runs {
                    if !self.runs.is_empty() {
                        self.selection.select(Some(
                            (self.selection.selected().unwrap_or_default() + 1)
                                .min(self.runs.len() - 1),
                        ));
                    }
                } else {
                    self.scroll = self.scroll.saturating_add(1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.screen == Screen::Runs {
                    if !self.runs.is_empty() {
                        self.selection.select(Some(
                            self.selection
                                .selected()
                                .unwrap_or_default()
                                .saturating_sub(1),
                        ));
                    }
                } else {
                    self.scroll = self.scroll.saturating_sub(1);
                }
            }
            KeyCode::Char('n') | KeyCode::PageDown if self.screen == Screen::Runs && self.more => {
                let result = self.load_runs(self.offset.saturating_add(PAGE_SIZE)).await;
                self.refreshed(result);
            }
            KeyCode::Char('p') | KeyCode::PageUp
                if self.screen == Screen::Runs && self.offset > 0 =>
            {
                let result = self.load_runs(self.offset.saturating_sub(PAGE_SIZE)).await;
                self.refreshed(result);
            }
            KeyCode::PageDown if self.screen != Screen::Runs => {
                self.scroll = self.scroll.saturating_add(10)
            }
            KeyCode::PageUp if self.screen != Screen::Runs => {
                self.scroll = self.scroll.saturating_sub(10)
            }
            KeyCode::Home => {
                self.scroll = 0;
                self.selection.select((!self.runs.is_empty()).then_some(0));
            }
            _ => {}
        }
        false
    }
}

struct RestoreTerminal;

impl Drop for RestoreTerminal {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

/// Read saved state only. `repo = None` selects every repository in this state directory.
pub async fn run(
    state: PathBuf,
    repo: Option<PathBuf>,
    cancellation: CancellationToken,
) -> Result<(), String> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err("monitor requires an interactive terminal (stdin and stdout).".into());
    }
    let _restore = RestoreTerminal;
    // ratatui also installs its restore-before-panic hook, including setup failures.
    let mut terminal = ratatui::try_init().map_err(|error| error.to_string())?;
    let mut app = App::new(state, repo);
    let mut events = EventStream::new();
    let mut refresh = tokio::time::interval(REFRESH_INTERVAL);
    refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        terminal
            .draw(|frame| ui::draw(frame, &mut app, OffsetDateTime::now_utc()))
            .map_err(|error| error.to_string())?;
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => break,
            result = async {
                tokio::select! {
                    _ = refresh.tick() => {
                        app.refresh().await;
                        Ok(false)
                    }
                    event = events.next() => match event {
                        Some(Ok(Event::Key(key))) => Ok(app.key(key).await),
                        Some(Ok(_)) => Ok(false),
                        Some(Err(error)) => Err(error.to_string()),
                        None => Ok(true),
                    }
                }
            } => if result? { break; }
        }
    }
    Ok(())
}
