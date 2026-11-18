//! Run lifecycle, scheduling, cancellation, and budget counters.

mod admission;
mod evaluated;
mod execution;
mod modes;
mod settlement;

use settlement::settled;

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::{
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
        let backend = request.options.backend.as_ref()?;
        let stopped = self.0.lock().unwrap();
        stopped
            .iter()
            .find(|stop| &stop.backend == backend)
            .cloned()
    }

    /// Whether `backend` stopped, asked later.
    fn check(&self, backend: &crate::config::Backend) -> Box<dyn FnOnce() -> bool + Send> {
        let (stops, backend) = (self.clone(), backend.clone());
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
        let Ok(error_code) = crate::types::BackendStopCode::try_from(*code) else {
            return;
        };
        let mut stopped = self.0.lock().unwrap();
        if !stopped.iter().any(|stop| &stop.backend == backend) {
            stopped.push(StoppedBackend {
                backend: backend.clone(),
                error_code,
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
        let deferred = modes::deferred(self.run, self.requests);
        // Nothing but unclaimed sign-offs has been left since then; `None` while anything else
        // runs or waits, or a reviewer holds a claim.
        let mut idle_since: Option<tokio::time::Instant> = None;
        let grace = self.limits.signoff_idle_grace();
        let mut admissions: Vec<_> = self.requests.iter().map(|_| Admission::Idle).collect();
        let mut evaluated = evaluated::Evaluated::default();
        // A pass looks evals and requests up by id; the Run's are fixed, so index them once
        // rather than searching the lists for each of them on every pass.
        let config = self.config.clone();
        let declarations: BTreeMap<&crate::config::EvalId, &crate::config::Eval> =
            config.evals.iter().map(|eval| (&eval.id, eval)).collect();
        let positions: BTreeMap<crate::config::EvalId, usize> = self
            .requests
            .iter()
            .enumerate()
            .map(|(index, request)| (request.eval_id.clone(), index))
            .collect();
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
                        evidence.insert(request.eval_id.clone(), settled(&request));
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
                    let index = positions.get(&eval.id).copied();
                    if index.is_some_and(|index| {
                        admissions[index].blocked()
                            || admissions[index].running()
                            || matches!(admissions[index], Admission::Joining)
                            || self.requests[index].status().is_human_wait()
                            || superseded(&self.requests[index])
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
                        let verdict = execution.verdict().ok_or_else(|| {
                            format!("Cached execution {} has no verdict.", execution.id)
                        })?;
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
                self.admit(
                    &mut evaluated,
                    &mut admissions,
                    &mut evidence,
                    admission::Pass {
                        declarations: &declarations,
                        deferred: &deferred,
                        owner,
                        producer: &producer,
                        run_dir: &run_dir,
                        saving: saving.as_ref(),
                        stops: &stops,
                    },
                )
                .await?;
            }
            self.settle_provisional(&mut evaluated, &evidence).await?;
            let cancelled = self.cancellation.is_cancelled();
            if !cancelled && evidence.len() != completed {
                continue;
            }
            let signoffs: Vec<_> = self
                .requests
                .iter()
                .filter(|request| request.status().is_human_wait())
                .map(|request| request.id.clone())
                .collect();
            let human_wait = !signoffs.is_empty();
            let admission_wait = admissions.iter().any(Admission::waiting);
            let idle = self.tasks.is_empty() && (!admission_wait || cancelled);
            if idle && (cancelled || !human_wait) {
                break;
            }
            // Only sign-offs are left: the Run stays open while one is claimed, and ends once
            // none has been for its idle grace. One submitted since this loop last looked is
            // taken on the next turn, never ended over.
            let grace_due = if idle && !self.receipts.signoff_attended(signoffs).await? {
                let due = *idle_since.get_or_insert_with(tokio::time::Instant::now) + grace;
                if tokio::time::Instant::now() >= due {
                    break;
                }
                Some(due)
            } else {
                idle_since = None;
                None
            };
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
            let joined = tokio::select! {
                biased;
                result = self.tasks.join_next(), if !self.tasks.is_empty() => result,
                _ = self.cancellation.cancelled(), if !cancelled => None,
                _ = changes.next_state(), if !cancelled => None,
                _ = reconcile.tick(), if waiting_external && !cancelled => None,
                _ = tokio::time::sleep_until(grace_due.unwrap_or_else(tokio::time::Instant::now)),
                    if grace_due.is_some() && !cancelled => None,
                _ = tokio::time::sleep_until(remote_due),
                    if human_wait && self.remote.is_some() && !self.run.force && !cancelled => None,
            };
            // Every task that has finished is settled before the next pass, not one per pass:
            // a pass evaluates the whole graph, so settling one completion per pass made a
            // large Run's tail as long as its number of completions times a pass.
            let mut next = joined;
            while let Some(result) = next {
                self.settle(result, &mut admissions, &mut evidence, &stops)
                    .await?;
                next = self.tasks.try_join_next();
            }
        }
        Ok(evidence)
    }
}

/// Whether a newer request for this sign-off's eval in the same working tree took it over:
/// that request asks for it now, so this Run never asks again.
fn superseded(request: &Request) -> bool {
    request.error_code() == Some(crate::types::FailureCode::Superseded)
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
