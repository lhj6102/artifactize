//! The shared Human Detail's backend: the open request as the terminal review shows it, and
//! claim, tool, submit and release through the same [`Job`]s as `artifactize review`.
//!
//! Only these jobs write. Drafts live in the frontend; a submission sends their values and
//! the review's own [`Form`] turns them into the result, so typed fields, coercion and local
//! errors are the terminal's.
mod form;

pub use form::{Draft, FormView, Verdict};

/// The state's identity right now, noted as the generation every Human view belongs to.
async fn current(app: &App) -> Result<u32, Failure> {
    let state = app.launch.state.clone();
    let identity = tokio::task::spawn_blocking(move || store::read_state_id(&state))
        .await
        .map_err(|error| Failure::Read(error.to_string()))?
        .map_err(Failure::Read)?;
    Ok(app.observe(identity))
}

use crate::{App, Failure, detail::Field, status::Mark};
use artifactize::{
    config::{HumanToolKind, ToolName},
    monitor,
    review::{self, Job, Outcome},
    store::{self, RequestView},
    tools::human::CommandLine,
    types::{RequestId, RequestStatus},
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

/// Per-app review state: what is cached for drawing, running jobs, and claims taken here.
#[derive(Default)]
pub struct Reviews {
    /// The last read view of each opened request, for redraws without a read.
    views: Mutex<BTreeMap<RequestId, RequestView>>,
    /// Resolved tool command lines, resolved once per request and again after each run.
    commands: Mutex<BTreeMap<RequestId, Commands>>,
    /// Running jobs by the frontend's job number.
    jobs: Mutex<BTreeMap<u32, CancellationToken>>,
    /// Claims this app took and has neither submitted nor released.
    taken: Mutex<BTreeSet<RequestId>>,
    /// The waiting list last read, and whether it covers every repository.
    waiting: Mutex<Option<(bool, Vec<RequestView>)>>,
}

impl Reviews {
    /// Forget everything read from a replaced state and cancel its jobs.
    pub fn reset(&self) {
        self.cancel_all();
        self.views.lock().expect("views lock").clear();
        self.commands.lock().expect("commands lock").clear();
        self.taken.lock().expect("taken lock").clear();
        *self.waiting.lock().expect("waiting lock") = None;
    }

    pub fn cancel_all(&self) {
        for token in self.jobs.lock().expect("jobs lock").values() {
            token.cancel();
        }
    }

    fn start(&self, job: u32) -> CancellationToken {
        let token = CancellationToken::new();
        self.jobs
            .lock()
            .expect("jobs lock")
            .insert(job, token.clone());
        token
    }

    fn end(&self, job: u32) {
        self.jobs.lock().expect("jobs lock").remove(&job);
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
    pub id: String,
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

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WaitingRow {
    /// The original request every action goes to.
    pub id: String,
    pub eval: String,
    pub title: String,
    /// `-`, the claimant, or `name (you)`.
    pub claim: String,
    pub mine: bool,
    pub waiting: String,
    pub repository: String,
}

fn parse(id: &str) -> Result<RequestId, Failure> {
    id.parse().map_err(Failure::Missing)
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
    commands: Option<&Commands>,
    generation: u32,
) -> HumanView {
    let request = &view.request;
    let settled = request.status() != RequestStatus::WaitingHuman;
    let claim = view.claim.as_ref().map(|claim| Claimant {
        reviewer: claim.reviewer.to_string(),
        mine: claim.reviewer.as_str() == reviewer,
    });
    let mine = claim.as_ref().is_some_and(|claim| claim.mine);
    HumanView {
        id: request.id.to_string(),
        title: format!("{} · {}", request.eval_id, request.title),
        meta: review::meta(view, reviewer, OffsetDateTime::now_utc()),
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
        stage: if settled {
            Stage::Completed
        } else if mine {
            Stage::Review
        } else {
            Stage::Claim
        },
        claim,
        reviewer: reviewer.to_owned(),
        tools: review::tools(request)
            .into_iter()
            .map(|tool| {
                let resolved = commands.and_then(|commands| commands.get(&tool.name));
                ToolView {
                    action: tool.action(resolved),
                    error: resolved.and_then(|resolved| resolved.as_ref().err().cloned()),
                    name: tool.name.to_string(),
                    kind: kind(tool.kind),
                    description: tool.description,
                }
            })
            .collect(),
        recorded: request.human_definition.is_some(),
        result: settled.then(|| {
            let result = request.result().map_or_else(
                || request.error().map(str::to_owned).unwrap_or_default(),
                |result| serde_json::to_string_pretty(result).unwrap_or_default(),
            );
            format!("{}\n{result}", request.status())
        }),
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
    let view = if view.request.status() == RequestStatus::WaitingHuman {
        store::read_original(state, &view)
            .await
            .map_err(Failure::Read)?
    } else {
        view
    };
    // Resolve what each tool runs once per request, so the Tools pane shows what runs.
    let resolve = view.request.status() == RequestStatus::WaitingHuman
        && view.request.human_definition.is_some()
        && !app
            .human
            .commands
            .lock()
            .expect("commands lock")
            .contains_key(&view.request.id);
    if resolve {
        let commands = async {
            let (receipts, _) = artifactize::human::open(state, &view.request.id).await?;
            artifactize::human::tool_commands(&receipts, &view.request.id).await
        }
        .await;
        app.human
            .commands
            .lock()
            .expect("commands lock")
            .insert(view.request.id.clone(), commands.unwrap_or_default());
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
    )
}

/// Open request `id` in the shared Human Detail. `reload: false` redraws the last read view
/// (its waiting time) without reading the state.
#[tauri::command]
#[specta::specta]
pub async fn human_open(
    app: tauri::State<'_, App>,
    id: String,
    reload: bool,
) -> Result<HumanView, Failure> {
    let id = parse(&id)?;
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

/// A fresh form for `verdict`, from the owner schema recorded with request `id`.
#[tauri::command]
#[specta::specta]
pub async fn human_form(
    app: tauri::State<'_, App>,
    id: String,
    verdict: Verdict,
) -> Result<FormView, Failure> {
    let id = parse(&id)?;
    form::template(&opened(&app, &id)?.request, verdict)
}

async fn run_job(app: &App, job: u32, work: Job) -> Outcome {
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

/// Claim request `id` for this reviewer; nothing else claims. The standalone review offers
/// its claims for release on quit; the monitor keeps them, as in the terminal.
#[tauri::command]
#[specta::specta]
pub async fn human_claim(
    app: tauri::State<'_, App>,
    id: String,
    job: u32,
    standalone: bool,
) -> Result<Acted, Failure> {
    let id = parse(&id)?;
    let outcome = run_job(&app, job, Job::Claim { id: id.clone() }).await;
    if standalone && let Outcome::Claimed { result: Ok(_) } = &outcome {
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
    id: String,
    tool: String,
    job: u32,
) -> Result<ToolRun, Failure> {
    let id = parse(&id)?;
    let tool: ToolName = tool.parse().map_err(Failure::Missing)?;
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
pub fn human_cancel(app: tauri::State<'_, App>, job: u32) {
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
fn opened(app: &App, id: &RequestId) -> Result<RequestView, Failure> {
    app.human
        .views
        .lock()
        .expect("views lock")
        .get(id)
        .cloned()
        .ok_or_else(|| Failure::Missing("Open the request first.".into()))
}

/// Submit `draft` for request `id` and publish it like `request submit`. A result the
/// schema rejects keeps the request waiting and fails as `invalid`.
#[tauri::command]
#[specta::specta]
pub async fn human_submit(
    app: tauri::State<'_, App>,
    id: String,
    draft: Draft,
    job: u32,
) -> Result<Submitted, Failure> {
    let id = parse(&id)?;
    let request = opened(&app, &id)?;
    let result = form::result(&request.request, draft)?;
    let outcome = run_job(
        &app,
        job,
        Job::Submit {
            id: id.clone(),
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
        _ => Settled::Published,
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
pub fn human_json(app: tauri::State<'_, App>, id: String, draft: Draft) -> Result<String, Failure> {
    let request = opened(&app, &parse(&id)?)?;
    form::json(&request.request, draft)
}

/// Release this reviewer's claims on `ids` without a verdict.
#[tauri::command]
#[specta::specta]
pub async fn human_release(
    app: tauri::State<'_, App>,
    ids: Vec<String>,
    job: u32,
) -> Result<Acted, Failure> {
    let ids = ids
        .iter()
        .map(|id| parse(id))
        .collect::<Result<Vec<_>, _>>()?;
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

/// The waiting list and the state generation it was read from.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Waiting {
    pub generation: u32,
    pub rows: Vec<WaitingRow>,
}

/// The waiting Human sign-offs of the launch repository, or with `all` of every one; one
/// entry per sign-off, followers collapsed onto their original. `reload: false` redraws
/// the last list.
#[tauri::command]
#[specta::specta]
pub async fn human_waiting(
    app: tauri::State<'_, App>,
    all: bool,
    reload: bool,
) -> Result<Waiting, Failure> {
    let cached = app.human.waiting.lock().expect("waiting lock").clone();
    let waiting = match cached {
        Some((covers, waiting)) if !reload && covers == all => waiting,
        _ => {
            current(&app).await?;
            let repo = if all { None } else { app.launch.initial() };
            let waiting = store::read_waiting_originals(&app.launch.state, repo.as_deref())
                .await
                .map_err(Failure::Read)?;
            *app.human.waiting.lock().expect("waiting lock") = Some((all, waiting.clone()));
            // The same read freshens every listed request's view, so its Preview shows the
            // claims and states the list shows.
            let mut views = app.human.views.lock().expect("views lock");
            for view in &waiting {
                views.insert(view.request.id.clone(), view.clone());
            }
            waiting
        }
    };
    let now = OffsetDateTime::now_utc();
    let reviewer = app.launch.reviewer.as_str();
    let rows = waiting
        .iter()
        .map(|view| {
            let request = &view.request;
            let mine = view
                .claim
                .as_ref()
                .is_some_and(|claim| claim.reviewer.as_str() == reviewer);
            WaitingRow {
                id: request.id.to_string(),
                eval: request.eval_id.to_string(),
                title: request.title.clone(),
                claim: view.claim.as_ref().map_or("-".into(), |claim| {
                    if mine {
                        format!("{} (you)", claim.reviewer)
                    } else {
                        claim.reviewer.to_string()
                    }
                }),
                mine,
                waiting: monitor::duration((now - request.created_at.time()).whole_seconds()),
                repository: request
                    .human_definition
                    .as_ref()
                    .and_then(|definition| definition.repo.value())
                    .map_or_else(
                        || "-".into(),
                        |repo| artifactize_tools::files::path_text(repo),
                    ),
            }
        })
        .collect();
    Ok(Waiting {
        generation: app.generation.current(),
        rows,
    })
}

/// Claims the standalone review took and has neither submitted nor released, for the quit
/// prompt.
#[tauri::command]
#[specta::specta]
pub fn human_taken(app: tauri::State<'_, App>) -> Vec<String> {
    app.human
        .taken
        .lock()
        .expect("taken lock")
        .iter()
        .map(ToString::to_string)
        .collect()
}
