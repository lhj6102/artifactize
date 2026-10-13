//! The shared Human Detail's backend: the open request as the terminal review shows it, and
//! claim, tool, submit and release through the same [`Job`]s as `artifactize review`.
//!
//! Only these jobs write. Drafts live in the frontend; a submission sends their values and
//! the review's own [`Form`] turns them into the result, so typed fields, coercion and local
//! errors are the terminal's.
mod form;

pub use form::{Draft, FormView, Verdict};

/// The state's origin right now, noted as the generation every Human view belongs to.
pub(crate) async fn current(app: &App) -> Result<u32, Failure> {
    let state = app.launch.state.clone();
    let origin = crate::failure::joined(
        tokio::task::spawn_blocking(move || monitor::Origin::read(&state)).await,
    )?
    .map_err(Failure::Read)?;
    Ok(app.observe(origin))
}

use crate::ids::{JobId, RequestKey};
use crate::{App, Failure, detail::Field, status::Mark};
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

/// Per-app review state: what is cached for drawing, running jobs, and claims taken here.
#[derive(Default)]
pub struct Reviews {
    /// The last read view of each opened request, for redraws without a read.
    views: Mutex<BTreeMap<RequestId, RequestView>>,
    /// Resolved tool command lines, resolved once per request and again after each run.
    commands: Mutex<BTreeMap<RequestId, Resolved>>,
    /// Running jobs by the frontend's job number.
    jobs: Mutex<BTreeMap<JobId, CancellationToken>>,
    /// Claims this app took and has neither submitted nor released.
    taken: Mutex<BTreeSet<RequestId>>,
}

impl Reviews {
    /// Forget everything read from a replaced state and cancel its jobs.
    pub fn reset(&self) {
        self.cancel_all();
        self.views.lock().expect("views lock").clear();
        self.commands.lock().expect("commands lock").clear();
        self.taken.lock().expect("taken lock").clear();
    }

    pub fn cancel_all(&self) {
        for token in self.jobs.lock().expect("jobs lock").values() {
            token.cancel();
        }
    }

    fn start(&self, job: JobId) -> CancellationToken {
        let token = CancellationToken::new();
        self.jobs
            .lock()
            .expect("jobs lock")
            .insert(job, token.clone());
        token
    }

    fn end(&self, job: JobId) {
        self.jobs.lock().expect("jobs lock").remove(&job);
    }

    /// Freshen the cached views of requests a search just read, so their Preview shows the
    /// claims and states the list shows.
    pub(crate) fn remember(&self, views: impl IntoIterator<Item = RequestView>) {
        let mut cached = self.views.lock().expect("views lock");
        for view in views {
            cached.insert(view.request.id.clone(), view);
        }
    }

    fn took(&self, id: &RequestId, taken: bool) {
        let mut set = self.taken.lock().expect("taken lock");
        if taken {
            set.insert(id.clone());
        } else {
            set.remove(id);
        }
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
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ToolView {
    pub name: String,
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
    /// The request every action goes to: a waiting follower's original.
    pub id: RequestKey,
    pub title: String,
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
    /// The state generation it was read from; a newer one means it no longer applies.
    pub generation: u32,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    pub text: String,
    pub error: bool,
}

impl From<(String, bool)> for Notice {
    fn from((text, error): (String, bool)) -> Self {
        Self { text, error }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Output {
    pub title: String,
    pub text: String,
    pub error: bool,
}

/// A finished tool run: its output pane and notice.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ToolRun {
    pub output: Output,
    pub notice: Notice,
}

/// What a Human action left: its notice and the request as it now stands.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Acted {
    pub notice: Notice,
    pub view: Option<HumanView>,
}

fn kind(kind: HumanToolKind) -> ToolKind {
    match kind {
        HumanToolKind::Launch => ToolKind::Launch,
        HumanToolKind::Output => ToolKind::Output,
    }
}

fn view(
    view: &RequestView,
    reviewer: &str,
    commands: Option<&Resolved>,
    generation: u32,
    now: OffsetDateTime,
) -> HumanView {
    let request = &view.request;
    let settled = !request.status().is_human_wait();
    let claim = view.claim.as_ref().map(|claim| Claimant {
        reviewer: claim.reviewer.to_string(),
        mine: claim.reviewer.as_str() == reviewer,
    });
    let mine = claim.as_ref().is_some_and(|claim| claim.mine);
    HumanView {
        id: request.id.clone().into(),
        title: format!("{} · {}", request.eval_id, request.title),
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
        stage: match review::stage(settled, mine) {
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
                    name: tool.name.to_string(),
                    kind: kind(tool.kind),
                    description: tool.description,
                }
            })
            .collect(),
        recorded: request.human_definition.is_some(),
        result: settled.then(|| format!("{}\n{}", request.status(), review::outcome_text(request))),
        generation,
    }
}

/// Read `id` afresh: a waiting follower resolves to its original, which records the Human
/// definition and receives every action.
async fn read(app: &App, id: &RequestId) -> Result<RequestView, Failure> {
    let state = &app.launch.state;
    let view = store::read_request(state, id)
        .await
        .map_err(Failure::Read)?;
    let view = if view.request.status().is_human_wait() {
        store::read_original(state, &view)
            .await
            .map_err(Failure::Read)?
    } else {
        view
    };
    // Resolve what each tool runs once per request, so the Tools pane shows what runs.
    let resolve = view.request.status().is_human_wait()
        && view.request.human_definition.is_some()
        && !app
            .human
            .commands
            .lock()
            .expect("commands lock")
            .contains_key(&view.request.id);
    if resolve {
        let commands = async {
            let (receipts, _) = artifactize_core::human::open(state, &view.request.id).await?;
            artifactize_core::human::tool_commands(&receipts, &view.request.id).await
        }
        .await;
        app.human
            .commands
            .lock()
            .expect("commands lock")
            .insert(view.request.id.clone(), commands);
    }
    let mut views = app.human.views.lock().expect("views lock");
    views.insert(id.clone(), view.clone());
    views.insert(view.request.id.clone(), view.clone());
    Ok(view)
}

fn show(app: &App, request: &RequestView) -> HumanView {
    let commands = app.human.commands.lock().expect("commands lock");
    view(
        request,
        app.launch.reviewer.as_str(),
        commands.get(&request.request.id),
        app.generation.current(),
        OffsetDateTime::now_utc(),
    )
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
    let request = match cached {
        Some(request) => request,
        None => read(&app, &id).await?,
    };
    Ok(show(&app, &request))
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

async fn run_job(app: &App, job: JobId, work: Job) -> Outcome {
    let cancel = app.human.start(job);
    let outcome = work
        .run(
            app.launch.state.clone(),
            app.launch.reviewer.clone(),
            cancel,
        )
        .await;
    app.human.end(job);
    outcome
}

/// Claim request `id` for this reviewer; nothing else claims. A claim of this window is offered
/// for release on quit, whichever screen took it.
#[tauri::command]
#[specta::specta]
pub async fn human_claim(
    app: tauri::State<'_, App>,
    id: RequestKey,
    job: JobId,
) -> Result<Acted, Failure> {
    let id = id.into_inner();
    // Someone else's claim, or a settled request, is refused in the library's words.
    let view = read(&app, &id).await?;
    review::actionable(Some(&view), &app.launch.reviewer).map_err(Failure::Action)?;
    let outcome = run_job(&app, job, Job::Claim { id: id.clone() }).await;
    if let Outcome::Claimed { result: Ok(_) } = &outcome {
        app.human.took(&id, true);
    }
    acted(&app, &id, outcome).await
}

async fn acted(app: &App, id: &RequestId, outcome: Outcome) -> Result<Acted, Failure> {
    let notice = outcome.notice().into();
    Ok(Acted {
        notice,
        view: read(app, id).await.ok().map(|request| show(app, &request)),
    })
}

/// Run tool `tool` of request `id`; job number `job` cancels it. The run needs this
/// reviewer's claim on a request that still waits and unchanged declarations.
#[tauri::command]
#[specta::specta]
pub async fn human_run(
    app: tauri::State<'_, App>,
    id: RequestKey,
    tool: String,
    job: JobId,
) -> Result<ToolRun, Failure> {
    let id = id.into_inner();
    let tool: ToolName = tool.parse().map_err(Failure::Missing)?;
    guard(&app, &id, review::review_refusal).await?;
    let outcome = run_job(
        &app,
        job,
        Job::Run {
            id: id.clone(),
            tool,
            claim: false,
        },
    )
    .await;
    // Declarations may have changed; resolve the commands again for the next run.
    app.human
        .commands
        .lock()
        .expect("commands lock")
        .remove(&id);
    let notice = outcome.notice().into();
    let Outcome::Ran { tool, result, .. } = outcome else {
        unreachable!("a run job ends in Ran");
    };
    let output = review::output(&tool, result);
    Ok(ToolRun {
        output: Output {
            title: output.title,
            text: output.text,
            error: output.error,
        },
        notice,
    })
}

/// Cancel running job `job`: a tool, or a submission's fingerprint recheck.
#[tauri::command]
#[specta::specta]
pub fn human_cancel(app: tauri::State<'_, App>, job: JobId) {
    if let Some(token) = app.human.jobs.lock().expect("jobs lock").get(&job) {
        token.cancel();
    }
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
) -> Result<(), Failure> {
    let view = read(app, id).await?;
    match rule(Some(&view), &app.launch.reviewer) {
        Some(refusal) => Err(Failure::Action(refusal)),
        None => Ok(()),
    }
}

fn opened(app: &App, id: &RequestId) -> Result<RequestView, Failure> {
    app.human
        .views
        .lock()
        .expect("views lock")
        .get(id)
        .cloned()
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

/// `draft` of request `id` with its owner-fields JSON replaced.
#[tauri::command]
#[specta::specta]
pub async fn human_edit_json(
    app: tauri::State<'_, App>,
    id: RequestKey,
    draft: Draft,
    json: String,
) -> Result<Draft, Failure> {
    let id = id.into_inner();
    form::edit_json(&opened(&app, &id)?.request, draft, json)
}

/// Submit `draft` for request `id` and publish it like `request submit`. A result the
/// schema rejects keeps the request waiting and fails as `invalid`.
#[tauri::command]
#[specta::specta]
pub async fn human_submit(
    app: tauri::State<'_, App>,
    id: RequestKey,
    draft: Draft,
    job: JobId,
) -> Result<Submitted, Failure> {
    let id = id.into_inner();
    guard(&app, &id, review::review_refusal).await?;
    let request = opened(&app, &id)?;
    let verdict = draft.verdict.into();
    let result = form::result(&request.request, draft)?;
    let outcome = run_job(
        &app,
        job,
        Job::Submit {
            id: id.clone(),
            verdict,
            result,
            claim: false,
        },
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
    app.human.took(&id, false);
    let Acted { notice, view } = acted(&app, &id, outcome).await?;
    Ok(Submitted {
        settled,
        notice,
        view,
    })
}

/// The owner-fields JSON of `draft`, to edit a typed form as JSON: what the terminal review
/// hands to `$EDITOR`.
#[tauri::command]
#[specta::specta]
pub fn human_json(
    app: tauri::State<'_, App>,
    id: RequestKey,
    draft: Draft,
) -> Result<String, Failure> {
    let request = opened(&app, &id.into_inner())?;
    form::json(&request.request, draft)
}

/// Release this reviewer's claims on `ids` without a verdict.
#[tauri::command]
#[specta::specta]
pub async fn human_release(
    app: tauri::State<'_, App>,
    ids: Vec<RequestKey>,
    job: JobId,
) -> Result<Acted, Failure> {
    let ids: Vec<RequestId> = ids.into_iter().map(RequestKey::into_inner).collect();
    if let [id] = ids.as_slice() {
        guard(&app, id, review::release_refusal).await?;
    }
    let outcome = run_job(
        &app,
        job,
        Job::Release {
            ids: ids.clone(),
            quit: false,
        },
    )
    .await;
    let Outcome::Released { failed, .. } = &outcome else {
        unreachable!("a release job ends in Released");
    };
    // A release that failed keeps its claim on the list.
    for id in ids
        .iter()
        .filter(|id| !failed.iter().any(|(failed, _)| failed == *id))
    {
        app.human.took(id, false);
    }
    let notice = outcome.notice().into();
    let view = match ids.as_slice() {
        [id] => read(&app, id)
            .await
            .ok()
            .map(|request| show(&app, &request)),
        _ => None,
    };
    Ok(Acted { notice, view })
}

/// Claims this window took and has neither submitted nor released, for the quit prompt.
#[tauri::command]
#[specta::specta]
pub fn human_taken(app: tauri::State<'_, App>) -> Vec<RequestKey> {
    app.human
        .taken
        .lock()
        .expect("taken lock")
        .iter()
        .cloned()
        .map(RequestKey::from)
        .collect()
}
