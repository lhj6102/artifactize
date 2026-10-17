//! Run lifecycle, scheduling, cancellation, and budget counters.

mod execution;
mod modes;

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::{
    agent::error::Code,
    cache,
    config::{ProfileKind, RepoConfig},
    graph::{EvalStatus, Evidence, Graph},
    limits::Limits,
    process,
    remote::Session,
    runtime::Verdict,
    store::{
        Capacity, Claim, Execution, Producer, Provenance, Receipts, Request, Run, StoppedBackend,
    },
    tools,
};

/// OS owner death and released machine-wide slots have no reliable writer notification.
/// Bound their reconciliation independently of immediate commit hints and task completion.
const OWNER_RECONCILE: Duration = Duration::from_secs(1);

#[cfg(test)]
mod wake_tests {
    /// A timestamp's saved text, or `None` when it is not a sortable time.
    fn sortable(value: &str) -> Option<String> {
        value
            .parse::<crate::types::Timestamp>()
            .ok()
            .map(|time| time.to_string())
    }
    #[test]
    fn sortable_rfc3339_year_bounds_keep_fixed_width_utc_text() {
        assert_eq!(
            sortable("0000-01-01T00:00:00Z").as_deref(),
            Some("0000-01-01T00:00:00.000000000Z")
        );
        assert_eq!(
            sortable("9999-12-31T23:59:59Z").as_deref(),
            Some("9999-12-31T23:59:59.000000000Z")
        );
        assert_eq!(sortable("0000-01-01T00:00:00+01:00"), None);
        assert_eq!(sortable("10000-01-01T00:00:00Z"), None);
    }
    #[test]
    fn sortable_rejects_utc_offset_overflow_without_panicking() {
        assert_eq!(sortable("9999-12-31T23:59:59-01:00"), None);
        assert_eq!(sortable("0000-01-01T00:00:00+01:00"), None);
        assert_eq!(sortable("9999-12-31T23:59:59+24:00"), None);
        assert_eq!(sortable("0000-01-01T00:00:00-24:00"), None);
        assert_eq!(
            sortable("9999-12-31T22:59:59-01:00").as_deref(),
            Some("9999-12-31T23:59:59.000000000Z")
        );
        assert_eq!(
            sortable("0000-01-01T01:00:00+01:00").as_deref(),
            Some("0000-01-01T00:00:00.000000000Z")
        );
    }
}

pub(crate) fn now() -> crate::types::Timestamp {
    crate::types::Timestamp::now()
}

/// The backends a Run stopped admitting reviews on, shared with its review tasks. A review
/// records its stop before its execution completes and frees its capacity slot, and taking a
/// slot asks again under the same write lock, so no other review of the backend can take the
/// slot and start.
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
        let backend = request.options.backend?;
        let stopped = self.0.lock().unwrap();
        stopped.iter().find(|stop| stop.backend == backend).cloned()
    }

    /// Whether `backend` stopped, asked later.
    fn check(&self, backend: &crate::config::Backend) -> Box<dyn FnOnce() -> bool + Send> {
        let (stops, backend) = (self.clone(), *backend);
        Box::new(move || {
            let stopped = stops.0.lock().unwrap();
            stopped.iter().any(|stop| stop.backend == backend)
        })
    }

    /// Stop admitting a backend after a failure every later review on it would repeat.
    fn record(&self, request: &Request) {
        let (Some(backend), Some(code)) = (&request.options.backend, &request.error_code()) else {
            return;
        };
        let mut stopped = self.0.lock().unwrap();
        if Code::stops_backend(code) && !stopped.iter().any(|stop| &stop.backend == backend) {
            stopped.push(StoppedBackend {
                backend: *backend,
                error_code: (*code).try_into().expect("backend stopping failure"),
                eval_id: request.eval_id.clone(),
                request_id: request.id.clone(),
                error: request.error().map(str::to_owned).unwrap_or_default(),
            });
        }
    }
}

/// Why an eval of a `--reuse-only` kind was not executed.
fn not_reused_reason(kind: ProfileKind) -> String {
    format!(
        "Not reused (--reuse-only {}): no cached or stored result, so it was not executed.",
        kind.name()
    )
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
    fingerprints: &BTreeMap<&crate::types::ArtifactName, cache::PreparedFingerprint>,
    keys: &BTreeMap<&crate::types::EvalId, cache::Key>,
    parallelism: &cache::Parallelism,
    limits: &Limits,
    run: &mut Run,
    requests: &mut [Request],
    receipts: &Receipts,
    remote: Option<Arc<Session>>,
    cancellation: CancellationToken,
) -> Result<BTreeMap<crate::config::EvalId, Evidence>, String> {
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
        while let Some(result) = tasks.join_next().await {
            let _ = crate::task::joined(result);
        }
    }
    result
}

/// One request's scheduler ownership, separate from its saved verdict. A cache hit or
/// preparation error replaces its wait, so finished work cannot keep the Run waiting.
enum Admission {
    /// No local task or external admission wait: not yet admitted, gated, or finished.
    Idle,
    Running {
        cancellation: CancellationToken,
        stop: Option<modes::Stop>,
    },
    /// Joining a live execution uses a job slot; awaiting backend capacity does not.
    Joining,
    Capacity,
    Mutex,
    Blocked,
}

impl Admission {
    fn running(&self) -> bool {
        matches!(self, Self::Running { .. })
    }

    fn blocked(&self) -> bool {
        matches!(
            self,
            Self::Blocked
                | Self::Running {
                    stop: Some(modes::Stop::Blocked),
                    ..
                }
        )
    }

    fn uses_job(&self) -> bool {
        matches!(self, Self::Running { .. } | Self::Joining)
    }

    fn waiting(&self) -> bool {
        matches!(self, Self::Joining | Self::Capacity | Self::Mutex)
    }

    fn stop(&mut self, reason: modes::Stop) {
        let Self::Running { cancellation, stop } = self else {
            unreachable!("only a running request can be stopped");
        };
        *stop = Some(reason);
        cancellation.cancel();
    }
}

struct Scheduler<'a, 'g> {
    config: Arc<RepoConfig>,
    graph: &'a Graph<'g>,
    fingerprints: &'a BTreeMap<&'g crate::types::ArtifactName, cache::PreparedFingerprint>,
    keys: &'a BTreeMap<&'g crate::types::EvalId, cache::Key>,
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
    async fn run(&mut self) -> Result<BTreeMap<crate::config::EvalId, Evidence>, String> {
        // Registration precedes any baseline/scheduling read; racing commits stay dirty.
        let mut changes = crate::changes::Subscription::new(&self.run.state_dir).await;
        let result = self.subscribed(&mut changes).await;
        let closed = changes.close().await;
        match (result, closed) {
            (Ok(evidence), Ok(())) => Ok(evidence),
            (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
            (Err(error), Err(close)) => {
                Err(format!("{error}; closing scheduler subscription: {close}"))
            }
        }
    }

    async fn subscribed(
        &mut self,
        changes: &mut crate::changes::Subscription,
    ) -> Result<BTreeMap<crate::config::EvalId, Evidence>, String> {
        let mut reconcile = tokio::time::interval(OWNER_RECONCILE);
        reconcile.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        reconcile.tick().await;
        let mut remote_due = tokio::time::Instant::now();
        let owner = process::child_identity(std::process::id()).map_err(|e| e.to_string())?;
        let producer = Producer::current();
        // Agent conversations are saved under the state, named by its id and this producer.
        let saving = if self.limits.agent_sessions().enabled {
            Some(crate::agent::session::Saving {
                state: self.run.state_dir.clone(),
                state_id: self.receipts.state_id().await?,
                producer: producer.name.parse()?,
            })
        } else {
            None
        };
        let run_dir = self.run.state_dir.join("runs").join(self.run.id.as_str());
        let mut evidence = BTreeMap::new();
        // Saved evals this Run has no request for: their evidence is recorded on the Run.
        let saved_evals: BTreeSet<crate::types::EvalId> = self
            .run
            .definitions
            .graph()
            .map(|graph| graph.evals().iter().map(|eval| eval.id.clone()).collect())
            .unwrap_or_default();
        let deferred = modes::deferred(&self.config, self.run, self.requests);
        let mut admissions: Vec<_> = self.requests.iter().map(|_| Admission::Idle).collect();
        let stops = Stops::new(self.run.stopped_backends.clone());
        loop {
            let completed = evidence.len();
            // Optional team-store lookups retain their separate due cadence; local session
            // events and cached redraws never schedule remote work. Forced Runs read none.
            if let Some(remote) = &self.remote
                && tokio::time::Instant::now() >= remote_due
                && !self.cancellation.is_cancelled()
                && !self.run.force
            {
                let keys = self
                    .requests
                    .iter()
                    .filter(|request| request.status().is_human_wait() && !request.force)
                    .filter_map(|request| request.key.clone())
                    .collect();
                remote.refresh(self.receipts, keys).await?;
                remote_due = tokio::time::Instant::now() + crate::remote::REFRESH_INTERVAL;
            }
            let human_ids: Vec<_> = self
                .requests
                .iter()
                .filter(|request| request.status().is_human_wait())
                .map(|request| request.id.clone())
                .collect();
            if !human_ids.is_empty() {
                for request in self.receipts.settled_human_requests(human_ids).await? {
                    if let Some(saved) = self
                        .requests
                        .iter_mut()
                        .find(|saved| saved.id == request.id && saved.status().is_human_wait())
                    {
                        evidence.insert(
                            request.eval_id.clone(),
                            match request.status() {
                                crate::types::RequestStatus::Green => {
                                    Evidence::Current(Verdict::Green)
                                }
                                crate::types::RequestStatus::Red => Evidence::Current(Verdict::Red),
                                _ => Evidence::OperationalError,
                            },
                        );
                        *saved = request;
                    }
                }
            }
            if !self.cancellation.is_cancelled() {
                let recorded = self.run.evidence.len();
                for eval in &self.config.evals {
                    if matches!(evidence.get(&eval.id), Some(value) if *value != Evidence::Deferred)
                    {
                        continue;
                    }
                    let index = self
                        .requests
                        .iter()
                        .position(|request| request.eval_id == eval.id);
                    if index.is_some_and(|index| {
                        admissions[index].blocked()
                            || admissions[index].running()
                            || matches!(admissions[index], Admission::Joining)
                            || self.requests[index].status().is_human_wait()
                            || self.requests[index].force
                    }) {
                        continue;
                    }
                    if let Err(error) = cache::validate_file_inputs(&self.config, eval) {
                        evidence.insert(eval.id.clone(), Evidence::OperationalError);
                        if index.is_none() && saved_evals.contains(&eval.id) {
                            self.run
                                .evidence
                                .insert(eval.id.clone(), crate::types::RequestStatus::Error);
                        }
                        if let Some(index) = index {
                            admissions[index] = Admission::Idle;
                            let request = &mut self.requests[index];
                            request.queue = None;
                            request.blocked_reason = None;
                            request.state = crate::store::RequestState::failed(
                                error,
                                Some(crate::types::FailureCode::PreparationFailed),
                                now(),
                            );
                            self.receipts.save_request(request).await?;
                        }
                        continue;
                    }
                    if let Some(key) = self.keys.get(&eval.id)
                        && let Some(execution) = self.receipts.cached_execution(&key.value).await?
                    {
                        let verdict = execution.verdict().expect("completed cache entry");
                        evidence.insert(eval.id.clone(), Evidence::Current(verdict));
                        if index.is_none() && saved_evals.contains(&eval.id) {
                            self.run.evidence.insert(
                                eval.id.clone(),
                                match verdict {
                                    Verdict::Green => crate::types::RequestStatus::Green,
                                    Verdict::Red => crate::types::RequestStatus::Red,
                                },
                            );
                        }
                        if let Some(index) = index {
                            admissions[index] = Admission::Idle;
                            let request = &mut self.requests[index];
                            cache::reuse(request, &execution, now());
                            self.receipts.reuse_execution(request).await?;
                        }
                    }
                }
                if self.run.evidence.len() != recorded {
                    self.receipts.save_run(self.run).await?;
                }
                for id in &deferred {
                    if !self.requests.iter().enumerate().any(|(index, request)| {
                        request.eval_id == *id && admissions[index].blocked()
                    }) {
                        evidence.entry(id.clone()).or_insert(Evidence::Deferred);
                    }
                }
                let mut evaluation = self
                    .graph
                    .evaluate_for_run(&evidence, self.run.modes.speculate);
                // Failures cancel transitively, including tasks, joined waits and sign-offs.
                for (request, admission) in self.requests.iter_mut().zip(admissions.iter_mut()) {
                    let eval = &evaluation.evals[request.eval_id.as_str()];
                    let sibling_failed = self.run.modes.defer_human
                        && request.profile.kind() == ProfileKind::Human
                        && modes::machines(&self.config, request).any(|id| {
                            matches!(
                                evidence.get(id),
                                Some(Evidence::Current(Verdict::Red))
                                    | Some(Evidence::OperationalError)
                            )
                        });
                    if (self.run.modes.speculate || self.run.modes.defer_human)
                        && (eval.readiness == crate::graph::Readiness::Blocked || sibling_failed)
                        && !admission.blocked()
                    {
                        evidence.remove(&request.eval_id);
                        if admission.running() {
                            admission.stop(modes::Stop::Blocked);
                        } else {
                            *admission = Admission::Blocked;
                            request.state = crate::store::RequestState::pending(
                                crate::types::RequestStatus::Blocked,
                                Some(now()),
                            )
                            .expect("blocked state");
                            request.blocked_by =
                                eval.blocked_by.iter().copied().map(Into::into).collect();
                            request.blocked_reason = Some(if sibling_failed {
                                "Artifact machine eval failed; sign-off withdrawn.".into()
                            } else {
                                crate::project::verify::gate_reason(eval)
                            });
                            request.queue = None;
                            self.receipts.block_request(request).await?;
                        }
                    }
                }
                evaluation = self
                    .graph
                    .evaluate_for_run(&evidence, self.run.modes.speculate);
                let order = modes::order(self.graph, self.requests, &evidence);
                // Backends found full in this pass are not asked again until the next one.
                let mut full = BTreeSet::new();
                // Request order preserves selection order, then recursive configuration order.
                for index in order {
                    if self.cancellation.is_cancelled() {
                        break;
                    }
                    let current = &self.requests[index];
                    let eval = &evaluation.evals[current.eval_id.as_str()];
                    let late_ready = !self.run.modes.defer_human
                        || current.profile.kind() != ProfileKind::Human
                        || modes::machines(&self.config, current).all(|id| {
                            matches!(evidence.get(id), Some(Evidence::Current(Verdict::Green)))
                        });
                    let ready = late_ready
                        && (eval.can_execute()
                            || (deferred.contains(&current.eval_id)
                                && eval.readiness == crate::graph::Readiness::Ready));
                    let can_start = current.profile.kind() == ProfileKind::Human
                        || self
                            .run
                            .max_executions
                            .is_none_or(|limit| self.run.executions_started < limit);
                    if ready
                        && can_start
                        && !current.status().is_human_wait()
                        && !admissions[index].running()
                        && !admissions[index].blocked()
                    {
                        // Ready work owns capacity; speculative tasks yield before any ready task.
                        let backend = current.options.backend;
                        let jobs_full = !matches!(admissions[index], Admission::Joining)
                            && self.run.jobs.is_some_and(|limit| {
                                admissions.iter().filter(|state| state.uses_job()).count() >= limit
                            });
                        let backend_full = backend.is_some_and(|backend| {
                            matches!(admissions[index], Admission::Capacity)
                                && admissions.iter().enumerate().any(|(active, state)| {
                                    state.running()
                                        && self.requests[active].options.backend == Some(backend)
                                })
                        });
                        if jobs_full || backend_full {
                            if let Some(active) = admissions
                                .iter()
                                .zip(self.requests.iter())
                                .rposition(|(state, request)| {
                                    matches!(state, Admission::Running { stop: None, .. })
                                        && evaluation.evals[request.eval_id.as_str()].readiness
                                            == crate::graph::Readiness::Wait
                                        && (!backend_full || request.options.backend == backend)
                                })
                            {
                                admissions[active].stop(modes::Stop::Yield);
                            }
                            if let Some(active) = admissions
                                .iter()
                                .zip(self.requests.iter())
                                .rposition(|(state, request)| {
                                    matches!(state, Admission::Joining)
                                        && evaluation.evals[request.eval_id.as_str()].readiness
                                            == crate::graph::Readiness::Wait
                                })
                            {
                                admissions[active] = Admission::Idle;
                            }
                        }
                    }
                    let request = &mut self.requests[index];
                    if admissions[index].blocked()
                        || admissions[index].running()
                        || request.status().is_human_wait()
                        || matches!(evidence.get(&request.eval_id), Some(value) if *value != Evidence::Deferred)
                    {
                        continue;
                    }
                    let eval = &evaluation.evals[request.eval_id.as_str()];
                    let speculative = self.run.modes.speculate
                        && eval.readiness == crate::graph::Readiness::Wait
                        && matches!(eval.evidence, None | Some(Evidence::Stale));
                    let deferred_human = deferred.contains(&request.eval_id);
                    let may_execute = eval.can_execute()
                        || speculative
                        || (deferred_human && eval.readiness == crate::graph::Readiness::Ready);
                    if !may_execute {
                        admissions[index] = Admission::Idle;
                        let status = crate::project::verify::status(eval.status);
                        let blocked_by: Vec<_> =
                            eval.blocked_by.iter().copied().map(Into::into).collect();
                        if request.status() != status || request.blocked_by != blocked_by {
                            request.state =
                                crate::store::RequestState::pending(status, request.completed_at())
                                    .expect("gated request");
                            request.blocked_by = blocked_by;
                            request.queue = None;
                            request.blocked_reason =
                                Some(crate::project::verify::gate_reason(eval));
                            self.receipts.save_request(request).await?;
                        }
                        continue;
                    }
                    if self.run.modes.defer_human
                        && request.profile.kind() == ProfileKind::Human
                        && (eval.readiness != crate::graph::Readiness::Ready
                            || modes::machines(&self.config, request).any(|id| {
                                !matches!(
                                    evidence.get(id),
                                    Some(Evidence::Current(_)) | Some(Evidence::OperationalError)
                                )
                            }))
                    {
                        continue;
                    }
                    if !matches!(admissions[index], Admission::Joining)
                        && self.run.jobs.is_some_and(|limit| {
                            admissions.iter().filter(|state| state.uses_job()).count() >= limit
                        })
                    {
                        continue;
                    }
                    let declared = self
                        .config
                        .evals
                        .iter()
                        .find(|eval| eval.id == request.eval_id)
                        .expect("included Eval");
                    if let Err(error) = cache::validate_file_inputs(&self.config, declared) {
                        admissions[index] = Admission::Idle;
                        request.queue = None;
                        request.blocked_reason = None;
                        request.state = crate::store::RequestState::failed(
                            error,
                            Some(crate::types::FailureCode::PreparationFailed),
                            now(),
                        );
                        self.receipts.save_request(request).await?;
                        evidence.insert(request.eval_id.clone(), Evidence::OperationalError);
                        evaluation = self
                            .graph
                            .evaluate_for_run(&evidence, self.run.modes.speculate);
                        continue;
                    }
                    let mut execution = Execution {
                        state: crate::store::ExecutionState::Running,
                        id: format!("execution-{}-{}", request.id, crate::agent::uuid()?)
                            .parse()?,
                        key: request.key.clone(),
                        fingerprint: request.fingerprint.clone(),
                        fingerprints: request.fingerprints.clone(),
                        artifact_kinds: self
                            .keys
                            .get(&request.eval_id)
                            .map(|key| key.artifact_kinds.clone())
                            .unwrap_or_default(),
                        eval_def_hash: request.eval_def_hash.clone(),
                        owner: Some(owner),
                        profile: request.profile.clone(),
                        options: request.options.clone(),
                        usage: None,
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
                        producer: Some(producer.clone()),
                        reviewer: None,
                        origin: None,
                        manifest: None,
                    };
                    if execution.key.is_some() {
                        execution.manifest = self.fingerprints[&request.target].manifest.clone();
                    }
                    let kind = self
                        .config
                        .evals
                        .iter()
                        .find(|eval| eval.id == request.eval_id)
                        .expect("included eval")
                        .declaration
                        .profile()
                        .kind();
                    let human = kind == ProfileKind::Human;
                    // An eval of a --reuse-only kind starts nothing, joins nothing and requests
                    // no Human sign-off: it reuses a completed result or is not executed.
                    let reuse_only = self.run.reuse_only.contains(&kind);
                    // A backend this Run stopped admits no new reviews and takes no slot; reuse and
                    // joining a live execution of the key still work.
                    let mut backend_stopped = stops.of(request);
                    let allow_start = !reuse_only
                        && backend_stopped.is_none()
                        && (human
                            || self
                                .run
                                .max_executions
                                .is_none_or(|limit| self.run.executions_started < limit));
                    // An Agent review of a backend with a machine-wide limit (limits.json) also
                    // needs one of its slots. The claim is probed first, so a request that would
                    // reuse or join takes no slot; one that would start waits for a slot without
                    // holding a job slot or an executor start.
                    let limited = match &request.profile {
                        crate::config::StoredProfile::Agent { backend, .. } => request
                            .options
                            .backend
                            .as_ref()
                            .and_then(|name| Some((*name, self.limits.limit(*backend)?))),
                        _ => None,
                    };
                    let mut claim = claim(
                        self.receipts,
                        self.remote.as_deref(),
                        self.run.force,
                        &execution,
                        request,
                        allow_start && limited.is_none(),
                        None,
                    )
                    .await?;
                    if let (Claim::BudgetExhausted, Some((backend, limit)), true) =
                        (&claim, &limited, allow_start)
                    {
                        claim = if full.contains(backend) {
                            Claim::Full
                        } else {
                            let claim = self::claim(
                                self.receipts,
                                self.remote.as_deref(),
                                self.run.force,
                                &execution,
                                request,
                                true,
                                Some(Capacity {
                                    limit: *limit,
                                    stopped: stops.check(backend),
                                }),
                            )
                            .await?;
                            // The backend may have stopped while this review waited for a slot.
                            backend_stopped = stops.of(request);
                            claim
                        };
                    }
                    match claim {
                        Claim::Mutex { eval_id, run_id } => {
                            admissions[index] = Admission::Mutex;
                            let reason = format!(
                                "Waiting for mutex tags held by eval {eval_id} in Run {run_id}."
                            );
                            if request.blocked_reason.as_ref() != Some(&reason) {
                                request.state = crate::store::RequestState::pending(
                                    crate::types::RequestStatus::Queued,
                                    None,
                                )
                                .expect("mutex waiter is queued");
                                request.queue = None;
                                request.blocked_reason = Some(reason);
                                self.receipts.save_request(request).await?;
                            }
                            continue;
                        }
                        Claim::Full => {
                            let (backend, limit) = limited.as_ref().expect("a limited backend");
                            full.insert(*backend);
                            let newly_waiting = !matches!(admissions[index], Admission::Capacity);
                            admissions[index] = Admission::Capacity;
                            if newly_waiting {
                                request.queue =
                                    Some(crate::store::QueueReason::Slot { backend: *backend });
                                request.blocked_reason = Some(format!(
                                    "Waiting for a free {backend} slot: all {limit} are in use on this machine (limits.json)."
                                ));
                                self.receipts.save_request(request).await?;
                            }
                            continue;
                        }
                        Claim::Reuse(execution) => {
                            admissions[index] = Admission::Idle;
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
                                .evaluate_for_run(&evidence, self.run.modes.speculate);
                            continue;
                        }
                        // Nothing to reuse: the evidence stays stale, so dependents wait as
                        // behind any missing evidence, and the Run ends INCOMPLETE.
                        _ if reuse_only => {
                            admissions[index] = Admission::Idle;
                            request.state = crate::store::RequestState::pending(
                                crate::types::RequestStatus::Stale,
                                request.completed_at(),
                            )
                            .expect("pending request state");
                            request.queue = None;
                            request.blocked_reason = Some(not_reused_reason(kind));
                            self.receipts.save_request(request).await?;
                            evidence.insert(request.eval_id.clone(), Evidence::Stale);
                            evaluation = self
                                .graph
                                .evaluate_for_run(&evidence, self.run.modes.speculate);
                            continue;
                        }
                        Claim::WaitHuman(id) => {
                            admissions[index] = Admission::Idle;
                            request.execution_id = Some(id);
                            request.state = if deferred_human {
                                crate::store::RequestState::Deferred
                            } else {
                                crate::store::RequestState::WaitingHuman
                            };
                            request.queue = None;
                            request.blocked_reason = Some(
                                "Waiting for the active Human execution of this reuse key.".into(),
                            );
                            *request = self.receipts.follow_human(request).await?;
                            if !request.status().is_human_wait() {
                                evidence.insert(
                                    request.eval_id.clone(),
                                    match request.status() {
                                        crate::types::RequestStatus::Green => {
                                            Evidence::Current(Verdict::Green)
                                        }
                                        crate::types::RequestStatus::Red => {
                                            Evidence::Current(Verdict::Red)
                                        }
                                        _ => Evidence::OperationalError,
                                    },
                                );
                                evaluation = self
                                    .graph
                                    .evaluate_for_run(&evidence, self.run.modes.speculate);
                            }
                            continue;
                        }
                        Claim::Wait(id) => {
                            admissions[index] = Admission::Joining;
                            if request.execution_id.as_ref() != Some(&id) {
                                request.execution_id = Some(id);
                                request.state = crate::store::RequestState::pending(
                                    crate::types::RequestStatus::Queued,
                                    request.completed_at(),
                                )
                                .expect("pending request state");
                                request.queue = None;
                                request.blocked_reason = Some(
                                    "Waiting for the active execution of this reuse key.".into(),
                                );
                                self.receipts.save_request(request).await?;
                            }
                            continue;
                        }
                        Claim::BudgetExhausted if let Some(cause) = backend_stopped => {
                            admissions[index] = Admission::Idle;
                            // A request waiting for a slot stops waiting.
                            request.queue = None;
                            request.blocked_reason = None;
                            request.state = crate::store::RequestState::failed(
                                format!(
                                    "Not started: this Run stopped admitting {} reviews after {} in {}: {}",
                                    cause.backend, cause.error_code, cause.eval_id, cause.error
                                ),
                                Some(crate::types::FailureCode::BackendStopped),
                                now(),
                            );
                            self.receipts.save_request(request).await?;
                            evidence.insert(request.eval_id.clone(), Evidence::OperationalError);
                            evaluation = self
                                .graph
                                .evaluate_for_run(&evidence, self.run.modes.speculate);
                            continue;
                        }
                        Claim::BudgetExhausted => {
                            admissions[index] = Admission::Idle;
                            let reason = budget_reason(self.run);
                            // Keep looking for reuse, but an unchanged budget decision cannot
                            // create a self-notification → write → notification hot loop.
                            if request.status() != crate::types::RequestStatus::BudgetExhausted
                                || request.blocked_reason.as_deref() != Some(reason.as_str())
                            {
                                request.state = crate::store::RequestState::pending(
                                    crate::types::RequestStatus::BudgetExhausted,
                                    request.completed_at(),
                                )
                                .expect("pending request state");
                                request.queue = None;
                                request.blocked_reason = Some(reason);
                                self.receipts.save_request(request).await?;
                            }
                            continue;
                        }
                        Claim::Owned => {}
                    }
                    admissions[index] = Admission::Idle;
                    // Followers join a claimed execution by its id.
                    request.execution_id = execution
                        .key
                        .as_ref()
                        .filter(|_| !request.force)
                        .map(|_| execution.id.clone());
                    request.queue = None;
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
                        request.started_at = Some(execution.started_at);
                    }
                    request.state = crate::store::RequestState::Running;
                    self.receipts.save_run(self.run).await?;
                    self.receipts.save_request(request).await?;
                    let config = self.config.clone();
                    let receipts = self.receipts.clone();
                    let request = request.clone();
                    let cancellation = self.cancellation.child_token();
                    admissions[index] = Admission::Running {
                        cancellation: cancellation.clone(),
                        stop: None,
                    };
                    let run_dir = run_dir.clone();
                    let parallelism = self.parallelism.clone();
                    let agent = request.profile.kind() == crate::config::ProfileKind::Agent;
                    let stops = stops.clone();
                    self.tasks.spawn(async move {
                        let mut execution = execution;
                        // An Agent result pins the files its tools execute, hashed before the
                        // review starts; a path that cannot be hashed fails the preparation.
                        let prepared = match prepared {
                            Ok(prepared) if agent && !cancellation.is_cancelled() => {
                                match tools::pins::execution_paths(&config, &request.eval_id).await
                                {
                                    Ok(pins) => {
                                        execution.provenance.execution_paths = pins;
                                        match prepared {
                                            execution::Prepared::Agent {
                                                state,
                                                session,
                                                saving,
                                                ..
                                            } => {
                                                match tools::Registry::new(
                                                    &config,
                                                    &request.eval_id,
                                                ) {
                                                    Ok(registry) => match registry
                                                        .start_sessions(
                                                            &state,
                                                            None,
                                                            cancellation.clone(),
                                                        )
                                                        .await
                                                    {
                                                        Ok(tools) => {
                                                            Ok(execution::Prepared::Agent {
                                                                state,
                                                                session,
                                                                saving,
                                                                tools,
                                                            })
                                                        }
                                                        Err(error) => Err(error),
                                                    },
                                                    Err(error) => Err(error),
                                                }
                                            }
                                            prepared => Ok(prepared),
                                        }
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
                            &stops,
                            cancellation,
                            deferred_human,
                        )
                        .await?;
                        Ok((index, request))
                    });
                }
            }
            let evaluation = self
                .graph
                .evaluate_for_run(&evidence, self.run.modes.speculate);
            for request in self.requests.iter_mut() {
                if evaluation.evals[request.eval_id.as_str()].status == EvalStatus::Provisional
                    && let crate::store::RequestState::Completed { result, at } = &request.state
                {
                    request.state = crate::store::RequestState::Provisional {
                        result: result
                            .clone()
                            .try_into()
                            .expect("provisional evidence is GREEN"),
                        at: *at,
                    };
                    self.receipts.save_request(request).await?;
                } else if evaluation.evals[request.eval_id.as_str()].status == EvalStatus::Green
                    && let crate::store::RequestState::Provisional { result, at } = &request.state
                {
                    request.state =
                        crate::store::RequestState::completed(result.clone().into(), *at);
                    self.receipts.save_request(request).await?;
                }
            }
            let cancelled = self.cancellation.is_cancelled();
            if !cancelled && evidence.len() != completed {
                continue;
            }
            let human_wait = self
                .requests
                .iter()
                .any(|request| request.status() == crate::types::RequestStatus::WaitingHuman);
            let admission_wait = admissions.iter().any(Admission::waiting);
            if self.tasks.is_empty() && (!admission_wait || cancelled) && (cancelled || !human_wait)
            {
                break;
            }
            if let Some(control) = &mut self.run.control {
                let activity = if cancelled {
                    crate::runs::Activity::Cancelling
                } else if self.tasks.is_empty() {
                    crate::runs::Activity::Waiting
                } else {
                    crate::runs::Activity::Executing
                };
                if control.activity != activity {
                    control.activity = activity;
                    self.receipts.save_run(self.run).await?;
                }
            }
            let waiting_external = admission_wait || human_wait;
            tokio::select! {
                biased;
                result = self.tasks.join_next(), if !self.tasks.is_empty() => {
                    let (index, mut request) = crate::task::joined(result.expect("active tasks"))
                        .map_err(|e| e.to_string())??;
                    let Admission::Running { stop, .. } = std::mem::replace(&mut admissions[index], Admission::Idle) else {
                        unreachable!("a joined task belongs to a running request");
                    };
                    match stop {
                        Some(modes::Stop::Blocked) => {
                            admissions[index] = Admission::Blocked;
                            let evaluation = self.graph.evaluate_for_run(&evidence, self.run.modes.speculate);
                            let eval = &evaluation.evals[request.eval_id.as_str()];
                            request.state = crate::store::RequestState::pending(crate::types::RequestStatus::Blocked, Some(now())).expect("blocked state");
                            request.blocked_by = eval.blocked_by.iter().copied().map(Into::into).collect();
                            request.blocked_reason = Some(crate::project::verify::gate_reason(eval));
                            request.queue = None;
                            self.receipts.block_request(&request).await?;
                            self.requests[index] = request;
                            continue;
                        }
                        Some(modes::Stop::Yield) => {
                            request.state = crate::store::RequestState::pending(crate::types::RequestStatus::Queued, None).expect("queued state");
                            request.execution_id = None;
                            request.started_at = None;
                            request.blocked_reason = Some("Speculative work yielded to a ready eval.".into());
                            self.receipts.save_request(&request).await?;
                            self.requests[index] = request;
                            continue;
                        }
                        None => {}
                    }
                    if let Some(remote) = &self.remote {
                        remote.publish_request(self.receipts, &request).await?;
                    }
                    let stopped = stops.all();
                    if stopped.len() != self.run.stopped_backends.len() {
                        self.run.stopped_backends = stopped;
                        self.receipts.save_run(self.run).await?;
                    }
                    if !request.status().is_human_wait() {
                        evidence.insert(request.eval_id.clone(), match request.status() {
                            crate::types::RequestStatus::Green => Evidence::Current(Verdict::Green),
                            crate::types::RequestStatus::Red => Evidence::Current(Verdict::Red),
                            _ => Evidence::OperationalError,
                        });
                    }
                    self.requests[index] = request;
                }
                _ = self.cancellation.cancelled(), if !cancelled => {},
                _ = changes.next_state(), if !cancelled => {},
                _ = reconcile.tick(), if waiting_external && !cancelled => {},
                _ = tokio::time::sleep_until(remote_due),
                    if human_wait && self.remote.is_some() && !self.run.force && !cancelled => {},
            }
        }
        Ok(evidence)
    }
}

/// Claim an execution for a request: a forced review takes no claim of its key and joins
/// nothing, while any other first looks up the remote store again (a hit becomes a local
/// record). With `capacity`, starting also takes a slot of the backend.
async fn claim(
    receipts: &Receipts,
    remote: Option<&Session>,
    run_force: bool,
    execution: &Execution,
    request: &Request,
    allow_start: bool,
    capacity: Option<Capacity>,
) -> Result<Claim, String> {
    let keyed = execution.key.is_some() && !request.force;
    if let (Some(remote), Some(key), true, false) = (remote, &execution.key, keyed, run_force) {
        remote.refresh(receipts, vec![key.clone()]).await?;
    }
    receipts
        .claim_execution(
            execution,
            keyed,
            request.execution_id.as_ref(),
            allow_start,
            capacity,
        )
        .await
}
