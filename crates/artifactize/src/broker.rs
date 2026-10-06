//! Run lifecycle, scheduling, cancellation, and budget counters.

mod execution;

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::{
    agent::error::{self as agent_error, Code},
    cache,
    config::{Profile, RepoConfig},
    graph::{Evidence, Graph},
    limits::Limits,
    process,
    remote::Session,
    runtime::Verdict,
    store::{Claim, Execution, Producer, Provenance, Receipts, Request, Run, StoppedBackend},
    tools,
};

pub(crate) fn now() -> String {
    timestamp(OffsetDateTime::now_utc())
}

/// RFC 3339 in UTC with nine fractional digits, so that timestamps also sort as text.
fn timestamp(time: OffsetDateTime) -> String {
    let time = time.to_offset(time::UtcOffset::UTC);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:09}Z",
        time.year(),
        u8::from(time.month()),
        time.day(),
        time.hour(),
        time.minute(),
        time.second(),
        time.nanosecond()
    )
}

/// Any RFC 3339 time in the sortable form of [`now`]; `None` when it does not parse.
pub(crate) fn sortable(value: &str) -> Option<String> {
    OffsetDateTime::parse(value, &Rfc3339)
        .ok()
        .filter(|time| (0..=9999).contains(&time.to_offset(time::UtcOffset::UTC).year()))
        .map(timestamp)
}

/// The backends a Run stopped admitting reviews on, shared with its review tasks. A review
/// records its stop before it frees its capacity slot, so no other review of the backend
/// can take that slot and start before admission sees the stop.
#[derive(Clone)]
struct Stops(Arc<std::sync::Mutex<Vec<StoppedBackend>>>);

impl Stops {
    fn new(stopped: Vec<StoppedBackend>) -> Self {
        Self(Arc::new(std::sync::Mutex::new(stopped)))
    }

    fn all(&self) -> Vec<StoppedBackend> {
        self.0.lock().unwrap().clone()
    }

    /// The stop of the backend the request's Agent review uses, if any.
    fn of(&self, request: &Request) -> Option<StoppedBackend> {
        let backend = request.options.backend.as_deref()?;
        let stopped = self.0.lock().unwrap();
        stopped.iter().find(|stop| stop.backend == backend).cloned()
    }

    /// Stop admitting a backend after a failure every later review on it would repeat.
    fn record(&self, request: &Request) {
        let (Some(backend), Some(code)) = (&request.options.backend, &request.error_code) else {
            return;
        };
        let mut stopped = self.0.lock().unwrap();
        if Code::stops_backend(code) && !stopped.iter().any(|stop| &stop.backend == backend) {
            stopped.push(StoppedBackend {
                backend: backend.clone(),
                error_code: code.clone(),
                eval_id: request.eval_id.clone(),
                request_id: request.id.clone(),
                error: request.error.clone().unwrap_or_default(),
            });
        }
    }
}

pub(crate) fn budget_reason(run: &Run) -> String {
    format!(
        "maxExecutions budget exhausted ({} of {} executions started).",
        run.executions_started,
        run.max_executions.expect("limited Run")
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "the Run's inputs, its local and remote stores, and cancellation"
)]
pub(crate) async fn schedule(
    config: Arc<RepoConfig>,
    graph: &Graph<'_>,
    fingerprints: &BTreeMap<&str, cache::PreparedFingerprint>,
    keys: &BTreeMap<&str, cache::Key>,
    parallelism: &cache::Parallelism,
    limits: &Limits,
    run: &mut Run,
    requests: &mut [Request],
    receipts: &Receipts,
    remote: Option<Arc<Session>>,
    cancellation: CancellationToken,
) -> Result<BTreeMap<String, Evidence>, String> {
    let cancellation = cancellation.child_token();
    let _cancel_on_drop = cancellation.clone().drop_guard();
    let mut tasks = JoinSet::new();
    let result = Scheduler {
        config,
        graph,
        fingerprints,
        keys,
        parallelism,
        limits,
        run,
        requests,
        receipts,
        remote,
        cancellation: cancellation.clone(),
        tasks: &mut tasks,
    }
    .run()
    .await;
    if result.is_err() {
        cancellation.cancel();
        while tasks.join_next().await.is_some() {}
    }
    result
}

struct Scheduler<'a, 'g> {
    config: Arc<RepoConfig>,
    graph: &'a Graph<'g>,
    fingerprints: &'a BTreeMap<&'g str, cache::PreparedFingerprint>,
    keys: &'a BTreeMap<&'g str, cache::Key>,
    parallelism: &'a cache::Parallelism,
    limits: &'a Limits,
    run: &'a mut Run,
    requests: &'a mut [Request],
    receipts: &'a Receipts,
    remote: Option<Arc<Session>>,
    cancellation: CancellationToken,
    tasks: &'a mut JoinSet<Result<(usize, Request), String>>,
}

impl Scheduler<'_, '_> {
    async fn run(&mut self) -> Result<BTreeMap<String, Evidence>, String> {
        let owner = process::child_identity(std::process::id()).map_err(|e| e.to_string())?;
        let producer = Producer::current();
        // Agent conversations are saved under the state, named by its id and this producer.
        let saving = if self.limits.agent_sessions().enabled {
            Some(crate::agent::session::Saving {
                state: self.run.state_dir.clone(),
                state_id: self.receipts.state_id().await?,
                producer: producer.name.clone(),
            })
        } else {
            None
        };
        let run_dir = self.run.state_dir.join("runs").join(&self.run.id);
        let mut evidence = BTreeMap::new();
        let mut running = BTreeSet::new();
        let mut waiting = BTreeSet::new();
        // Requests waiting for a machine-wide backend slot: no job slot, no executor start.
        let mut capacity_waiting = BTreeSet::new();
        let stops = Stops::new(self.run.stopped_backends.clone());
        let deadline = self
            .run
            .wait_timeout_ms
            .map(|ms| tokio::time::Instant::now() + Duration::from_millis(u64::from(ms)));
        loop {
            let completed = evidence.len();
            // A remote result for a waiting Human key settles it locally (each verify --wait poll).
            // A forced Run never reads from the store.
            if let Some(remote) = &self.remote
                && !self.cancellation.is_cancelled()
                && !self.run.force
            {
                let keys = self
                    .requests
                    .iter()
                    .filter(|request| request.status == "WAITING_HUMAN" && !request.force)
                    .filter_map(|request| request.key.clone())
                    .collect();
                remote.refresh(self.receipts, keys).await?;
            }
            let human_ids: Vec<_> = self
                .requests
                .iter()
                .filter(|request| request.status == "WAITING_HUMAN")
                .map(|request| request.id.clone())
                .collect();
            if !human_ids.is_empty() {
                for request in self.receipts.settled_human_requests(human_ids).await? {
                    if let Some(saved) = self
                        .requests
                        .iter_mut()
                        .find(|saved| saved.id == request.id && saved.status == "WAITING_HUMAN")
                    {
                        evidence.insert(
                            request.eval_id.clone(),
                            match request.status.as_str() {
                                "GREEN" => Evidence::Current(Verdict::Green),
                                "RED" => Evidence::Current(Verdict::Red),
                                _ => Evidence::OperationalError,
                            },
                        );
                        *saved = request;
                    }
                }
            }
            if !self.cancellation.is_cancelled() {
                for eval in &self.config.evals {
                    if evidence.contains_key(&eval.id) {
                        continue;
                    }
                    let index = self
                        .requests
                        .iter()
                        .position(|request| request.eval_id == eval.id);
                    if index.is_some_and(|index| {
                        running.contains(&index)
                            || waiting.contains(&index)
                            || self.requests[index].status == "WAITING_HUMAN"
                            || self.requests[index].force
                    }) {
                        continue;
                    }
                    if let Some(key) = self.keys.get(eval.id.as_str())
                        && let Some(execution) = self.receipts.cached_execution(&key.value).await?
                    {
                        evidence.insert(
                            eval.id.clone(),
                            Evidence::Current(execution.verdict().expect("completed cache entry")),
                        );
                        if let Some(index) = index {
                            waiting.remove(&index);
                            let request = &mut self.requests[index];
                            cache::reuse(request, &execution, now());
                            self.receipts.reuse_execution(request).await?;
                        }
                    }
                }
                let mut evaluation = self
                    .graph
                    .evaluate_with_policy(&evidence, self.run.ignore_gates);
                // Backends found full in this pass are not asked again until the next one.
                let mut full = BTreeSet::new();
                // Request order preserves selection order, then recursive configuration order.
                for index in 0..self.requests.len() {
                    if self.cancellation.is_cancelled() {
                        break;
                    }
                    let request = &mut self.requests[index];
                    if running.contains(&index)
                        || request.status == "WAITING_HUMAN"
                        || evidence.contains_key(&request.eval_id)
                    {
                        continue;
                    }
                    if !evaluation.evals[request.eval_id.as_str()].can_execute() {
                        waiting.remove(&index);
                        continue;
                    }
                    if !waiting.contains(&index) && running.len() + waiting.len() >= self.run.jobs {
                        continue;
                    }
                    let mut execution = Execution {
                        id: format!("execution-{}", request.id),
                        key: request.key.clone(),
                        fingerprint: request.fingerprint.clone(),
                        fingerprints: request.fingerprints.clone(),
                        eval_def_hash: request.eval_def_hash.clone(),
                        owner_pid: owner.pid,
                        owner_start_time: owner.start_time,
                        status: "RUNNING".into(),
                        result: None,
                        error: None,
                        error_code: None,
                        profile: request.profile.clone(),
                        options: request.options.clone(),
                        usage: None,
                        tool_calls: Vec::new(),
                        provenance: Provenance {
                            repo_path: self.config.root.clone(),
                            run_id: self.run.id.clone(),
                            request_id: request.id.clone(),
                            eval_id: request.eval_id.clone(),
                            eval_def_hash: request.eval_def_hash.clone(),
                            completed_at: None,
                            execution_paths: Default::default(),
                        },
                        started_at: now(),
                        completed_at: None,
                        producer: Some(producer.clone()),
                        reviewer: None,
                        origin: None,
                        manifest: None,
                    };
                    if execution.key.is_some() {
                        execution.manifest =
                            self.fingerprints[request.target.as_str()].manifest.clone();
                    }
                    let human = matches!(
                        self.config
                            .evals
                            .iter()
                            .find(|eval| eval.id == request.eval_id)
                            .expect("included eval")
                            .declaration
                            .profile,
                        Profile::Human {}
                    );
                    // A backend this Run stopped admits no new reviews and takes no slot; reuse and
                    // joining a live execution of the key still work.
                    let backend_stopped = stops.of(request);
                    let allow_start = backend_stopped.is_none()
                        && (human
                            || self
                                .run
                                .max_executions
                                .is_none_or(|limit| self.run.executions_started < limit));
                    // An Agent review of a backend with a machine-wide limit (limits.json) also
                    // needs one of its slots. The claim is probed first, so a request that would
                    // reuse or join takes no slot; one that would start waits for a slot without
                    // holding a job slot or an executor start.
                    let limited = request.options.backend.as_deref().and_then(|backend| {
                        Some((backend.to_owned(), self.limits.limit(backend)?))
                    });
                    let mut claim = claim(
                        self.receipts,
                        self.remote.as_deref(),
                        self.run.force,
                        &execution,
                        request,
                        allow_start && limited.is_none(),
                    )
                    .await?;
                    let mut slot = None;
                    if let (Claim::BudgetExhausted, Some((backend, limit)), true) =
                        (&claim, &limited, allow_start)
                    {
                        if full.contains(backend)
                            || !self
                                .receipts
                                .acquire_slot(backend, *limit, &execution.id, owner)
                                .await?
                        {
                            full.insert(backend.clone());
                            waiting.remove(&index);
                            if capacity_waiting.insert(index) {
                                request.blocked_reason = Some(format!(
                                    "Waiting for a free {backend} slot: all {limit} are in use on this machine (limits.json)."
                                ));
                                self.receipts.save_request(request).await?;
                            }
                            continue;
                        }
                        claim = self::claim(
                            self.receipts,
                            self.remote.as_deref(),
                            self.run.force,
                            &execution,
                            request,
                            true,
                        )
                        .await?;
                        if matches!(claim, Claim::Owned) {
                            slot = Some(execution.id.clone());
                        } else {
                            self.receipts.release_slot(&execution.id).await?;
                        }
                    }
                    capacity_waiting.remove(&index);
                    match claim {
                        Claim::Reuse(execution) => {
                            waiting.remove(&index);
                            evidence.insert(
                                request.eval_id.clone(),
                                Evidence::Current(
                                    execution.verdict().expect("completed cache entry"),
                                ),
                            );
                            cache::reuse(request, &execution, now());
                            self.receipts.reuse_execution(request).await?;
                            evaluation = self
                                .graph
                                .evaluate_with_policy(&evidence, self.run.ignore_gates);
                            continue;
                        }
                        Claim::WaitHuman(id) => {
                            waiting.remove(&index);
                            request.execution_id = Some(id);
                            request.status = "WAITING_HUMAN".into();
                            request.blocked_reason = Some(
                                "Waiting for the active Human execution of this reuse key.".into(),
                            );
                            *request = self.receipts.follow_human(request).await?;
                            if request.status != "WAITING_HUMAN" {
                                evidence.insert(
                                    request.eval_id.clone(),
                                    match request.status.as_str() {
                                        "GREEN" => Evidence::Current(Verdict::Green),
                                        "RED" => Evidence::Current(Verdict::Red),
                                        _ => Evidence::OperationalError,
                                    },
                                );
                                evaluation = self
                                    .graph
                                    .evaluate_with_policy(&evidence, self.run.ignore_gates);
                            }
                            continue;
                        }
                        Claim::Wait(id) => {
                            waiting.insert(index);
                            if request.execution_id.as_ref() != Some(&id) {
                                request.execution_id = Some(id);
                                request.status = "QUEUED".into();
                                request.blocked_reason = Some(
                                    "Waiting for the active execution of this reuse key.".into(),
                                );
                                self.receipts.save_request(request).await?;
                            }
                            continue;
                        }
                        Claim::BudgetExhausted if let Some(cause) = backend_stopped => {
                            waiting.remove(&index);
                            request.status = "ERROR".into();
                            // A request waiting for a slot stops waiting.
                            request.blocked_reason = None;
                            request.error_code = Some(agent_error::BACKEND_STOPPED.into());
                            request.error = Some(format!(
                                "Not started: this Run stopped admitting {} reviews after {} in {}: {}",
                                cause.backend, cause.error_code, cause.eval_id, cause.error
                            ));
                            request.completed_at = Some(now());
                            self.receipts.save_request(request).await?;
                            evidence.insert(request.eval_id.clone(), Evidence::OperationalError);
                            evaluation = self
                                .graph
                                .evaluate_with_policy(&evidence, self.run.ignore_gates);
                            continue;
                        }
                        Claim::BudgetExhausted => {
                            waiting.remove(&index);
                            request.status = "BUDGET_EXHAUSTED".into();
                            request.blocked_reason = Some(budget_reason(self.run));
                            self.receipts.save_request(request).await?;
                            continue;
                        }
                        Claim::Owned => {}
                    }
                    waiting.remove(&index);
                    // Followers join a claimed execution by its id.
                    request.execution_id = execution
                        .key
                        .as_ref()
                        .filter(|_| !request.force)
                        .map(|_| execution.id.clone());
                    request.blocked_reason = None;
                    let eval = self
                        .config
                        .evals
                        .iter()
                        .find(|eval| eval.id == request.eval_id)
                        .expect("included eval");
                    let prepared = execution::prepare(
                        &self.config,
                        eval,
                        &run_dir,
                        &self.run.state_dir,
                        saving.as_ref(),
                        request,
                    );
                    if prepared.is_ok() && !human && !self.cancellation.is_cancelled() {
                        self.run.executions_started += 1;
                        execution.started_at = now();
                        request.started_at = Some(execution.started_at.clone());
                    }
                    request.status = "RUNNING".into();
                    self.receipts.save_run(self.run).await?;
                    self.receipts.save_request(request).await?;
                    let config = self.config.clone();
                    let receipts = self.receipts.clone();
                    let request = request.clone();
                    let remote = self.remote.clone();
                    let cancellation = self.cancellation.clone();
                    let run_dir = run_dir.clone();
                    let parallelism = self.parallelism.clone();
                    let agent = request.profile["kind"] == "agent";
                    let stops = stops.clone();
                    running.insert(index);
                    self.tasks.spawn(async move {
                        let receipts_for_slot = receipts.clone();
                        let mut execution = execution;
                        // An Agent result pins the files its tools execute, hashed before the
                        // review starts; a path that cannot be hashed fails the preparation.
                        let prepared = match prepared {
                            Ok(prepared) if agent && !cancellation.is_cancelled() => {
                                match tools::pins::execution_paths(&config, &request.eval_id).await
                                {
                                    Ok(pins) => {
                                        execution.provenance.execution_paths = pins;
                                        Ok(prepared)
                                    }
                                    Err(error) => Err(error),
                                }
                            }
                            prepared => prepared,
                        };
                        let request = execution::execute(
                            config,
                            receipts.clone(),
                            request,
                            execution,
                            prepared,
                            run_dir,
                            &parallelism,
                            cancellation,
                        )
                        .await;
                        // Admission sees a stop before this review's slot frees up.
                        if let Ok(request) = &request {
                            stops.record(request);
                        }
                        if let Some(slot) = slot {
                            receipts_for_slot.release_slot(&slot).await?;
                        }
                        let request = request?;
                        // Publication runs after the local commit, outside any transaction.
                        if let Some(remote) = remote {
                            remote.publish_request(&receipts, &request).await?;
                        }
                        Ok((index, request))
                    });
                }
            }
            let cancelled = self.cancellation.is_cancelled();
            if !cancelled && evidence.len() != completed {
                continue;
            }
            let human_wait = deadline.is_some()
                && self
                    .requests
                    .iter()
                    .any(|request| request.status == "WAITING_HUMAN");
            if self.tasks.is_empty()
                && ((waiting.is_empty() && capacity_waiting.is_empty()) || cancelled)
            {
                if cancelled || !human_wait {
                    break;
                }
                if deadline.is_some_and(|deadline| tokio::time::Instant::now() >= deadline) {
                    self.run.wait_timed_out = true;
                    break;
                }
            }
            let poll = deadline
                .filter(|_| human_wait)
                .map(|deadline| deadline.saturating_duration_since(tokio::time::Instant::now()))
                .filter(|remaining| !remaining.is_zero())
                .unwrap_or(Duration::from_millis(200))
                .min(Duration::from_millis(200));
            tokio::select! {
                biased;
                result = self.tasks.join_next(), if !self.tasks.is_empty() => {
                    let (index, request) = result.expect("active tasks").map_err(|e| e.to_string())??;
                    running.remove(&index);
                    let stopped = stops.all();
                    if stopped.len() != self.run.stopped_backends.len() {
                        self.run.stopped_backends = stopped;
                        self.receipts.save_run(self.run).await?;
                    }
                    if request.status != "WAITING_HUMAN" {
                        evidence.insert(request.eval_id.clone(), match request.status.as_str() {
                            "GREEN" => Evidence::Current(Verdict::Green),
                            "RED" => Evidence::Current(Verdict::Red),
                            _ => Evidence::OperationalError,
                        });
                    }
                    self.requests[index] = request;
                }
                _ = self.cancellation.cancelled(), if !cancelled => {},
                _ = tokio::time::sleep(poll), if (!waiting.is_empty() || !capacity_waiting.is_empty() || human_wait) && !cancelled => {},
            }
        }
        Ok(evidence)
    }
}

/// Claim an execution for a request: a forced review takes no claim and joins nothing, while
/// any other first looks up the remote store again (a hit becomes a local record).
async fn claim(
    receipts: &Receipts,
    remote: Option<&Session>,
    run_force: bool,
    execution: &Execution,
    request: &Request,
    allow_start: bool,
) -> Result<Claim, String> {
    if request.force {
        return Ok(if allow_start {
            Claim::Owned
        } else {
            Claim::BudgetExhausted
        });
    }
    if let (Some(remote), Some(key), false) = (remote, &execution.key, run_force) {
        remote.refresh(receipts, vec![key.clone()]).await?;
    }
    receipts
        .claim_execution(execution, request.execution_id.as_deref(), allow_start)
        .await
}
