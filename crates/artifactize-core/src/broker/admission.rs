//! Scheduling policy within a pass: block, admit and start requests with the available capacity.
use super::*;

pub(super) struct Pass<'a, 'cfg> {
    pub declarations: &'a BTreeMap<&'cfg crate::config::EvalId, &'cfg crate::config::Eval>,
    pub deferred: &'a BTreeSet<crate::config::EvalId>,
    pub owner: process::ChildIdentity,
    pub producer: &'a Producer,
    pub run_dir: &'a std::path::Path,
    pub saving: Option<&'a crate::agent::session::Saving>,
    pub stops: &'a Stops,
}

impl<'a> Scheduler<'a, '_> {
    pub(super) async fn admit(
        &mut self,
        evaluated: &mut evaluated::Evaluated<'a>,
        admissions: &mut [Admission],
        evidence: &mut BTreeMap<crate::config::EvalId, Evidence>,
        Pass {
            declarations,
            deferred,
            owner,
            producer,
            run_dir,
            saving,
            stops,
        }: Pass<'_, '_>,
    ) -> Result<(), String> {
        let mut evaluation = evaluated.evaluation(self.graph, evidence, self.run.modes.speculate);
        // Failures cancel transitively, including tasks, joined waits and sign-offs.
        // Without speculation a cached result is taken before its gates resolve, even
        // behind RED, so only work this Run started past a pending sign-off is cancelled.
        for (request, admission) in self.requests.iter_mut().zip(admissions.iter_mut()) {
            let eval = &evaluation.evals[request.eval_id.as_str()];
            if eval.readiness == crate::graph::Readiness::Blocked
                && !admission.blocked()
                && (self.run.modes.speculate || !crate::query::reused(request))
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
                    request.blocked_by = eval.blocked_by.iter().copied().map(Into::into).collect();
                    request.blocked_reason = Some(crate::project::verify::gate_reason(eval));
                    request.queue = None;
                    self.receipts.block_request(request).await?;
                }
            }
        }
        evaluation = evaluated.evaluation(self.graph, evidence, self.run.modes.speculate);
        let order = evaluated.order(self.graph, self.requests, evidence);
        // Backends found full in this pass are not asked again until the next one.
        let mut full = BTreeSet::new();
        // Request order preserves selection order, then recursive configuration order.
        for &index in order.iter() {
            if self.cancellation.is_cancelled() {
                break;
            }
            let current = &self.requests[index];
            let eval = &evaluation.evals[current.eval_id.as_str()];
            let ready = eval.can_execute()
                || (deferred.contains(&current.eval_id)
                    && eval.readiness == crate::graph::Readiness::Ready);
            let can_start = current.profile.kind() == ProfileKind::Human
                || self
                    .run
                    .max_executions
                    .is_none_or(|limit| self.run.executions_started < limit);
            if ready
                && can_start
                && !current.status().is_human_wait()
                && !superseded(current)
                && !admissions[index].running()
                && !admissions[index].blocked()
            {
                // Ready work owns capacity; speculative tasks yield before any ready task.
                let backend = current.options.backend.as_ref();
                let jobs_full = !matches!(admissions[index], Admission::Joining)
                    && self.run.jobs.is_some_and(|limit| {
                        admissions.iter().filter(|state| state.uses_job()).count() >= limit
                    });
                let backend_full = backend.is_some_and(|backend| {
                    matches!(admissions[index], Admission::Capacity)
                        && admissions.iter().enumerate().any(|(active, state)| {
                            state.running()
                                && self.requests[active].options.backend.as_ref() == Some(backend)
                        })
                });
                if jobs_full || backend_full {
                    if let Some(active) =
                        admissions
                            .iter()
                            .zip(self.requests.iter())
                            .rposition(|(state, request)| {
                                matches!(state, Admission::Running { stop: None, .. })
                                    && evaluation.evals[request.eval_id.as_str()].readiness
                                        == crate::graph::Readiness::Wait
                                    && (!backend_full
                                        || request.options.backend.as_ref() == backend)
                            })
                    {
                        admissions[active].stop(modes::Stop::Yield);
                    }
                    if let Some(active) =
                        admissions
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
                || superseded(request)
                || matches!(evidence.get(&request.eval_id), Some(value) if *value != Evidence::Deferred)
            {
                continue;
            }
            let eval = &evaluation.evals[request.eval_id.as_str()];
            // People are never asked before the machine evals of their Artifact pass.
            let siblings_pending = request.profile.kind() == ProfileKind::Human
                && eval
                    .unmet_gates
                    .iter()
                    .any(|gate| self.graph.eval_target(gate) == Some(&request.target));
            let speculative = self.run.modes.speculate
                && eval.readiness == crate::graph::Readiness::Wait
                && !siblings_pending
                && matches!(eval.evidence, None | Some(Evidence::Stale));
            let deferred_human = deferred.contains(&request.eval_id);
            let may_execute = eval.can_execute()
                || speculative
                || (deferred_human && eval.readiness == crate::graph::Readiness::Ready);
            if !may_execute {
                admissions[index] = Admission::Idle;
                let status = crate::project::verify::status(eval.status);
                let blocked_by: Vec<_> = eval.blocked_by.iter().copied().map(Into::into).collect();
                if request.status() != status || request.blocked_by != blocked_by {
                    request.state =
                        crate::store::RequestState::pending(status, request.completed_at())
                            .expect("gated request");
                    request.blocked_by = blocked_by;
                    request.queue = None;
                    request.blocked_reason = Some(crate::project::verify::gate_reason(eval));
                    self.receipts.save_request(request).await?;
                }
                continue;
            }
            // A sign-off is asked for only once its gates are met, never speculatively.
            if request.profile.kind() == ProfileKind::Human
                && eval.readiness != crate::graph::Readiness::Ready
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
            let declared = declarations[&request.eval_id];
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
                evaluation = evaluated.evaluation(self.graph, evidence, self.run.modes.speculate);
                continue;
            }
            let mut execution = Execution {
                state: crate::store::ExecutionState::Running,
                id: format!("execution-{}-{}", request.id, crate::agent::uuid()?).parse()?,
                key: request.key.clone(),
                fingerprint: request.fingerprint.clone(),
                fingerprints: request.fingerprints.clone(),
                artifact_kinds: request
                    .fingerprints
                    .keys()
                    .map(|name| (name.clone(), self.config.artifacts[name].kind()))
                    .collect(),
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
            let kind = declarations[&request.eval_id].declaration.profile().kind();
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
                    .and_then(|name| Some((name.clone(), self.limits.limit(backend)?))),
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
                    let reason =
                        format!("Waiting for mutex tags held by eval {eval_id} in Run {run_id}.");
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
                    full.insert(backend.clone());
                    let newly_waiting = !matches!(admissions[index], Admission::Capacity);
                    admissions[index] = Admission::Capacity;
                    if newly_waiting {
                        request.queue = Some(crate::store::QueueReason::Slot {
                            backend: backend.clone(),
                        });
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
                        execution
                            .verdict()
                            .map_or(Evidence::OperationalError, Evidence::Current),
                    );
                    cache::reuse(request, &execution, now());
                    self.receipts.reuse_execution(request).await?;
                    evaluation =
                        evaluated.evaluation(self.graph, evidence, self.run.modes.speculate);
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
                    evaluation =
                        evaluated.evaluation(self.graph, evidence, self.run.modes.speculate);
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
                    request.blocked_reason =
                        Some("Waiting for the active Human execution of this reuse key.".into());
                    *request = self.receipts.follow_human(request).await?;
                    if !request.status().is_human_wait() {
                        evidence.insert(request.eval_id.clone(), settled(request));
                        evaluation =
                            evaluated.evaluation(self.graph, evidence, self.run.modes.speculate);
                    }
                    continue;
                }
                Claim::Wait(id) => {
                    admissions[index] = Admission::Joining;
                    if request.execution_id.as_ref() != Some(&id) {
                        request.execution_id = Some(id.clone());
                        request.state = crate::store::RequestState::pending(
                            crate::types::RequestStatus::Queued,
                            request.completed_at(),
                        )
                        .expect("pending request state");
                        request.queue = None;
                        request.blocked_reason = Some(format!(
                            "Waiting for execution {id} with the same reuse key and profile."
                        ));
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
                    evaluation =
                        evaluated.evaluation(self.graph, evidence, self.run.modes.speculate);
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
            let eval = declarations[&request.eval_id];
            let prepared = execution::prepare(
                &self.config,
                eval,
                run_dir,
                &self.run.state_dir,
                saving,
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
            let run_dir = run_dir.to_path_buf();
            let parallelism = self.parallelism.clone();
            let agent = request.profile.kind() == crate::config::ProfileKind::Agent;
            let stops = stops.clone();
            self.tasks.spawn(async move {
                let mut execution = execution;
                // An Agent result pins the files its tools execute, hashed before the
                // review starts; a path that cannot be hashed fails the preparation.
                let prepared = match prepared {
                    Ok(prepared) if agent && !cancellation.is_cancelled() => {
                        match tools::pins::execution_paths(&config, &request.eval_id).await {
                            Ok(pins) => {
                                execution.provenance.execution_paths = pins;
                                match prepared {
                                    execution::Prepared::Agent {
                                        state,
                                        session,
                                        saving,
                                        ..
                                    } => match tools::Registry::new(&config, &request.eval_id) {
                                        Ok(registry) => Ok(execution::Prepared::Agent {
                                            state: state.clone(),
                                            session,
                                            saving,
                                            tools: registry.sessions(
                                                &state,
                                                None,
                                                cancellation.clone(),
                                            ),
                                        }),
                                        Err(error) => Err(error),
                                    },
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
        Ok(())
    }
}
