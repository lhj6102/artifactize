//! The shared Human Detail's backend: the open request as the terminal review shows it, and
//! claim, tool, submit and release through the same [`Job`]s as `artifactize review`.
//!
//! Only these jobs write. Drafts live in the frontend; a submission sends their values and
//! the review's own [`Form`] turns them into the result, so typed fields, coercion and local
//! errors are the terminal's.
pub(crate) mod draft;
mod form;
mod jobs;
pub(crate) mod log;
pub(crate) mod search;
mod searches;

pub use form::{Draft, FormView, Verdict};
use jobs::{Action, Jobs};
pub(crate) use jobs::{Caller, Lease};

use crate::ids::{JobId, RequestKey};
use crate::{
    App, Failure,
    detail::{Field, ResultEntry},
    status::Mark,
};
use artifactize_core::{
    config::{HumanToolKind, ToolName},
    monitor,
    review::{self, Job, Outcome},
    store::{self, RequestView},
    tools::human::CommandLine,
    types::RequestId,
};
use serde::Serialize;
use specta::Type;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Mutex,
};
use time::OffsetDateTime;
use tokio_util::sync::CancellationToken;

/// Resolved command lines of a request's tools, by tool name.
type Commands = BTreeMap<ToolName, Result<CommandLine, String>>;
/// What resolving a request's tool commands answered: them, or why it could not.
type Resolved = Result<Commands, String>;

/// Request reads numbered in the order their database reads ran, the waiting stack's
/// included: of two views of a request, the one with the higher number is the newer.
/// Reads answer in any order, so a read that answers late never replaces a newer view.
#[derive(Default)]
pub(crate) struct Reads(tokio::sync::Mutex<u32>);

impl Reads {
    /// Run the database read `read` after the reads numbered before it; its number and what
    /// it read.
    pub(crate) async fn numbered<T>(&self, read: impl Future<Output = T>) -> (u32, T) {
        let mut last = self.0.lock().await;
        let value = read.await;
        *last += 1;
        (*last, value)
    }
}

/// Keep the referenced request for the header and Back link, and its owner for review actions.
#[derive(Clone)]
struct ReadReview {
    generation: u32,
    exact: RequestView,
    owner: RequestView,
}

/// Per-app review state: what is cached for drawing, running jobs, claims taken here, and
/// the Tool log every run logs to.
pub struct Reviews {
    /// The newest read view of each opened request and its read's number, for redraws
    /// without a read.
    views: Mutex<BTreeMap<RequestId, (u32, ReadReview)>>,
    /// Numbers every read of requests.
    pub(crate) reads: Reads,
    searches: searches::Searches,
    /// Resolved tool command lines, resolved once per request and again after each run.
    commands: Mutex<BTreeMap<RequestId, Resolved>>,
    jobs: Jobs,
    /// A shutdown snapshot must not race the database claim or its local ownership record.
    pub(crate) claim_gate: tokio::sync::Mutex<()>,
    /// Claims this app took and has neither submitted nor released.
    taken: Mutex<BTreeMap<RequestId, store::HumanClaim>>,
    log: review::log::Sink,
    /// The Tool log of each review a run logged to, until it is submitted, and of each
    /// Artifact whose tool ran without a review, while the app runs.
    logs: Mutex<Logs>,
    log_settlement: tokio::sync::Notify,
    /// The Tool log's receiving end, until the app starts forwarding it.
    events: Mutex<Option<review::log::Events>>,
}

#[derive(Default)]
struct Logs {
    open: BTreeMap<review::log::Key, review::log::ToolLog>,
    /// A queued Start can still arrive after the submission's invalidation.
    settled: BTreeSet<RequestId>,
}

impl Default for Reviews {
    fn default() -> Self {
        let (log, events) = review::log::channel();
        let log = log.with_viewer_launch();
        Self {
            views: Mutex::default(),
            reads: Reads::default(),
            searches: searches::Searches::default(),
            commands: Mutex::default(),
            jobs: Jobs::default(),
            claim_gate: tokio::sync::Mutex::default(),
            taken: Mutex::default(),
            log,
            logs: Mutex::default(),
            log_settlement: tokio::sync::Notify::new(),
            events: Mutex::new(Some(events)),
        }
    }
}

impl Reviews {
    /// The Tool log's events, for the one forwarder of this app.
    pub(crate) fn take_events(&self) -> Option<review::log::Events> {
        self.events.lock().expect("events lock").take()
    }

    /// The sink every tool run of this app logs to.
    pub(crate) fn log(&self) -> &review::log::Sink {
        &self.log
    }

    /// Take `events` into each Tool log; answer the logs they changed.
    pub(crate) fn logged(&self, events: Vec<review::log::Event>) -> Vec<log::KeyedLog> {
        let mut logs = self.logs.lock().expect("logs lock");
        let mut changed = BTreeSet::new();
        for event in events {
            if matches!(&event.key, review::log::Key::Review(id) if logs.settled.contains(id)) {
                continue;
            }
            let log = logs.open.entry(event.key.clone()).or_default();
            if !log.entries().is_empty() || matches!(event.kind, review::log::Kind::Start { .. }) {
                changed.insert(event.key.clone());
            }
            log.apply(event);
        }
        changed
            .into_iter()
            .map(|key| log::keyed_log(&key, &logs.open[&key]))
            .collect()
    }

    fn clear_log(&self, id: &RequestId) -> Option<log::KeyedLog> {
        let mut logs = self.logs.lock().expect("logs lock");
        logs.settled.insert(id.clone());
        let key = review::log::Key::from(id);
        logs.open
            .remove(&key)
            .map(|_| log::keyed_log(&key, &review::log::ToolLog::default()))
    }

    pub(crate) fn review_status_changed(&self) {
        self.log_settlement.notify_one();
    }

    pub(crate) async fn wait_for_review_status(&self) {
        self.log_settlement.notified().await;
    }

    /// Read the authoritative review after a commit in any process, not just this window.
    pub(crate) async fn clear_settled_logs(&self, state: &std::path::Path) -> Vec<log::KeyedLog> {
        let ids: Vec<_> = self
            .logs
            .lock()
            .expect("logs lock")
            .open
            .keys()
            .filter_map(|key| match key {
                review::log::Key::Review(id) => Some(id.clone()),
                review::log::Key::Artifact { .. } => None,
            })
            .collect();
        let mut cleared = Vec::new();
        for id in ids {
            if store::read_request_as(state, id.as_str(), store::Shown::Requests)
                .await
                .is_ok_and(|view| {
                    matches!(
                        view.request.status(),
                        artifactize_core::types::RequestStatus::Green
                            | artifactize_core::types::RequestStatus::Red
                    )
                })
                && let Some(log) = self.clear_log(&id)
            {
                cleared.push(log);
            }
        }
        cleared
    }

    /// Forget everything read from a replaced state and cancel its jobs.
    pub fn reset(&self) {
        self.cancel_all();
        *self.logs.lock().expect("logs lock") = Logs::default();
        self.views.lock().expect("views lock").clear();
        self.commands.lock().expect("commands lock").clear();
        self.taken.lock().expect("taken lock").clear();
    }

    pub(crate) fn invalidate_searches(&self) {
        self.searches.invalidate();
    }

    pub(crate) fn review_worktrees_changed(&self) -> bool {
        self.searches.rescan()
    }

    pub fn cancel_all(&self) {
        self.jobs.cancel_all();
    }

    pub(crate) fn start_artifact(&self, caller: Caller, job: JobId) -> Result<Lease, Failure> {
        self.jobs.register(caller, job, Action::Tool)
    }

    /// Freshen the cached views of requests the waiting stack read as read number `read`, so
    /// a review opened from it shows the claims and states the stack shows.
    pub(crate) fn remember(
        &self,
        generation: u32,
        read: u32,
        views: impl IntoIterator<Item = RequestView>,
    ) {
        for view in views {
            let id = view.request.id.clone();
            self.keep(
                read,
                id,
                ReadReview {
                    generation,
                    exact: view.clone(),
                    owner: view,
                },
            );
        }
    }

    fn keep(&self, read: u32, id: RequestId, view: ReadReview) {
        let mut cached = self.views.lock().expect("views lock");
        if cached.get(&id).is_none_or(|(newest, _)| *newest < read) {
            cached.insert(id, (read, view));
        }
    }

    fn took(&self, claim: store::HumanClaim) {
        self.taken
            .lock()
            .expect("taken lock")
            .insert(claim.request_id.clone(), claim);
    }

    fn forget_claim(&self, id: &RequestId) {
        self.taken.lock().expect("taken lock").remove(id);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Stage {
    /// Waiting and not claimed by this reviewer: read-only until claimed.
    Claim,
    /// Claimed by this reviewer.
    Review,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ToolKind {
    /// Starts a program and returns at once.
    Launch,
    /// Waits and captures the output.
    Output,
    /// Opens the Artifact Viewer.
    Viewer,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ToolView {
    pub name: String,
    /// The Artifact that declares it, and its name there: `open` of `notes` is `open_notes`.
    pub artifact: String,
    pub operation: String,
    pub kind: ToolKind,
    pub description: String,
    /// What running it does: the resolved command line, or a builtin's action.
    pub action: String,
    /// Why the command could not be resolved; it is shown as declared.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Claimant {
    pub reviewer: String,
    pub mine: bool,
}

/// One request in the shared Human Detail.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HumanView {
    pub id: RequestKey,
    /// Draft and Tool log events use this original request ID, including for followers.
    pub owner: RequestKey,
    pub target: search::ReviewTarget,
    pub notes: Option<String>,
    pub title: String,
    /// The Artifact the reviewed eval belongs to: its tools come first.
    pub artifact: String,
    /// Status, claim and waiting time in one line.
    pub meta: String,
    /// For a follower: which request and repository its actions go to.
    pub shared: Option<String>,
    pub instruction: String,
    /// Identifiers, the repository, the claim and the raw owner schemas.
    pub technical: Vec<Field>,
    pub mark: Mark,
    pub stage: Stage,
    pub claim: Option<Claimant>,
    pub reviewer: String,
    pub tools: Vec<ToolView>,
    /// A Human definition is recorded; without one, tools and schemas are unknown.
    pub recorded: bool,
    /// A completed request's status and result, or its error.
    pub result: Option<String>,
    /// A completed request's verdict and fields by name, the form's constants left out.
    pub completed: Option<Vec<ResultEntry>>,
    /// The state generation it was read from; a newer one means it no longer applies.
    pub generation: u32,
    /// The number of the read it shows: a view of a lower number was read before it, and
    /// never replaces it.
    pub read: u32,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    pub text: String,
    pub error: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub viewer: Option<crate::viewer::ViewerLaunch>,
}

impl From<(String, bool)> for Notice {
    fn from((text, error): (String, bool)) -> Self {
        Self {
            text,
            error,
            viewer: None,
        }
    }
}

/// What a Human action left: its notice and the request as it now stands.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Acted {
    pub notice: Notice,
    pub view: Option<HumanView>,
}

pub(crate) fn kind(kind: HumanToolKind) -> ToolKind {
    match kind {
        HumanToolKind::Launch => ToolKind::Launch,
        HumanToolKind::Output => ToolKind::Output,
        HumanToolKind::Viewer => ToolKind::Viewer,
    }
}

/// A settled result as its reviewer entered it: the verdict, then each field the form asked
/// for, without the constants the form filled in.
fn completed(request: &store::Request) -> Option<Vec<ResultEntry>> {
    let result = request.result()?;
    let verdict = result.verdict();
    let constants: BTreeSet<String> = review::form(request, verdict)
        .map(|form| {
            form.fields
                .into_iter()
                .filter(|field| matches!(field.input, review::Input::Fixed(_)))
                .map(|field| field.name)
                .collect()
        })
        .unwrap_or_default();
    let verdict = serde_json::json!(verdict);
    let fields = result.owner_fields();
    Some(crate::detail::result_entries(
        std::iter::once((&"verdict".to_owned(), &verdict))
            .chain(fields.iter().filter(|(key, _)| !constants.contains(*key)))
            .collect::<Vec<_>>(),
    ))
}

fn view(
    view: &RequestView,
    reviewer: &str,
    commands: Option<&Resolved>,
    generation: u32,
    read: u32,
    now: OffsetDateTime,
) -> HumanView {
    let request = &view.request;
    let settled = !request.status().is_human_wait();
    let claim = view.claim.as_ref().map(|claim| Claimant {
        reviewer: claim.reviewer.to_string(),
        mine: claim.reviewer.as_str() == reviewer,
    });
    HumanView {
        id: request.id.clone().into(),
        owner: request.id.clone().into(),
        target: review::Target::from(request.as_ref()).into(),
        notes: request.review_notes.clone(),
        title: format!("{} · {}", request.eval_id, request.title),
        artifact: request.target.to_string(),
        meta: review::meta(view, reviewer, now),
        shared: review::shared(view),
        instruction: request.payload.instruction().to_owned(),
        technical: review::technical(view, reviewer)
            .into_iter()
            .map(|(key, value)| Field {
                key: key.into(),
                value,
            })
            .collect(),
        mark: monitor::Status::from(request.status()).into(),
        stage: match review::stage(view, reviewer) {
            review::Stage::Completed => Stage::Completed,
            review::Stage::Review => Stage::Review,
            review::Stage::Claim => Stage::Claim,
        },
        claim,
        reviewer: reviewer.to_owned(),
        tools: review::tools(request)
            .into_iter()
            .map(|tool| {
                // Every tool carries why its commands could not be resolved at all.
                let (resolved, failed) = match commands {
                    Some(Ok(commands)) => (commands.get(&tool.name), None),
                    Some(Err(error)) => (None, Some(error.clone())),
                    None => (None, None),
                };
                ToolView {
                    action: tool.action(resolved),
                    error: failed
                        .or_else(|| resolved.and_then(|resolved| resolved.as_ref().err().cloned())),
                    operation: tool.operation().to_owned(),
                    artifact: tool.artifact.to_string(),
                    name: tool.name.to_string(),
                    kind: kind(tool.kind),
                    description: tool.description,
                }
            })
            .collect(),
        recorded: request.human_definition.is_some(),
        result: settled.then(|| format!("{}\n{}", request.status(), review::outcome_text(request))),
        completed: settled.then(|| completed(request)).flatten(),
        generation,
        read,
    }
}

/// Read the exact request and its actionable owner under the same read sequence number.
async fn read(app: &App, id: &RequestId) -> Result<(u32, ReadReview), Failure> {
    read_review(app, id, false).await
}

async fn read_review(
    app: &App,
    id: &RequestId,
    refresh: bool,
) -> Result<(u32, ReadReview), Failure> {
    let state = &app.launch.state;
    let generation = app.generation.current();
    let (number, views) = app
        .human
        .reads
        .numbered(async {
            let exact = store::read_request_as(state, id, store::Shown::Requests).await?;
            resolve(app, generation, exact).await
        })
        .await;
    Ok((
        number,
        loaded(app, number, views.map_err(Failure::Read)?, refresh).await?,
    ))
}

async fn resolve(app: &App, generation: u32, exact: RequestView) -> Result<ReadReview, String> {
    let owner = store::read_review_owner(&app.launch.state, &exact).await?;
    Ok(ReadReview {
        generation,
        exact,
        owner,
    })
}

async fn loaded(
    app: &App,
    number: u32,
    read: ReadReview,
    refresh: bool,
) -> Result<ReadReview, Failure> {
    let view = &read.owner;
    // Resolve what each tool runs once per request, so the Tools pane shows what runs.
    let resolve = view.request.status().is_human_wait()
        && view.request.human_definition.is_some()
        && (refresh
            || !app
                .human
                .commands
                .lock()
                .expect("commands lock")
                .contains_key(&view.request.id));
    let commands = if resolve {
        Some(artifactize_core::human::preview_tool_commands(view.request.clone()).await)
    } else {
        None
    };
    app.generation
        .if_current(read.generation, || {
            if let Some(commands) = commands {
                app.human
                    .commands
                    .lock()
                    .expect("commands lock")
                    .insert(view.request.id.clone(), commands);
            }
            app.human
                .keep(number, read.exact.request.id.clone(), read.clone());
        })
        .ok_or_else(|| {
            Failure::Read("The state was replaced while reading the review; reopen it.".into())
        })?;
    Ok(read)
}

/// The review of `request`, read as read number `read`.
fn show(app: &App, read: u32, request: &ReadReview) -> HumanView {
    let commands = app
        .generation
        .if_current(request.generation, || {
            app.human
                .commands
                .lock()
                .expect("commands lock")
                .get(&request.owner.request.id)
                .cloned()
        })
        .flatten();
    let mut shown = view(
        &request.owner,
        app.launch.reviewer.as_str(),
        commands.as_ref(),
        request.generation,
        read,
        OffsetDateTime::now_utc(),
    );
    shown.id = request.exact.request.id.clone().into();
    shown.technical = review::technical(&request.exact, app.launch.reviewer.as_str())
        .into_iter()
        .map(|(key, value)| Field {
            key: key.into(),
            value,
        })
        .collect();
    shown.target = review::Target::from(request.exact.request.as_ref()).into();
    shown.shared = review::shared(&request.exact);
    shown
}

/// Open request `id` in the shared Human Detail. `reload: false` redraws the last read view
/// (its waiting time) without reading the state.
#[tauri::command]
#[specta::specta]
pub async fn human_open(
    app: tauri::State<'_, App>,
    id: RequestKey,
    reload: bool,
) -> Result<HumanView, Failure> {
    let id = id.into_inner();
    let cached = (!reload)
        .then(|| {
            app.human
                .views
                .lock()
                .expect("views lock")
                .get(&id)
                .cloned()
        })
        .flatten();
    let (number, request) = match cached {
        Some(cached) => cached,
        None => read_review(&app, &id, reload).await?,
    };
    Ok(show(&app, number, &request))
}

/// The form for `verdict`, from the owner schema recorded with request `id`, holding the values
/// of `kept` (the earlier draft of this verdict) when the schema still takes them.
#[tauri::command]
#[specta::specta]
pub async fn human_form(
    app: tauri::State<'_, App>,
    id: RequestKey,
    verdict: Verdict,
    kept: Option<Draft>,
) -> Result<FormView, Failure> {
    let id = id.into_inner();
    guard(&app, &id, review::review_refusal).await?;
    form::template(&opened(&app, &id)?.request, verdict, kept)
}

async fn run_job(app: &App, work: Job, cancel: CancellationToken) -> Outcome {
    work.run(
        app.launch.state.clone(),
        app.launch.reviewer.clone(),
        cancel,
        app.human.log.clone(),
    )
    .await
}

/// Claim request `id` for this reviewer; nothing else claims. A claim of this window is offered
/// for release on quit, whichever screen took it.
#[tauri::command]
#[specta::specta]
pub async fn human_claim(
    app: tauri::State<'_, App>,
    caller: Caller,
    id: RequestKey,
    job: JobId,
) -> Result<Acted, Failure> {
    let id = id.into_inner();
    let mut work = app.human.jobs.register(caller, job, Action::Ordinary)?;
    // Someone else's claim, or a settled request, is refused in the library's words.
    let (_, views) = read(&app, &id).await?;
    let view = views.owner;
    review::actionable(Some(&view), &app.launch.reviewer).map_err(Failure::Action)?;
    let owner = view.request.id.clone();
    work.admit(vec![owner.clone()])?;
    let outcome = {
        let _settling = tokio::select! {
            biased;
            () = work.cancel.cancelled() => return Err(Failure::Action("Review action cancelled.".into())),
            gate = app.human.claim_gate.lock() => gate,
        };
        if app.windows.is_closing() {
            return Err(Failure::Action("This window is closing.".into()));
        }
        work.ready().await?;
        let outcome = run_job(&app, Job::Claim { id: owner }, work.cancel.clone()).await;
        if let Outcome::Claimed {
            result: Ok((claim, true)),
        } = &outcome
        {
            app.human.took(claim.clone());
        }
        outcome
    };
    acted(&app, &id, outcome).await
}

async fn acted(app: &App, id: &RequestId, outcome: Outcome) -> Result<Acted, Failure> {
    let notice = outcome.notice().into();
    Ok(Acted {
        notice,
        view: read(app, id)
            .await
            .ok()
            .map(|(number, request)| show(app, number, &request)),
    })
}

/// Run tool `tool` of request `id`; job number `job` cancels it. The run needs this
/// reviewer's claim on a request that still waits and unchanged declarations. What it prints
/// arrives as [`log::HumanToolLog`] events while it runs; a launch tool answers at once and its
/// program's output keeps arriving until it exits.
#[tauri::command]
#[specta::specta]
pub async fn human_run(
    app: tauri::State<'_, App>,
    caller: Caller,
    id: RequestKey,
    tool: String,
    job: JobId,
) -> Result<Notice, Failure> {
    let id = id.into_inner();
    let tool: ToolName = tool.parse().map_err(Failure::Missing)?;
    let mut work = app.human.jobs.register(caller, job, Action::Tool)?;
    let request = guard(&app, &id, review::review_refusal).await?;
    let owner = request.request.id.clone();
    work.admit(vec![owner.clone()])?;
    work.ready().await?;
    let mut outcome = run_job(
        &app,
        Job::Run {
            id: owner.clone(),
            tool,
            claim: false,
        },
        work.cancel.clone(),
    )
    .await;
    // Declarations may have changed; resolve the commands again for the next run.
    app.human
        .commands
        .lock()
        .expect("commands lock")
        .remove(&owner);
    let mut notice: Notice = outcome.notice().into();
    notice.viewer = crate::viewer::launch(&app, &mut outcome, Some(owner), work);
    Ok(notice)
}

/// Cancel running job `job`: a tool, or a submission's fingerprint recheck.
#[tauri::command]
#[specta::specta]
pub fn human_cancel(app: tauri::State<'_, App>, caller: Caller, job: JobId) {
    app.viewers.cancel_job(&caller, job);
    app.human.jobs.cancel(caller, job);
}

/// Where a submission's settled result stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Settled {
    /// Settled locally and, with a review store configured, published to it.
    Published,
    /// Settled locally, but publishing it failed: the result stands and is never submitted
    /// again; the notice says what failed.
    SavedLocally,
}

/// A settled submission: how it stands, its notice and the completed request.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Submitted {
    pub settled: Settled,
    pub notice: Notice,
    pub view: Option<HumanView>,
}

/// The request recorded for request `id` when it was opened.
/// A rule of the library's (`review::review_refusal`, `release_refusal`) applied to the request
/// as it stands now: its refusal is the failure, so every interface refuses in the same words.
async fn guard(
    app: &App,
    id: &RequestId,
    rule: fn(Option<&RequestView>, &artifactize_core::types::ReviewerId) -> Option<String>,
) -> Result<RequestView, Failure> {
    let (_, views) = read(app, id).await?;
    let view = views.owner;
    match rule(Some(&view), &app.launch.reviewer) {
        Some(refusal) => Err(Failure::Action(refusal)),
        None => Ok(view),
    }
}

fn opened(app: &App, id: &RequestId) -> Result<RequestView, Failure> {
    app.human
        .views
        .lock()
        .expect("views lock")
        .get(id)
        .map(|(_, view)| view.owner.clone())
        .ok_or_else(|| Failure::Missing("Open the request first.".into()))
}

/// `draft` of request `id` with field `name` set to `value`, as the library's form takes it.
#[tauri::command]
#[specta::specta]
pub async fn human_edit_field(
    app: tauri::State<'_, App>,
    id: RequestKey,
    draft: Draft,
    name: String,
    value: form::FieldValue,
) -> Result<Draft, Failure> {
    let id = id.into_inner();
    form::edit_field(&opened(&app, &id)?.request, draft, &name, value)
}

/// `draft` of request `id` with an empty item added to its list field `name`, as the library's
/// form adds it.
#[tauri::command]
#[specta::specta]
pub async fn human_add_item(
    app: tauri::State<'_, App>,
    id: RequestKey,
    draft: Draft,
    name: String,
) -> Result<Draft, Failure> {
    let id = id.into_inner();
    form::add_item(&opened(&app, &id)?.request, draft, &name)
}

/// `draft` of request `id` with item `item` removed from its list field `name`; the library's
/// form keeps one empty item.
#[tauri::command]
#[specta::specta]
pub async fn human_remove_item(
    app: tauri::State<'_, App>,
    id: RequestKey,
    draft: Draft,
    name: String,
    item: u32,
) -> Result<Draft, Failure> {
    let id = id.into_inner();
    form::remove_item(&opened(&app, &id)?.request, draft, &name, item)
}

/// Submit `draft` for request `id` and publish it like `request submit`. A result the
/// schema rejects keeps the request waiting and fails as `invalid`.
#[tauri::command]
#[specta::specta]
pub async fn human_submit(
    app: tauri::State<'_, App>,
    caller: Caller,
    id: RequestKey,
    draft: Draft,
    job: JobId,
) -> Result<Submitted, Failure> {
    let id = id.into_inner();
    let work = app.human.jobs.register(caller, job, Action::Finish)?;
    let request = guard(&app, &id, review::review_refusal).await?;
    let result = form::result(&request.request, draft)?
        .try_into()
        .map_err(Failure::Invalid)?;
    submit(
        &app,
        work,
        id,
        request.request.id.clone(),
        review::Answer::Explicit(result),
    )
    .await
}

async fn submit(
    app: &App,
    mut work: Lease,
    id: RequestId,
    owner: RequestId,
    answer: review::Answer,
) -> Result<Submitted, Failure> {
    work.admit(vec![owner.clone()])?;
    app.viewers.cancel_review(&owner);
    work.ready().await?;
    let outcome = run_job(
        app,
        Job::Submit {
            id: owner.clone(),
            answer,
            claim: false,
        },
        work.cancel.clone(),
    )
    .await;
    let settled = match &outcome {
        Outcome::Submitted {
            result: Err(error), ..
        } => return Err(Failure::Invalid(error.clone())),
        Outcome::SavedLocally { .. } => Settled::SavedLocally,
        Outcome::Submitted { result: Ok(_), .. } => Settled::Published,
        other => unreachable!("a submit job ends in Submitted or SavedLocally, not {other:?}"),
    };
    app.human.forget_claim(&owner);
    crate::viewer::settled(app, &owner);
    if id != owner {
        crate::viewer::settled(app, &id);
    }
    // A submitted review's log ends here; what a launched program still prints is dropped.
    app.human.review_status_changed();
    let Acted { notice, view } = acted(app, &id, outcome).await?;
    Ok(Submitted {
        settled,
        notice,
        view,
    })
}

/// Release this reviewer's claims on `ids` without a verdict.
#[tauri::command]
#[specta::specta]
pub async fn human_release(
    app: tauri::State<'_, App>,
    caller: Caller,
    ids: Vec<RequestKey>,
    job: JobId,
) -> Result<Acted, Failure> {
    let ids: Vec<RequestId> = ids.into_iter().map(RequestKey::into_inner).collect();
    let mut work = app.human.jobs.register(caller, job, Action::Ordinary)?;
    let mut owners = BTreeSet::new();
    for id in &ids {
        let view = if ids.len() == 1 {
            guard(&app, id, review::release_refusal).await?
        } else {
            read(&app, id).await?.1.owner
        };
        owners.insert(view.request.id.clone());
    }
    let owners: Vec<_> = owners.into_iter().collect();
    work.admit(owners.clone())?;
    for owner in &owners {
        app.viewers.cancel_review(owner);
    }
    let outcome = run_job(
        &app,
        Job::Release {
            ids: owners.clone(),
            quit: false,
        },
        work.cancel.clone(),
    )
    .await;
    let Outcome::Released { failed, .. } = &outcome else {
        unreachable!("a release job ends in Released");
    };
    // A release that failed keeps its claim on the list.
    for id in owners
        .iter()
        .filter(|id| !failed.iter().any(|(failed, _)| failed == *id))
    {
        app.human.forget_claim(id);
        crate::viewer::settled(&app, id);
    }
    let notice = outcome.notice().into();
    let view = match ids.as_slice() {
        [id] => read(&app, id)
            .await
            .ok()
            .map(|(number, request)| show(&app, number, &request)),
        _ => None,
    };
    Ok(Acted { notice, view })
}

/// Claims this window took and has neither submitted nor released, for the quit prompt.
#[tauri::command]
#[specta::specta]
pub async fn human_taken(app: tauri::State<'_, App>) -> Result<Vec<RequestKey>, Failure> {
    let _settling = app.human.claim_gate.lock().await;
    live_claims(&app).await
}

/// The caller holds `claim_gate`, so a pending acquisition cannot change this snapshot.
pub(crate) async fn live_claims(app: &App) -> Result<Vec<RequestKey>, Failure> {
    let claims = app.human.taken.lock().expect("taken lock").clone();
    let mut live = Vec::new();
    for (id, claim) in claims {
        if store::read_request_as(&app.launch.state, &id, store::Shown::Requests)
            .await
            .map_err(Failure::Read)?
            .claim
            .as_ref()
            == Some(&claim)
        {
            live.push(id.into());
        }
    }
    Ok(live)
}

/// Release and quit only releases claims this app took and still owns.
#[tauri::command]
#[specta::specta]
pub async fn human_release_taken(
    app: tauri::State<'_, App>,
    caller: Caller,
    ids: Vec<RequestKey>,
    job: JobId,
) -> Result<Acted, Failure> {
    let claims: Vec<_> = {
        let taken = app.human.taken.lock().expect("taken lock");
        ids.into_iter()
            .filter_map(|id| taken.get(&id.into_inner()).cloned())
            .collect()
    };
    let ids: Vec<_> = claims
        .iter()
        .map(|claim| claim.request_id.clone())
        .collect();
    let work = app
        .human
        .jobs
        .begin(caller, job, ids.clone(), Action::Finish)?;
    for id in &ids {
        app.viewers.cancel_review(id);
    }
    work.ready().await?;
    let mut failed = Vec::new();
    for claim in claims {
        let id = claim.request_id.clone();
        match artifactize_core::human::release_exact_claim(&app.launch.state, claim).await {
            Ok(()) => {
                app.human.forget_claim(&id);
                crate::viewer::settled(&app, &id);
            }
            Err(error) => failed.push((id, error)),
        }
    }
    drop(work);
    let outcome = Outcome::Released {
        ids,
        quit: true,
        failed,
    };
    Ok(Acted {
        notice: outcome.notice().into(),
        view: None,
    })
}
