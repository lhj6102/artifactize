//! Human review TUI: claim, run Human tools and submit, in-process like `request`.

mod detail;
mod embedded;
#[cfg(test)]
mod embedded_tests;
#[cfg(test)]
mod identity_tests;
mod list;
#[cfg(test)]
mod list_tests;
mod record;
mod terminal;
#[cfg(test)]
pub(crate) mod tests;
mod view;

pub use artifactize_core::review::*;
pub(crate) use detail::{Handled, Hits};
pub(crate) use embedded::Control;
pub use terminal::run;

use crate::config::ToolName;
use std::{
    future::Future,
    path::PathBuf,
    time::{Duration, Instant},
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{layout::Position, widgets::TableState};
use serde_json::Value;
use time::OffsetDateTime;
use tokio_util::sync::CancellationToken;

use crate::{
    human,
    store::{self, HumanClaim, RequestView},
    tools::human::CommandLine,
    types::RequestId,
};

/// A key event as the form edits with it, and whether Control or Alt is held.
pub(crate) fn form_key(key: KeyEvent) -> (Key, bool) {
    let modified = key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
    let code = match key.code {
        KeyCode::Char(character) => Key::Char(character),
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Delete => Key::Delete,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::Enter => Key::Enter,
        KeyCode::Tab => Key::Tab,
        KeyCode::BackTab => Key::BackTab,
        _ => Key::Other,
    };
    (code, modified)
}

/// Redraw cached claim times and durations; database refresh uses invalidation hints.
const REFRESH: Duration = Duration::from_secs(1);
/// Animate running tools at ten frames per second, independently of database refresh.
const SPIN: Duration = Duration::from_millis(100);
/// Page keys move ten output rows in idle and busy views, preserving existing navigation.
const SCROLL_PAGE: u16 = 10;
/// A wheel tick moves three rows, as in monitor.
const WHEEL_ROWS: i16 = 3;

#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    /// The open request without a form: claim, choose a verdict or run a tool.
    Request,
    Form(Form),
    /// Keep or release the claims this session took without submitting.
    Leave,
}

/// The standalone review's two levels: the waiting list and the Detail of one request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    List,
    Detail,
}

/// Sub-areas of a Human review Detail; the focused one grows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Area {
    Instruction,
    Tools,
    Fields,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    None,
    Refresh,
    Quit,
    Start(Job),
    /// The standalone review turned mouse capture on or off (F2).
    Capture(bool),
}

struct Busy {
    label: String,
    since: Instant,
    cancel: CancellationToken,
}

impl Drop for Busy {
    fn drop(&mut self) {
        // A terminal error or unwinding must cancel an active Human tool too.
        self.cancel.cancel();
    }
}

/// Resolved command lines by tool name; a tool that cannot be resolved carries its error.
type Commands = std::collections::BTreeMap<ToolName, Result<CommandLine, String>>;

/// What the Detail holds. A Human request is a review; any other kind of request is only read.
enum Shown {
    Nothing,
    Human(Box<RequestView>),
    Record(Box<record::Record>),
}

impl Shown {
    fn request(&self) -> Option<&RequestView> {
        match self {
            Self::Human(view) => Some(view.as_ref()),
            _ => None,
        }
    }
    fn request_mut(&mut self) -> Option<&mut RequestView> {
        match self {
            Self::Human(view) => Some(view.as_mut()),
            _ => None,
        }
    }
    fn record(&self) -> Option<&record::Record> {
        match self {
            Self::Record(record) => Some(record.as_ref()),
            _ => None,
        }
    }
    fn record_mut(&mut self) -> Option<&mut record::Record> {
        match self {
            Self::Record(record) => Some(record.as_mut()),
            _ => None,
        }
    }
    fn take_record(&mut self) -> Option<record::Record> {
        match std::mem::replace(self, Self::Nothing) {
            Self::Record(record) => Some(*record),
            other => {
                *self = other;
                None
            }
        }
    }
}

/// Screen state over saved data; only jobs write, through the `human` lifecycle API.
pub struct Review {
    state: PathBuf,
    /// The standalone list's search: its text, what it parsed to, and while `/` edits it the
    /// text it had before.
    query: String,
    filter: Result<search::Query, String>,
    editing_query: Option<String>,
    /// Completion candidates of the query's last term, after Tab.
    candidates: Vec<String>,
    /// Tab asked to complete the query; the next refresh completes it before searching.
    completing: bool,
    /// Where each page shown so far starts, the current one last, and where the next starts.
    pages: Vec<u64>,
    next: Option<u64>,
    /// Numbers each session reader, so a stale reader job is recognised.
    serial: crate::monitor::session::ReaderId,
    reviewer: crate::types::ReviewerId,
    mode: Mode,
    /// The standalone level with focus; the Monitor keeps its own panes.
    focus: Focus,
    area: Area,
    /// The folded Technical section is shown.
    technical: bool,
    /// The standalone `review` owns the waiting list.
    standalone: bool,
    /// The requests the query found on the current page, and their searchable facts.
    listed: Vec<RequestView>,
    entries: Vec<search::Entry>,
    list: TableState,
    /// The open request, if any, and its last successfully loaded view.
    open: Option<RequestId>,
    /// What the Detail holds: the open Human request, or a read-only record of another kind.
    shown: Shown,
    tool: usize,
    /// The Tool log of the open request's runs, kept while it stays open.
    log: log::ToolLog,
    /// Where this review's runs log, and where their events arrive.
    sink: log::Sink,
    events: log::Events,
    /// The Tool log's top row while the reader has scrolled up; `None` follows the bottom.
    scroll: Option<u16>,
    /// The Tool log's last top row that still fills its pane, as last drawn.
    log_bottom: std::cell::Cell<u16>,
    /// Requests this session claimed and has neither submitted nor released.
    taken: Vec<RequestId>,
    /// Resolved command lines of the open request's tools, by tool name, and the request
    /// they were resolved for. Shown before a run; each run resolves again.
    commands: Option<(RequestId, Commands)>,
    busy: Option<Busy>,
    /// The last action's message and whether it is an error.
    notice: Option<(String, bool)>,
    refreshed: Option<OffsetDateTime>,
    error: Option<String>,
    /// Leave once nothing else waits after a submission.
    submitted: bool,
    /// Embedded mode keeps GREEN and RED drafts separate across verdict switches.
    drafts: std::collections::BTreeMap<crate::runtime::Verdict, Form>,
    field_scroll: u16,
    instruction_scroll: u16,
    /// Standalone mouse capture, on by default like monitor; F2 toggles it.
    mouse_capture: bool,
    /// Geometry of the last drawn standalone frame for mouse hit tests.
    hits: view::Hits,
}

impl Review {
    /// The list starts on the waiting Human reviews of every repository, or with `repo` of that
    /// repository only (its term in the query); `open` starts on one request.
    pub fn new(
        state: PathBuf,
        repo: Option<PathBuf>,
        reviewer: crate::types::ReviewerId,
        open: Option<RequestId>,
    ) -> Self {
        let repo = repo.map(|path| crate::workspace::canonical_target(&path).unwrap_or(path));
        let query = search::Query::text_for(repo.as_deref());
        let (sink, events) = log::channel();
        Self {
            state,
            filter: query.parse(),
            query,
            editing_query: None,
            candidates: Vec::new(),
            completing: false,
            pages: vec![0],
            next: None,
            serial: crate::monitor::session::ReaderId::default(),
            reviewer,
            mode: Mode::Request,
            focus: if open.is_some() {
                Focus::Detail
            } else {
                Focus::List
            },
            area: Area::Fields,
            technical: false,
            standalone: true,
            listed: Vec::new(),
            entries: Vec::new(),
            list: TableState::default(),
            open,
            shown: Shown::Nothing,
            tool: 0,
            log: log::ToolLog::default(),
            sink,
            events,
            scroll: None,
            log_bottom: std::cell::Cell::new(0),
            taken: Vec::new(),
            commands: None,
            busy: None,
            notice: None,
            refreshed: None,
            error: None,
            submitted: false,
            drafts: std::collections::BTreeMap::new(),
            field_scroll: 0,
            instruction_scroll: 0,
            mouse_capture: true,
            hits: view::Hits::default(),
        }
    }

    pub fn mode(&self) -> &Mode {
        &self.mode
    }

    pub fn focus(&self) -> Focus {
        self.focus
    }

    pub fn area(&self) -> Area {
        self.area
    }

    pub fn busy(&self) -> bool {
        self.busy.is_some()
    }

    /// Reload the visible screen read-only; on failure keep the last-known data.
    pub async fn refresh(&mut self) -> Action {
        // The standalone list stays beside the open request; monitor reads only the request.
        // Followers of later Runs collapse onto the original request, which records the Human
        // definition (tools and owner schemas) and receives every action, as in monitor.
        let mut result = Ok(());
        // Boxed: the search and Detail reads are deep futures, and refresh runs on test and
        // terminal stacks alike.
        if self.standalone {
            result = Box::pin(self.search()).await;
        }
        if let (Ok(()), Some(id)) = (&result, self.open.clone()) {
            let read = match store::read_request(&self.state, &id).await {
                Ok(view) if view.request.status().is_human_wait() => {
                    store::read_original(&self.state, &view).await
                }
                other => other,
            };
            result = match read {
                // A Human review opens the shared Human Detail; any other request is read-only.
                Ok(view) if view.request.profile.kind() == crate::config::ProfileKind::Human => {
                    self.open = Some(view.request.id.clone());
                    self.show(view);
                    Ok(())
                }
                Ok(view) => {
                    self.serial = self.serial.next();
                    let previous = self.shown.take_record();
                    Box::pin(record::Record::load(
                        &self.state,
                        view,
                        previous,
                        self.serial,
                    ))
                    .await
                    .map(|record| self.shown = Shown::Record(Box::new(record)))
                }
                Err(error) => Err(error),
            };
        }
        let now = OffsetDateTime::now_utc();
        match result {
            Ok(()) => {
                self.refreshed = Some(now);
                self.error = None;
            }
            Err(error) => {
                self.error = Some(format!(
                    "read failed at {}: {error}",
                    crate::monitor::clock(now)
                ));
                return Action::None;
            }
        }
        self.resolve_commands().await;
        self.tool = self.tool.min(self.tools().len().saturating_sub(1));
        if self.submitted && self.open.is_none() {
            self.submitted = false;
            // Nothing matches any more after the submission, on any page: the review is done.
            if self.listed.is_empty() {
                return Action::Quit;
            }
        }
        Action::None
    }

    /// Resolve the open waiting request's tool command lines once per request (and again after
    /// a run), so the Tools pane shows what Enter runs. Reads declarations; runs nothing.
    pub(crate) async fn resolve_commands(&mut self) {
        let Some(view) = self.shown.request() else {
            return;
        };
        let id = view.request.id.clone();
        if !view.request.status().is_human_wait()
            || view.request.human_definition.is_none()
            || self.commands.as_ref().is_some_and(|(seen, _)| seen == &id)
        {
            return;
        }
        let commands = async {
            let (receipts, _) = human::open(&self.state, &id).await?;
            let result = human::tool_commands(&receipts, &id).await;
            let closed = receipts.close().await;
            result.and_then(|commands| closed.map(|()| commands))
        }
        .await;
        // A request that cannot be resolved shows its tools as declared.
        self.commands = Some((id, commands.unwrap_or_default()));
    }

    /// The resolved command line of a tool of the open request, once known.
    pub(crate) fn command(&self, tool: &str) -> Option<&Result<CommandLine, String>> {
        self.commands
            .as_ref()
            .and_then(|(_, commands)| commands.get(tool))
    }

    /// Tools of the open request, from its saved Human definition.
    pub fn tools(&self) -> Vec<Tool> {
        self.shown
            .request()
            .map_or_else(Vec::new, |view| tools(&view.request))
    }

    fn mine(&self) -> bool {
        let claim = self.shown.request().and_then(|view| view.claim.as_ref());
        claim.is_some_and(|claim| claim.reviewer == self.reviewer)
    }

    /// The open request when this reviewer may act on it.
    fn actionable(&self) -> Result<(RequestId, bool), String> {
        actionable(self.shown.request(), &self.reviewer)
    }

    fn notify(&mut self, message: impl Into<String>, error: bool) -> Action {
        self.notice = Some((message.into(), error));
        Action::None
    }

    fn quit(&mut self) -> Action {
        if self.taken.is_empty() {
            return Action::Quit;
        }
        // The prompt never discards a form; Esc returns and g or r reopens the draft.
        self.stop_editing();
        self.mode = Mode::Leave;
        Action::None
    }

    /// Leave the form without discarding it; the same verdict reopens the draft.
    /// Only a form is set aside; any other mode, such as the quit prompt, stays.
    fn stop_editing(&mut self) {
        if !matches!(self.mode, Mode::Form(_)) {
            return;
        }
        if let Mode::Form(form) = std::mem::replace(&mut self.mode, Mode::Request) {
            self.drafts.insert(form.verdict, form);
        }
    }

    /// Standalone keys: the waiting list, then the shared Human review Detail. Esc steps back
    /// and never quits; q and Ctrl-C quit and offer to release this session's claims.
    pub fn key(&mut self, key: KeyEvent) -> Action {
        let interrupt =
            key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c');
        // F2 turns mouse capture on or off in any state, as in monitor; paste works either way.
        if key.code == KeyCode::F(2) {
            self.mouse_capture = !self.mouse_capture;
            return Action::Capture(self.mouse_capture);
        }
        if self.busy.is_some() {
            return match self.key_detail(key) {
                Handled::Action(action) => action,
                Handled::Back | Handled::Quit | Handled::Pass => Action::None,
            };
        }
        if interrupt {
            return if self.mode == Mode::Leave {
                Action::Quit
            } else {
                self.quit()
            };
        }
        if !matches!(self.mode, Mode::Form(_)) {
            self.notice = None;
        }
        if self.mode == Mode::Leave {
            return match key.code {
                KeyCode::Char('k' | 'n' | 'q') => Action::Quit,
                KeyCode::Char('u' | 'r' | 'y') => Action::Start(Job::Release {
                    ids: self.taken.clone(),
                    quit: true,
                }),
                KeyCode::Esc => {
                    self.mode = Mode::Request;
                    Action::None
                }
                _ => Action::None,
            };
        }
        match self.focus {
            Focus::List if self.editing_query.is_some() => self.query_key(key),
            Focus::List => self.list_key(key),
            Focus::Detail if self.shown.record().is_some() || self.shown.request().is_none() => {
                self.record_key(key)
            }
            Focus::Detail => match self.key_detail(key) {
                Handled::Action(action) => action,
                Handled::Back => {
                    self.focus = Focus::List;
                    Action::Refresh
                }
                Handled::Quit => self.quit(),
                // The standalone review has no help overlay or attention order.
                Handled::Pass => Action::None,
            },
        }
    }

    /// Standalone mouse: a click focuses and selects, a click on the selected list row opens it,
    /// and inside the Detail it acts like monitor's Human Detail; the wheel scrolls.
    pub fn mouse(&mut self, event: MouseEvent) -> Action {
        if !self.mouse_capture || self.mode == Mode::Leave {
            return Action::None;
        }
        let point = Position::new(event.column, event.row);
        let hits = std::mem::take(&mut self.hits);
        let action = self.mouse_at(&hits, event.kind, point);
        self.hits = hits;
        action
    }

    fn mouse_at(&mut self, hits: &view::Hits, kind: MouseEventKind, point: Position) -> Action {
        let in_detail = hits.detail.contains(point);
        match kind {
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let down = kind == MouseEventKind::ScrollDown;
                if in_detail {
                    let delta = if down { WHEEL_ROWS } else { -WHEEL_ROWS };
                    if hits.review.instruction.contains(point) {
                        self.scroll_instruction(delta);
                    } else if hits.review.fields.contains(point) {
                        self.scroll_single(delta, true);
                    } else if hits.review.tools.contains(point) {
                        self.scroll_single(delta, false);
                    }
                } else if hits.list.contains(point) && self.focus == Focus::List {
                    return self.list_key(KeyEvent::from(if down {
                        KeyCode::Down
                    } else {
                        KeyCode::Up
                    }));
                }
                Action::None
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if in_detail {
                    if hits.review.close.contains(point) {
                        if self.busy() {
                            return self.control(Control::Cancel);
                        }
                        return match self.key_detail(KeyEvent::from(KeyCode::Esc)) {
                            Handled::Back => {
                                self.focus = Focus::List;
                                Action::Refresh
                            }
                            Handled::Action(action) => action,
                            Handled::Quit | Handled::Pass => Action::None,
                        };
                    }
                    return self.click(&hits.review, point);
                }
                // A working review or an open form keeps the Detail.
                if self.focus == Focus::Detail && (self.busy() || self.editing()) {
                    return Action::None;
                }
                if hits.preview.contains(point) {
                    return match self.selected().cloned() {
                        Some(view) => self.open_view(view),
                        None => Action::None,
                    };
                }
                let Some(&(_, index)) = hits.rows.iter().find(|(rect, _)| rect.contains(point))
                else {
                    if hits.list.contains(point) {
                        self.focus = Focus::List;
                    }
                    return Action::None;
                };
                let open = self.focus == Focus::List && self.list.selected() == Some(index);
                self.focus = Focus::List;
                self.list.select(Some(index));
                match (open, self.listed.get(index).cloned()) {
                    (true, Some(view)) => self.open_view(view),
                    _ => Action::None,
                }
            }
            _ => Action::None,
        }
    }

    /// Open a listed request in Detail; another request starts from a clean review.
    fn open_view(&mut self, view: RequestView) -> Action {
        if view.request.profile.kind() != crate::config::ProfileKind::Human {
            // Read-only: the record loads with the refresh this returns.
            // A record of the same request stays, to be read again by the refresh.
            if self.open.as_ref() != Some(&view.request.id) || self.shown.request().is_some() {
                self.shown = Shown::Nothing;
            }
            self.open = Some(view.request.id.clone());
            self.focus = Focus::Detail;
            return Action::Refresh;
        }
        if self.open.as_ref() != Some(&view.request.id) {
            self.mode = Mode::Request;
            self.area = Area::Fields;
            self.tool = 0;
            self.log.clear();
            self.scroll = None;
            self.field_scroll = 0;
            self.instruction_scroll = 0;
            self.drafts.clear();
        }
        self.open = Some(view.request.id.clone());
        self.shown = Shown::Human(Box::new(view));
        self.focus = Focus::Detail;
        Action::Refresh
    }

    fn move_tool(&mut self, down: bool) {
        self.tool = if down {
            (self.tool + 1).min(self.tools().len().saturating_sub(1))
        } else {
            self.tool.saturating_sub(1)
        };
    }

    /// Run the selected tool at once; the run itself checks the claim and resolves the command.
    fn run_selected(&mut self) -> Action {
        let Some(tool) = self.tools().into_iter().nth(self.tool) else {
            return self.notify("This request declares no Human tools.", true);
        };
        self.run(tool.name)
    }

    /// The open request when this reviewer holds its claim. Jobs never claim implicitly: a
    /// claim is always the explicit `c`.
    fn owned_request(&self) -> Result<RequestId, String> {
        match self.actionable()? {
            (id, false) => Ok(id),
            (_, true) => Err(CLAIM_FIRST.into()),
        }
    }

    fn release(&mut self) -> Action {
        match (self.actionable(), self.mine()) {
            (Ok((id, _)), true) => Action::Start(Job::Release {
                ids: vec![id],
                quit: false,
            }),
            (Ok(_), false) => self.notify(NOT_CLAIMED, true),
            (Err(error), _) => self.notify(error, true),
        }
    }

    fn run(&mut self, tool: ToolName) -> Action {
        match self.owned_request() {
            Ok(id) => Action::Start(Job::Run {
                id,
                tool,
                claim: false,
            }),
            Err(error) => self.notify(error, true),
        }
    }

    fn submit(&mut self, result: Value) -> Action {
        match self.owned_request().and_then(|id| {
            artifactize_core::review::Submission::try_from(result).map(|result| (id, result))
        }) {
            Ok((id, result)) => Action::Start(Job::Submit {
                id,
                result,
                claim: false,
            }),
            Err(error) => {
                if let Mode::Form(form) = &mut self.mode {
                    form.error = Some(error);
                }
                Action::None
            }
        }
    }

    /// Mark the screen busy and return the job's future; feed its outcome to `finish`.
    pub fn start(&mut self, job: Job) -> impl Future<Output = Outcome> + 'static {
        let cancel = CancellationToken::new();
        let label = match &job {
            Job::Claim { .. } => "Claiming the review".into(),
            Job::Run { tool, .. } => format!("Running {tool}"),
            Job::Submit { result, .. } => format!("Submitting {}", result.verdict().as_str()),
            Job::Release { .. } => "Releasing the claim".into(),
        };
        self.busy = Some(Busy {
            label,
            since: Instant::now(),
            cancel: cancel.clone(),
        });
        let state = self.state.clone();
        let reviewer = self.reviewer.clone();
        let sink = self.sink.clone();
        async move { job.run(state, reviewer, cancel, sink).await }
    }

    /// The next Tool log event of this review's runs, for the screen's event loop.
    pub(crate) async fn log_event(&mut self) -> log::Event {
        self.events
            .recv()
            .await
            .expect("the review holds its own sink")
    }

    /// Take a Tool log event in, and any others already waiting. Events of a request this
    /// review no longer shows are dropped.
    pub(crate) fn logged(&mut self, event: log::Event) {
        if self.open.as_ref() == event.key.request() {
            self.log.apply(event);
        }
        self.drain_log();
    }

    /// Take in the Tool log events already waiting.
    pub(crate) fn drain_log(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            if self.open.as_ref() == event.key.request() {
                self.log.apply(event);
            }
        }
    }

    /// Remember a claim this session took on the open request.
    fn took(&mut self, claim: Option<HumanClaim>) {
        if let (Some(_), Some(id)) = (claim, &self.open)
            && !self.taken.contains(id)
        {
            self.taken.push(id.clone());
        }
    }

    /// Apply a job's outcome.
    pub fn finish(&mut self, outcome: Outcome) -> Action {
        self.busy = None;
        let notice = outcome.notice();
        match outcome {
            Outcome::SavedLocally { request, .. } => {
                self.taken.retain(|id| id != &request.id);
                if let Some(view) = self.shown.request_mut() {
                    view.request = *request;
                    view.claim = None;
                }
                self.mode = Mode::Request;
                self.notice = Some(notice);
                Action::Refresh
            }
            Outcome::Claimed { result } => {
                if let Ok(claim) = result {
                    self.took(Some(claim.clone()));
                    if let Some(view) = self.shown.request_mut() {
                        view.claim = Some(claim);
                    }
                    if !matches!(self.mode, Mode::Form(_)) {
                        self.mode = Mode::Request;
                    }
                }
                self.notice = Some(notice);
                Action::Refresh
            }
            Outcome::Ran { claimed, .. } => {
                self.took(claimed);
                // Declarations may have changed; show the command the next run resolves.
                self.commands = None;
                self.notice = Some(notice);
                Action::Refresh
            }
            Outcome::Submitted {
                id,
                claimed,
                result,
            } => {
                self.took(claimed);
                match result {
                    Ok(_) => {
                        self.taken.retain(|taken| *taken != id);
                        self.notice = Some(notice);
                        self.open = None;
                        self.shown = Shown::Nothing;
                        self.log.clear();
                        self.mode = Mode::Request;
                        self.focus = Focus::List;
                        self.submitted = true;
                    }
                    Err(error) => {
                        if let Mode::Form(form) = &mut self.mode {
                            form.error = Some(error);
                        } else {
                            self.notice = Some(notice);
                        }
                    }
                }
                Action::Refresh
            }
            Outcome::Released { ids, quit, failed } => {
                self.taken.retain(|taken| !ids.contains(taken));
                if failed.is_empty() {
                    if quit {
                        return Action::Quit;
                    }
                } else {
                    self.taken.extend(failed.into_iter().map(|(id, _)| id));
                }
                self.notice = Some(notice);
                Action::Refresh
            }
        }
    }

    /// A bracketed paste goes to the focused field of the open review, as in monitor.
    /// Paste into the search being edited (one line, searched from the first page like typed
    /// text) or into the focused form field.
    pub fn paste(&mut self, text: &str) -> Action {
        if self.focus == Focus::List && self.editing_query.is_some() {
            return self.paste_query(text);
        }
        if self.focus == Focus::Detail && self.area == Area::Fields {
            self.paste_single(text);
        }
        Action::None
    }
}
