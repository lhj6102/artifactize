//! Human review TUI: claim, run Human tools and submit, in-process like `request`.

mod detail;
mod embedded;
#[cfg(test)]
mod embedded_tests;
mod form;
#[cfg(test)]
mod identity_tests;
#[cfg(test)]
pub(crate) mod tests;
mod view;

pub(crate) use detail::{Handled, Hits};
pub(crate) use embedded::Control;
pub use form::{Field, Form, Input, template};

use std::{
    future::Future,
    io::Read,
    path::PathBuf,
    pin::Pin,
    time::{Duration, Instant},
};

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures_util::StreamExt;
use ratatui::widgets::TableState;
use serde_json::Value;
use time::OffsetDateTime;
use tokio_util::sync::CancellationToken;

use crate::{
    config::HumanToolKind,
    human,
    store::{self, HumanClaim, Request, RequestView},
    tools::human::{CommandLine, Content, ToolResult},
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
    /// The first run of a command line in this session waits for confirmation.
    Confirm {
        tool: String,
        command: CommandLine,
    },
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

/// Lifecycle work with owned inputs, so the screen keeps drawing while it runs.
#[derive(Debug, Clone, PartialEq)]
pub enum Job {
    /// Explicit claim for an embedded single-request review.
    Claim { id: RequestId },
    /// Resolve a tool's command line for confirmation; claims nothing.
    Inspect { id: RequestId, tool: String },
    /// Claim first when `claim`, then run the tool.
    Run {
        id: RequestId,
        tool: String,
        claim: bool,
    },
    /// Claim first when `claim`, then submit and publish like `request submit`.
    Submit {
        id: RequestId,
        result: Value,
        claim: bool,
    },
    /// Release the reviewer's remaining claims; quit afterwards when `quit`.
    Release { ids: Vec<RequestId>, quit: bool },
}

#[derive(Debug)]
pub enum Outcome {
    Claimed {
        result: Result<HumanClaim, String>,
    },
    /// Publishing failed after settlement; never offer to submit the verdict twice.
    SavedLocally {
        request: Box<Request>,
        error: String,
    },
    Inspected {
        tool: String,
        result: Result<CommandLine, String>,
    },
    Ran {
        tool: String,
        claimed: Option<HumanClaim>,
        result: Result<ToolResult, String>,
    },
    Submitted {
        id: RequestId,
        claimed: Option<HumanClaim>,
        result: Result<Box<Request>, String>,
    },
    Released {
        ids: Vec<RequestId>,
        quit: bool,
        /// Request IDs whose release failed, with the error.
        failed: Vec<(RequestId, String)>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    None,
    Refresh,
    Quit,
    Start(Job),
    /// Suspend the screen and edit this JSON in `$EDITOR`.
    Edit(String),
}

/// A declared Human tool as the saved definition records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tool {
    pub name: String,
    pub kind: HumanToolKind,
    pub description: String,
    /// Declared command and args, placeholders unresolved.
    pub declared: String,
}

/// The last tool result or failure, shown in the output pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub title: String,
    pub text: String,
    pub error: bool,
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

/// Screen state over saved data; only jobs write, through the `human` lifecycle API.
pub struct Review {
    state: PathBuf,
    repo: Option<PathBuf>,
    reviewer: String,
    mode: Mode,
    /// The standalone level with focus; monitor keeps its own panes.
    focus: Focus,
    area: Area,
    /// The folded Technical section is shown.
    technical: bool,
    /// The standalone `review` owns the waiting list and may suspend for `$EDITOR`.
    standalone: bool,
    waiting: Vec<RequestView>,
    list: TableState,
    /// The open request, if any, and its last successfully loaded view.
    open: Option<RequestId>,
    /// An invalid compatibility constructor ID never enters state reads or jobs.
    invalid_open: Option<String>,
    request: Option<RequestView>,
    tool: usize,
    output: Option<Output>,
    scroll: u16,
    /// Requests this session claimed and has neither submitted nor released.
    taken: Vec<RequestId>,
    /// Command lines the reviewer confirmed in this session.
    confirmed: Vec<CommandLine>,
    busy: Option<Busy>,
    /// The last action's message and whether it is an error.
    notice: Option<(String, bool)>,
    refreshed: Option<OffsetDateTime>,
    error: Option<String>,
    /// Leave once nothing else waits after a submission.
    submitted: bool,
    /// Embedded mode keeps GREEN and RED drafts separate across verdict switches.
    drafts: std::collections::BTreeMap<&'static str, Form>,
    field_scroll: u16,
    instruction_scroll: u16,
    /// A Human tool confirmation temporarily replaces, but never discards, its form.
    tool_draft: Option<Form>,
}

impl Review {
    /// `repo: None` lists waiting requests from every repository; `open` starts on one request.
    pub fn new(
        state: PathBuf,
        repo: Option<PathBuf>,
        reviewer: String,
        open: Option<String>,
    ) -> Self {
        let (open, invalid_open) = match open.map(|id| id.parse::<RequestId>()).transpose() {
            Ok(open) => (open, None),
            Err(error) => (None, Some(error)),
        };
        Self {
            state,
            repo,
            reviewer,
            mode: Mode::Request,
            focus: if open.is_some() || invalid_open.is_some() {
                Focus::Detail
            } else {
                Focus::List
            },
            area: Area::Fields,
            technical: false,
            standalone: true,
            waiting: Vec::new(),
            list: TableState::default(),
            open,
            invalid_open,
            request: None,
            tool: 0,
            output: None,
            scroll: 0,
            taken: Vec::new(),
            confirmed: Vec::new(),
            busy: None,
            notice: None,
            refreshed: None,
            error: None,
            submitted: false,
            drafts: std::collections::BTreeMap::new(),
            field_scroll: 0,
            instruction_scroll: 0,
            tool_draft: None,
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
        if let Some(error) = &self.invalid_open {
            self.error = Some(error.clone());
            return Action::None;
        }
        // The standalone list stays beside the open request; monitor reads only the request.
        let mut result = Ok(());
        if self.standalone {
            result = store::read_waiting(&self.state, self.repo.as_deref())
                .await
                .map(|waiting| self.set_waiting(waiting));
        }
        if let (Ok(()), Some(id)) = (&result, self.open.clone()) {
            result = store::read_request(&self.state, &id)
                .await
                .map(|view| self.show(view));
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
        self.tool = self.tool.min(self.tools().len().saturating_sub(1));
        if self.submitted && self.open.is_none() {
            self.submitted = false;
            if self.waiting.is_empty() {
                return Action::Quit;
            }
        }
        Action::None
    }

    fn set_waiting(&mut self, waiting: Vec<RequestView>) {
        let selected = self.selected().map(|view| view.request.id.clone());
        let index = selected
            .and_then(|id| waiting.iter().position(|view| view.request.id == id))
            .or(self.list.selected());
        self.waiting = waiting;
        self.list.select(
            index
                .map(|index| index.min(self.waiting.len().saturating_sub(1)))
                .or(Some(0))
                .filter(|_| !self.waiting.is_empty()),
        );
    }

    fn selected(&self) -> Option<&RequestView> {
        self.list
            .selected()
            .and_then(|index| self.waiting.get(index))
    }

    /// Tools of the open request, from its saved Human definition.
    pub fn tools(&self) -> Vec<Tool> {
        self.request
            .as_ref()
            .map_or_else(Vec::new, |view| tools(&view.request))
    }

    fn mine(&self) -> bool {
        let claim = self.request.as_ref().and_then(|view| view.claim.as_ref());
        claim.is_some_and(|claim| claim.reviewer == self.reviewer)
    }

    /// The open request when this reviewer may act on it.
    fn actionable(&self) -> Result<(RequestId, bool), String> {
        let view = self
            .request
            .as_ref()
            .ok_or("The request is still loading.")?;
        if view.request.status != crate::types::RequestStatus::WaitingHuman {
            return Err(format!(
                "The request is {}; there is nothing to review.",
                view.request.status
            ));
        }
        match &view.claim {
            Some(claim) if claim.reviewer != self.reviewer => Err(format!(
                "Claimed by {}; read-only for {}.",
                claim.reviewer, self.reviewer
            )),
            claim => Ok((view.request.id.clone(), claim.is_none())),
        }
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
    fn stop_editing(&mut self) {
        if let Mode::Form(form) = std::mem::replace(&mut self.mode, Mode::Request) {
            self.drafts.insert(form.verdict, form);
        }
    }

    /// Standalone keys: the waiting list, then the shared Human review Detail. Esc steps back
    /// and never quits; q and Ctrl-C quit and offer to release this session's claims.
    pub fn key(&mut self, key: KeyEvent) -> Action {
        let interrupt =
            key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c');
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
            Focus::List => self.list_key(key),
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

    fn list_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Char('q') => self.quit(),
            KeyCode::Char('r') => Action::Refresh,
            KeyCode::Down | KeyCode::Char('j') => {
                let next = self.list.selected().map_or(0, |index| index + 1);
                if next < self.waiting.len() {
                    self.list.select(Some(next));
                }
                Action::None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.list.select_previous();
                Action::None
            }
            KeyCode::Enter | KeyCode::Right => match self.selected().cloned() {
                Some(view) => self.open_view(view),
                None => Action::None,
            },
            _ => Action::None,
        }
    }

    /// Open a listed request in Detail; another request starts from a clean review.
    fn open_view(&mut self, view: RequestView) -> Action {
        if self.open.as_ref() != Some(&view.request.id) {
            self.mode = Mode::Request;
            self.area = Area::Fields;
            self.tool = 0;
            self.output = None;
            self.scroll = 0;
            self.field_scroll = 0;
            self.instruction_scroll = 0;
            self.drafts.clear();
            self.tool_draft = None;
        }
        self.open = Some(view.request.id.clone());
        self.invalid_open = None;
        self.request = Some(view);
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

    /// Resolve the selected tool's command line; the run waits for its confirmation.
    fn inspect(&mut self) -> Action {
        let Some(tool) = self.tools().into_iter().nth(self.tool) else {
            return self.notify("This request declares no Human tools.", true);
        };
        match self.actionable() {
            Ok((id, _)) => Action::Start(Job::Inspect {
                id,
                tool: tool.name,
            }),
            Err(error) => self.notify(error, true),
        }
    }

    /// Answer the first-run confirmation; declining brings back a form set aside for the tool.
    fn confirm(&mut self, run: bool) -> Action {
        let Mode::Confirm { tool, command } = std::mem::replace(&mut self.mode, Mode::Request)
        else {
            return Action::None;
        };
        if run {
            self.confirmed.push(command);
            return self.run(tool);
        }
        if let Some(form) = self.tool_draft.take() {
            self.mode = Mode::Form(form);
        }
        Action::None
    }

    fn release(&mut self) -> Action {
        match (self.actionable(), self.mine()) {
            (Ok((id, _)), true) => Action::Start(Job::Release {
                ids: vec![id],
                quit: false,
            }),
            (Ok(_), false) => self.notify("The request is not claimed.", true),
            (Err(error), _) => self.notify(error, true),
        }
    }

    fn run(&mut self, tool: String) -> Action {
        match self.actionable() {
            Ok((id, claim)) => Action::Start(Job::Run { id, tool, claim }),
            Err(error) => self.notify(error, true),
        }
    }

    fn submit(&mut self, result: Value) -> Action {
        match self.actionable() {
            Ok((id, claim)) => Action::Start(Job::Submit { id, result, claim }),
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
            Job::Inspect { tool, .. } => format!("Resolving {tool}"),
            Job::Run { tool, .. } => format!("Running {tool}"),
            Job::Submit { result, .. } => {
                format!(
                    "Submitting {}",
                    result["verdict"].as_str().unwrap_or_default()
                )
            }
            Job::Release { .. } => "Releasing the claim".into(),
        };
        self.busy = Some(Busy {
            label,
            since: Instant::now(),
            cancel: cancel.clone(),
        });
        let state = self.state.clone();
        let reviewer = self.reviewer.clone();
        async move { job.run(state, reviewer, cancel).await }
    }

    /// Remember a claim this session took on the open request.
    fn took(&mut self, claim: Option<HumanClaim>) {
        if let (Some(_), Some(id)) = (claim, &self.open)
            && !self.taken.contains(id)
        {
            self.taken.push(id.clone());
        }
    }

    /// Apply a job's outcome; a form set aside for a tool run comes back afterwards.
    pub fn finish(&mut self, outcome: Outcome) -> Action {
        let restore_form = matches!(
            &outcome,
            Outcome::Ran { .. } | Outcome::Inspected { result: Err(_), .. }
        );
        let action = self.settle(outcome);
        if restore_form && let Some(form) = self.tool_draft.take() {
            self.mode = Mode::Form(form);
        }
        action
    }

    fn settle(&mut self, outcome: Outcome) -> Action {
        self.busy = None;
        match outcome {
            Outcome::SavedLocally { request, error } => {
                self.taken.retain(|id| id != &request.id);
                if let Some(view) = &mut self.request {
                    view.request = *request;
                    view.claim = None;
                }
                self.mode = Mode::Request;
                self.notice = Some((error, true));
                Action::Refresh
            }
            Outcome::Claimed { result } => {
                match result {
                    Ok(claim) => {
                        self.took(Some(claim.clone()));
                        if let Some(view) = &mut self.request {
                            view.claim = Some(claim);
                        }
                        if !matches!(self.mode, Mode::Form(_)) {
                            self.mode = Mode::Request;
                        }
                        self.notice = Some((
                            "Claimed. Fill GREEN or RED fields, then Submit.".into(),
                            false,
                        ));
                    }
                    Err(error) => self.notice = Some((error, true)),
                }
                Action::Refresh
            }
            Outcome::Inspected { tool, result } => match result {
                Ok(command) if self.confirmed.contains(&command) => self.run(tool),
                Ok(command) => {
                    self.mode = Mode::Confirm { tool, command };
                    Action::None
                }
                Err(error) => self.notify(format!("{tool}: {error}"), true),
            },
            Outcome::Ran {
                tool,
                claimed,
                result,
            } => {
                self.took(claimed);
                let launched = Content::Launch { launched: true };
                self.notice = Some(match &result {
                    Ok(result) if result.is_error => (
                        format!("{tool} failed; a tool error is not a verdict."),
                        true,
                    ),
                    Ok(result) if result.content.contains(&launched) => {
                        (format!("{tool} launched."), false)
                    }
                    Ok(_) => (format!("{tool} finished."), false),
                    Err(_) => (format!("{tool} did not run."), true),
                });
                self.output = Some(output(&tool, result));
                self.scroll = 0;
                Action::Refresh
            }
            Outcome::Submitted {
                id,
                claimed,
                result,
            } => {
                self.took(claimed);
                match result {
                    Ok(request) => {
                        self.taken.retain(|taken| *taken != id);
                        self.notice = Some((
                            format!(
                                "Submitted {} for {} ({}).",
                                request.status, request.eval_id, request.id
                            ),
                            false,
                        ));
                        self.open = None;
                        self.request = None;
                        self.output = None;
                        self.mode = Mode::Request;
                        self.focus = Focus::List;
                        self.submitted = true;
                    }
                    Err(error) => {
                        if let Mode::Form(form) = &mut self.mode {
                            form.error = Some(error);
                        } else {
                            self.notice = Some((error, true));
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
                    self.notice = Some(("Claim released.".into(), false));
                } else {
                    let lines = failed.iter().map(|(id, error)| format!("{id}: {error}"));
                    self.notice = Some((lines.collect::<Vec<_>>().join("\n"), true));
                    self.taken.extend(failed.into_iter().map(|(id, _)| id));
                }
                Action::Refresh
            }
        }
    }

    /// Feed back the text `$EDITOR` saved; a non-empty object is submitted at once.
    pub fn edited(&mut self, text: Result<String, String>) -> Action {
        let Mode::Form(form) = &mut self.mode else {
            return Action::None;
        };
        match text {
            Err(error) => form.error = Some(error),
            Ok(text) if text.trim().is_empty() => {
                form.error = Some("The edited file was empty; nothing was submitted.".into())
            }
            Ok(text) => {
                form.json = Some(text);
                match form.result() {
                    Ok(result) => return self.submit(result),
                    Err(error) => form.error = Some(error),
                }
            }
        }
        Action::None
    }
}

impl Job {
    async fn run(self, state: PathBuf, reviewer: String, cancel: CancellationToken) -> Outcome {
        let mut claimed = None;
        match self {
            Job::Claim { id } => {
                let result = async {
                    let (receipts, _) = human::open(&state, &id).await?;
                    human::claim(&receipts, &id, &reviewer).await
                }
                .await;
                Outcome::Claimed { result }
            }
            Job::Inspect { id, tool } => {
                let result = async {
                    let (receipts, _) = human::open(&state, &id).await?;
                    human::tool_command(&receipts, &id, &tool).await
                };
                Outcome::Inspected {
                    result: result.await,
                    tool,
                }
            }
            Job::Run {
                id,
                tool,
                claim: first,
            } => {
                let result = async {
                    let (receipts, _) = human::open(&state, &id).await?;
                    if first {
                        claimed = Some(human::claim(&receipts, &id, &reviewer).await?);
                    }
                    human::run_human_tool(&receipts, &id, &reviewer, &tool, cancel).await
                }
                .await;
                Outcome::Ran {
                    tool,
                    claimed,
                    result,
                }
            }
            Job::Submit {
                id,
                result,
                claim: first,
            } => {
                let result = async {
                    if first {
                        let (receipts, _) = human::open(&state, &id).await?;
                        claimed = Some(human::claim(&receipts, &id, &reviewer).await?);
                    }
                    human::submit_and_publish(&state, &id, &reviewer, &result, cancel)
                        .await
                        .map(Box::new)
                }
                .await;
                if let Err(error) = &result
                    && let Ok(view) = store::read_request(&state, &id).await
                    && matches!(
                        view.request.status,
                        crate::types::RequestStatus::Green | crate::types::RequestStatus::Red
                    )
                    && view.execution.as_ref().is_some_and(|execution| {
                        execution.reviewer.as_deref() == Some(reviewer.as_str())
                    })
                {
                    return Outcome::SavedLocally {
                        request: Box::new(view.request),
                        error: error.clone(),
                    };
                }
                Outcome::Submitted {
                    id,
                    claimed,
                    result,
                }
            }
            Job::Release { ids, quit } => {
                let mut failed = Vec::new();
                for id in &ids {
                    let release = async {
                        // Settled requests and claims released elsewhere leave nothing to release.
                        let view = store::read_request(&state, id).await?;
                        if view.claim.is_none_or(|claim| claim.reviewer != reviewer) {
                            return Ok(());
                        }
                        let (receipts, _) = human::open(&state, id).await?;
                        human::unclaim(&receipts, id, &reviewer).await.map(drop)
                    };
                    if let Err(error) = release.await {
                        failed.push((id.clone(), error));
                    }
                }
                Outcome::Released { ids, quit, failed }
            }
        }
    }
}

/// Declared Human tools in the eval scope, named like the registry.
pub fn tools(request: &Request) -> Vec<Tool> {
    let definition = request.human_definition.as_ref();
    let artifacts = definition.and_then(|definition| definition["artifacts"].as_object());
    let mut tools: Vec<_> = artifacts
        .into_iter()
        .flatten()
        .flat_map(|(id, artifact)| {
            let declared = artifact["views"]["humanTools"]
                .as_object()
                .into_iter()
                .flatten();
            declared.filter_map(move |(operation, tool)| {
                let words = std::iter::once(&tool["command"])
                    .chain(tool["args"].as_array().into_iter().flatten())
                    .filter_map(Value::as_str);
                Some(Tool {
                    name: format!("{operation}_{id}"),
                    kind: serde_json::from_value(tool["kind"].clone()).ok()?,
                    description: tool["description"]
                        .as_str()
                        .unwrap_or_default()
                        .replace("{artifactName}", id),
                    declared: shell(words),
                })
            })
        })
        .collect();
    tools.sort_by(|a, b| a.name.cmp(&b.name));
    tools
}

/// Words joined for display, single-quoted when the shell would split or expand them.
pub fn shell<'a>(words: impl IntoIterator<Item = &'a str>) -> String {
    let quote = |word: &str| {
        let plain = !word.is_empty()
            && word
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_./=:,+@%{}".contains(c));
        if plain {
            word.to_owned()
        } else {
            format!("'{}'", word.replace('\'', r"'\''"))
        }
    };
    words.into_iter().map(quote).collect::<Vec<_>>().join(" ")
}

fn output(tool: &str, result: Result<ToolResult, String>) -> Output {
    match result {
        Ok(result) => Output {
            title: if result.is_error {
                format!("{tool} · tool error")
            } else {
                tool.to_owned()
            },
            text: result
                .content
                .iter()
                .map(|content| match content {
                    Content::Text { text } => text.trim_end().to_owned(),
                    Content::Launch { launched: true } => "launched".into(),
                    Content::Launch { launched: false } => "not launched".into(),
                })
                .collect::<Vec<_>>()
                .join("\n"),
            error: result.is_error,
        },
        Err(error) => Output {
            title: format!("{tool} · failed"),
            text: error,
            error: true,
        },
    }
}

/// Edit text in `$EDITOR` (default `vi`, Notepad on Windows) on the restored terminal; bounded
/// like `--fields-file`.
async fn edit(text: String) -> Result<String, String> {
    let file = tempfile::Builder::new()
        .prefix("artifactize-fields-")
        .suffix(".json")
        .tempfile()
        .map_err(|e| e.to_string())?;
    std::fs::write(file.path(), text).map_err(|e| e.to_string())?;
    let editor = std::env::var("EDITOR")
        .ok()
        .filter(|editor| !editor.trim().is_empty())
        .unwrap_or_else(|| crate::platform::DEFAULT_EDITOR.into());
    let status = crate::platform::editor(&editor, file.path())
        .status()
        .await
        .map_err(|e| format!("cannot start the editor: {e}"))?;
    if !status.success() {
        return Err(format!(
            "The editor exited unsuccessfully ({status}); nothing was submitted."
        ));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(file.path())
        .and_then(|file| {
            file.take(crate::human::FIELDS_READ_BYTES)
                .read_to_end(&mut bytes)
        })
        .map_err(|e| e.to_string())?;
    if bytes.len() > crate::human::MAX_RESULT_BYTES {
        return Err("Human fields exceed 256000 bytes.".into());
    }
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

/// Run the review UI until q, a signal, or a submission that leaves nothing waiting.
pub async fn run(
    state: PathBuf,
    repo: Option<PathBuf>,
    reviewer: String,
    open: Option<String>,
    cancellation: CancellationToken,
) -> Result<(), String> {
    // ratatui's panic hook restores the terminal before a panic message is printed.
    let mut terminal = ratatui::try_init().map_err(|e| {
        let _ = crossterm::terminal::disable_raw_mode();
        format!("cannot start the review: {e}")
    })?;
    let review = Review::new(state, repo, reviewer, open);
    let result = drive(&mut terminal, review, cancellation).await;
    drop(terminal);
    ratatui::try_restore().map_err(|e| e.to_string())?;
    result
}

async fn drive(
    terminal: &mut ratatui::DefaultTerminal,
    mut review: Review,
    cancellation: CancellationToken,
) -> Result<(), String> {
    let mut changes = crate::changes::Subscription::new(&review.state).await;
    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(REFRESH);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut spin = tokio::time::interval(SPIN);
    let mut pending: Option<Pin<Box<dyn Future<Output = Outcome>>>> = None;
    let mut action = Action::Refresh;
    loop {
        loop {
            action = match action {
                Action::None => break,
                Action::Quit => return Ok(()),
                Action::Refresh => {
                    tick.reset();
                    review.refresh().await
                }
                Action::Start(job) => {
                    pending = Some(Box::pin(review.start(job)));
                    Action::None
                }
                Action::Edit(text) => {
                    // The editor owns the terminal; no event reader may compete for its input.
                    drop(events);
                    let edited = crate::monitor::suspend(terminal, edit(text)).await;
                    events = EventStream::new();
                    review.edited(edited?)
                }
            };
        }
        terminal
            .draw(|frame| review.draw(frame))
            .map_err(|e| e.to_string())?;
        action = tokio::select! {
            _ = cancellation.cancelled() => Action::Quit,
            outcome = async { pending.as_mut().expect("pending job").await }, if pending.is_some() => {
                pending = None;
                if matches!(outcome, Outcome::Submitted { .. }) {
                    // Remote and cache warnings may have reached stderr; repaint every cell.
                    crate::monitor::repaint(terminal)?;
                }
                review.finish(outcome)
            }
            _ = spin.tick(), if review.busy() => Action::None,
            _ = tick.tick() => Action::None,
            change = changes.next() => match change {
                crate::changes::Change::StateInvalidated | crate::changes::Change::Resync => Action::Refresh,
                crate::changes::Change::SessionInvalidated(_) => Action::None,
            },
            event = events.next() => match event {
                Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => review.key(key),
                Some(Ok(_)) => Action::None,
                Some(Err(error)) => return Err(error.to_string()),
                None => Action::Quit,
            },
        };
    }
}
