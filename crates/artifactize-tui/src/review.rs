//! Human review TUI: claim, run Human tools and submit, in-process like `request`.

mod detail;
mod detail_input;
mod embedded;
#[cfg(test)]
mod embedded_tests;
#[cfg(test)]
mod identity_tests;
mod input;
mod jobs;
mod list;
#[cfg(test)]
mod list_tests;
mod record;
mod terminal;
#[cfg(test)]
pub(crate) mod tests;
mod view;

pub use artifactize_core::review::*;
pub(crate) use detail::Hits;
pub(crate) use detail_input::Handled;
pub(crate) use embedded::Control;
pub(crate) use input::form_key;
use jobs::Busy;
pub use terminal::run;

use crate::config::ToolName;
use std::{path::PathBuf, time::Duration};

use ratatui::widgets::TableState;
use time::OffsetDateTime;

use crate::{
    human,
    store::{self, RequestView},
    tools::human::CommandLine,
    types::RequestId,
};

/// Redraw cached claim times and durations; database refresh uses invalidation hints.
const REFRESH: Duration = Duration::from_secs(1);
/// Animate running tools at ten frames per second, independently of database refresh.
const SPIN: Duration = Duration::from_millis(100);
/// Page keys move ten output rows in idle and busy views, preserving existing navigation.
const SCROLL_PAGE: u16 = 10;

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
}
