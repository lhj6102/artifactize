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
    cache,
    config::{Profile, RepoConfig},
    graph::{Evidence, Graph},
    process,
    remote::Session,
    runtime::Verdict,
    store::{Claim, Execution, Producer, Provenance, Receipts, Request, Run},
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
        let run_dir = self.run.state_dir.join("runs").join(&self.run.id);
        let mut evidence = BTreeMap::new();
        let mut running = BTreeSet::new();
        let mut waiting = BTreeSet::new();
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
                    let allow_start = human
                        || self
                            .run
                            .max_executions
                            .is_none_or(|limit| self.run.executions_started < limit);
                    // A forced review neither reuses nor joins a live execution of its key; it
                    // adds a newer record when it completes.
                    let claim = if request.force {
                        if allow_start {
                            Claim::Owned
                        } else {
                            Claim::BudgetExhausted
                        }
                    } else {
                        // Look up the remote again just before claiming; a hit becomes a local
                        // record.
                        if let (Some(remote), Some(key), false) =
                            (&self.remote, &execution.key, self.run.force)
                        {
                            remote.refresh(self.receipts, vec![key.clone()]).await?;
                        }
                        self.receipts
                            .claim_execution(
                                &execution,
                                request.execution_id.as_deref(),
                                allow_start,
                            )
                            .await?
                    };
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
                    running.insert(index);
                    self.tasks.spawn(async move {
                        let request = execution::execute(
                            config,
                            receipts.clone(),
                            request,
                            execution,
                            prepared,
                            run_dir,
                            cancellation,
                        )
                        .await?;
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
            if self.tasks.is_empty() && (waiting.is_empty() || cancelled) {
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
                _ = tokio::time::sleep(poll), if (!waiting.is_empty() || human_wait) && !cancelled => {},
            }
        }
        Ok(evidence)
    }
}
