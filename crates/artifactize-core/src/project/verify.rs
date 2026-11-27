use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};

use tokio_util::sync::CancellationToken;

use crate::{
    broker::{self, now},
    cache,
    config::{ProfileKind, read_workspace_config},
    graph::{EvalStatus, Evidence, Graph},
    project::selection::Selection,
    remote::Session,
    store::{self, ExecutionOptions, Receipts, Request, Run, RunView},
    workspace,
};

#[derive(Debug, Clone, Default)]
pub struct VerifyOptions {
    pub modes: crate::runs::RunModes,
    /// Optional per-Run concurrency cap; absent admits every READY eval.
    pub jobs: Option<usize>,
    /// Fingerprints computed at once, in preparation and end-of-review rechecks; `None` uses
    /// the available CPUs.
    pub fingerprint_jobs: Option<usize>,
    pub max_executions: Option<u64>,
    /// Kinds whose evals only reuse a result, from the cache or the review store; one with
    /// nothing to reuse is not executed, and the Run ends INCOMPLETE.
    pub reuse_only: BTreeSet<ProfileKind>,
    pub recursive: bool,
    /// Force only explicitly selected Evals, never recursive dependencies.
    pub force: bool,
    /// What the requester is working on, shown with the Run; it changes nothing it does.
    pub comment: Option<crate::types::RunComment>,
}

/// Submit to artifactize and observe settlement. Cancelling this future/token only stops watching.
pub async fn verify(
    repo: &Path,
    state_dir: Option<&Path>,
    selection: &Selection,
    options: &VerifyOptions,
    cancellation: CancellationToken,
) -> Result<RunView, String> {
    super::fingerprint_parallelism(options)?;
    let context = crate::runs::Context::discover(repo, state_dir)?;
    let observation = crate::runs::start(&context, selection, options, None).await?;
    crate::runs::wait(&context, &observation.run, None, cancellation).await?;
    crate::runs::view(&context, &observation.run).await
}
/// Validate declarations and reserve initial requests; fingerprints and remote I/O follow admission.
pub(crate) async fn reserve(
    repo: &Path,
    state_dir: Option<&Path>,
    selection: &Selection,
    options: &VerifyOptions,
    request_key: Option<&crate::types::RequestKey>,
    request_args_hash: Option<&str>,
    origin: &crate::runs::Origin,
) -> Result<(crate::types::RunId, bool), String> {
    if options.jobs == Some(0) {
        return Err("jobs must be at least 1.".into());
    }
    let parallelism = super::fingerprint_parallelism(options)?;
    let config = Arc::new(read_workspace_config(repo).map_err(|e| e.to_string())?);
    Graph::new(&config).map_err(|e| e.to_string())?;
    let selected = selection.resolve(&config)?;
    let selected_ids: BTreeSet<_> = selected.evals.iter().map(|eval| &eval.id).collect();
    let evals = selection.included_evals(&config, options.recursive)?;
    let definitions =
        store::definitions::Definitions::from_view(&crate::query::graph(&config, selection)?)
            .map_err(|error| error.to_string())?;
    let state = store::state_dir(state_dir)?;
    let _limits = crate::limits::Limits::read(&state)?;
    let receipts = Receipts::open(&state, &config.root).await?;
    let result = async {
        let runs = workspace::prepare_directory(&state.join("runs"), &config.root)
            .map_err(|e| e.to_string())?;
        let directory =
            crate::platform::private_tempdir_in("run-", &runs).map_err(|e| e.to_string())?;
        let id = directory
            .path()
            .file_name()
            .unwrap()
            .to_str()
            .expect("generated run directory names are ASCII")
            .to_owned();
        let _ = directory.keep();
        let run = Run {
            modes: options.modes.clone(),
            control: Some(crate::runs::Control::starting()?),
            request_key: request_key.cloned(),
            request_args_hash: request_args_hash.map(str::to_owned),
            origin: Some(origin.clone()),
            comment: options.comment.clone(),
            id: id.parse()?,
            repo_path: config.root.clone(),
            repository: crate::repository::identify(&config.root),
            state_dir: state,
            state: crate::store::RunState::Running,
            created_at: now(),
            selection: selection.clone(),
            definitions,
            jobs: options.jobs,
            fingerprint_jobs: Some(parallelism.limit()),
            max_executions: options.max_executions,
            executions_started: 0,
            wait_timeout_ms: None,
            wait_timed_out: false,
            reuse_only: options.reuse_only.clone(),
            recursive: options.recursive,
            force: options.force,
            validation: Default::default(),
            stopped_backends: Vec::new(),
            evidence: BTreeMap::new(),
        };
        let requests: Vec<_> = evals
            .iter()
            .enumerate()
            .map(|(ordinal, eval)| Request {
                state: crate::store::RequestState::pending(
                    crate::types::RequestStatus::Queued,
                    None,
                )
                .expect("queued state"),
                id: format!("{}-{}", run.id, ordinal + 1)
                    .parse()
                    .expect("generated request id is a safe segment"),
                run_id: run.id.clone(),
                eval_id: eval.id.clone(),
                target: eval.target.clone(),
                title: eval.declaration.title.clone(),
                profile: (eval.declaration.profile()).into(),
                requested_profile: (eval.declaration.profile()).into(),
                eval_def_hash: cache::eval_definition_hash(&eval.declaration),
                options: ExecutionOptions {
                    mutex: eval.declaration.mutex.clone(),
                    ..ExecutionOptions::new(eval.declaration.profile())
                },
                execution_id: None,
                provenance: None,
                usage: None,
                reused_usage: None,
                joined: false,
                producer: None,
                reviewer: None,
                origin: None,
                session_id: None,
                session: None,
                review_notes: None,
                human_definition: None,
                payload: eval.declaration.payload().into(),
                references: eval.references.clone(),
                deps: eval.deps.clone(),
                force: options.force && selected_ids.contains(&eval.id),
                fingerprint: None,
                key: None,
                fingerprints: Default::default(),
                created_at: run.created_at,
                started_at: None,
                cwd: config.root.join(config.artifacts[&eval.target].folder()),
                run_dir: None,
                argv: None,
                child: None,
                blocked_reason: None,
                blocked_by: Vec::new(),
            })
            .collect();
        let id = receipts.reserve(&run, &requests).await?;
        let accepted = id == run.id;
        if !accepted {
            let _ = std::fs::remove_dir(run.state_dir.join("runs").join(run.id.as_str()));
        }
        Ok((id, accepted))
    }
    .await;
    let closed = receipts.close().await;
    result.and_then(|reserved| closed.map(|()| reserved))
}
/// Keep the failure kind typed through execution; prefixes belong only to display.
#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("INPUT_CHANGED: {0}")]
    InputChanged(String),
    #[error("{0}")]
    Preparation(String),
}
impl Error {
    pub(crate) fn code(&self) -> crate::types::FailureCode {
        match self {
            Self::InputChanged(_) => crate::types::FailureCode::InputChanged,
            Self::Preparation(_) => crate::types::FailureCode::PreparationFailed,
        }
    }
}
impl From<String> for Error {
    fn from(message: String) -> Self {
        Self::Preparation(message)
    }
}
impl From<&str> for Error {
    fn from(message: &str) -> Self {
        Self::Preparation(message.into())
    }
}

/// Execute exactly the admitted Run. Never reserve a second Run or substitute changed declarations.
pub(crate) async fn execute(
    mut run: Run,
    receipts: &Receipts,
    cancellation: CancellationToken,
) -> Result<(), Error> {
    let selection = &run.selection.clone();
    let options = &VerifyOptions {
        modes: run.modes.clone(),
        jobs: run.jobs,
        fingerprint_jobs: run.fingerprint_jobs,
        max_executions: run.max_executions,
        reuse_only: run.reuse_only.clone(),
        recursive: run.recursive,
        force: run.force,
        comment: run.comment.clone(),
    };
    let repo = &run.repo_path;
    let state_dir = Some(run.state_dir.as_path());
    if options.jobs == Some(0) {
        return Err("jobs must be at least 1.".into());
    }
    let parallelism = super::fingerprint_parallelism(options)?;
    // The profiles read the environment the Run was submitted with, not this process's.
    let saved = run.definitions.environment();
    let config = Arc::new(
        crate::config::read_workspace_declarations_with(repo, &|name| {
            saved.get(name).cloned().flatten()
        })
        .map_err(|e| Error::InputChanged(e.to_string()))?,
    );
    let graph = Graph::new(&config).map_err(|e| Error::InputChanged(e.to_string()))?;
    let selected = selection
        .resolve(&config)
        .map_err(|e| Error::InputChanged(e.to_string()))?;
    let required: BTreeSet<_> = graph
        .dependency_closure(&selected.roots)
        .map_err(|e| e.to_string())?
        .into_iter()
        .collect();
    let evals = selection.included_evals(&config, options.recursive)?;
    let definitions =
        store::definitions::Definitions::from_view(&crate::query::graph(&config, selection)?)
            .map_err(|error| error.to_string())?;
    let state = store::state_dir(state_dir)?;
    let limits = crate::limits::Limits::read(&state)?;
    let runs = workspace::prepare_directory(&state.join("runs"), &config.root)
        .map_err(|e| e.to_string())?;
    if serde_json::to_string(&definitions).map_err(|e| e.to_string())?
        != serde_json::to_string(&run.definitions).map_err(|e| e.to_string())?
    {
        return Err(Error::InputChanged(
            "declarations changed after acceptance.".into(),
        ));
    }
    let fingerprints = cache::prepare(
        &config,
        cache::fingerprint_targets(&config, &required)
            .into_iter()
            .filter(|name| {
                crate::scope::validate_file_target(&config.root, &config.artifacts[*name]).is_ok()
            }),
        &runs,
        &parallelism,
        cancellation.clone(),
    )
    .await?;
    if cancellation.is_cancelled() {
        return Err("Project preparation was cancelled.".into());
    }
    let keys = cache::eval_keys(&config, &fingerprints);
    // The store is asked once for every key, and a record that completed after the local
    // latest is mirrored into the local history before the Run claims anything. A forced Run
    // reads nothing from the store, but publishes its results like any other.
    let remote = Session::open(Some(&state), Some(&config.root))?;
    // Results from a fake provider must never reach the shared review store.
    if remote.is_some()
        && let Some(variable) = crate::llm::active_test_endpoint()
    {
        return Err(format!(
            "{variable} points Agent reviews at a local test endpoint; set ARTIFACTIZE_REMOTE=off so their results stay out of the review store."
        ).into());
    }
    if let Some(remote) = &remote
        && !options.force
    {
        remote
            .refresh(
                receipts,
                keys.values().map(|key| key.value.clone()).collect(),
            )
            .await?;
    }
    unchanged_declarations(repo, selection, &run.definitions)?;
    let mut requests = store::read_run_requests_only(&state, &run.id).await?;
    for request in &mut requests {
        request.fingerprint = fingerprints
            .get(&request.target)
            .map(|fingerprint| fingerprint.value.clone());
        request.key = keys.get(&request.eval_id).map(|key| key.value.clone());
        let eval = config
            .evals
            .iter()
            .find(|eval| eval.id == request.eval_id)
            .ok_or_else(|| {
                format!(
                    "Saved request {} in Run {} refers to Eval {}, which is missing from the workspace configuration.",
                    request.id, request.run_id, request.eval_id
                )
            })?;
        request.fingerprints = cache::dependencies(&config, eval)
            .into_iter()
            .filter_map(|name| {
                fingerprints
                    .get(name)
                    .map(|fingerprint| (name.clone(), fingerprint.value.clone()))
            })
            .collect();
        receipts.save_request(request).await?;
    }
    run.control.as_mut().expect("owned Run").activity = crate::runs::Activity::Executing;
    receipts.save_run(&run).await?;
    let mut evidence = match broker::schedule(
        config.clone(),
        &graph,
        &fingerprints,
        &keys,
        &parallelism,
        &limits,
        &mut run,
        &mut requests,
        receipts,
        remote.clone(),
        cancellation.clone(),
    )
    .await
    {
        Ok(evidence) => evidence,
        Err(error) => {
            return Err(error.into());
        }
    };
    let evaluation = graph.evaluate_for_run(&evidence, run.modes.speculate);
    // Freeze cancellation once at settlement so the Run and its requests agree.
    let cancelled = cancellation.is_cancelled();
    let at = now();
    for request in &mut requests {
        // A cancelled Run withdraws its waiting sign-offs too, deferred ones included.
        let withdrawn = request.status().is_human_wait() && cancelled;
        if !withdrawn
            && (evidence.contains_key(request.eval_id.as_str()) || request.status().is_human_wait())
        {
            continue;
        }
        let eval = &evaluation.evals[request.eval_id.as_str()];
        if request.status() == crate::types::RequestStatus::Blocked && !cancelled {
            // A block saved mid-Run named the causes known then; the settled Run names all.
            if eval.status == EvalStatus::Blocked {
                request.blocked_by = eval.blocked_by.iter().copied().map(Into::into).collect();
                request.blocked_reason = Some(gate_reason(eval));
            }
            continue;
        }
        if cancelled {
            request.state = crate::store::RequestState::failed(
                "Run was cancelled.".into(),
                Some(crate::types::FailureCode::Cancelled),
                at,
            );
            evidence.insert(request.eval_id.clone(), Evidence::OperationalError);
        } else if request.status() == crate::types::RequestStatus::BudgetExhausted
            && eval.can_execute()
        {
            continue;
        } else {
            request.state =
                crate::store::RequestState::pending(status(eval.status), request.completed_at())
                    .expect("non-executed request state");
            request.blocked_reason = Some(gate_reason(eval));
        }
    }
    let evaluation = graph.evaluate_for_run(&evidence, run.modes.speculate);
    let required_evals: Vec<_> = evaluation
        .evals
        .iter()
        .filter(|(id, _)| required.contains(&graph.eval_target(id).unwrap()))
        .collect();
    let satisfied = required
        .iter()
        .all(|id| evaluation.artifacts[id.as_str()].satisfied());
    let budget_exhausted = requests
        .iter()
        .any(|request| request.status() == crate::types::RequestStatus::BudgetExhausted);
    // Only a --reuse-only eval that had nothing to reuse leaves stale evidence in a Run.
    let not_reused = evidence
        .values()
        .filter(|evidence| matches!(evidence, Evidence::Stale))
        .count();
    run.state = terminal_state(
        TerminalFacts {
            required_evals: &required_evals,
            satisfied,
            required_have_review_basis: required.iter().all(|id| {
                // A parent without evals of its own is reviewed through its parts.
                let artifact = &graph.artifacts()[id.as_str()];
                artifact.basis || !artifact.evals.is_empty() || !artifact.parts.is_empty()
            }),
            budget_error: budget_exhausted.then(|| broker::budget_reason(&run)),
            not_reused,
        },
        at,
        cancelled,
    );
    if cancelled {
        run.control.as_mut().expect("owned Run").failure_code =
            Some(crate::types::FailureCode::Cancelled);
    }
    use crate::config::Field;
    run.validation = store::Validation(Some(store::ValidationSnapshot {
        selection: Field::Value(store::definitions::SavedSelection {
            known: run.selection.clone(),
            extra: Default::default(),
        }),
        recursive: Field::Value(run.recursive),
        force: Field::Value(run.force),
        selected_eval_ids: Field::Value(
            selected.evals.iter().map(|eval| eval.id.clone()).collect(),
        ),
        included_eval_ids: Field::Value(evals.iter().map(|eval| eval.id.clone()).collect()),
        satisfied: Field::Value(satisfied && !cancelled),
        obligations: Field::Value(
            evaluation
                .obligations
                .iter()
                .filter(|id| required.contains(*id))
                .map(|id| (*id).clone())
                .collect(),
        ),
        artifacts: Field::Value(
            required
                .iter()
                .map(|id| {
                    let artifact = &evaluation.artifacts[id.as_str()];
                    let (fingerprint_kind, fingerprint) = fingerprints.get(id).map_or(
                        (Field::Missing, Field::Missing),
                        |fingerprint| {
                            (
                                config.artifacts[*id]
                                    .fingerprint
                                    .as_ref()
                                    .map(crate::config::Fingerprint::kind)
                                    .into(),
                                Field::Value(fingerprint.value.clone()),
                            )
                        },
                    );
                    store::ArtifactValidation {
                        id: (*id).clone(),
                        status: Field::Value(artifact.status.into()),
                        passed: Field::Value(artifact.passed as u64),
                        total: Field::Value(artifact.total as u64),
                        satisfied: Field::Value(artifact.satisfied()),
                        fingerprint_kind,
                        fingerprint,
                        extra: Default::default(),
                    }
                })
                .collect(),
        ),
        evals: Field::Value(
            required_evals
                .iter()
                .map(|(id, eval)| store::EvalValidation {
                    id: (*id).clone(),
                    status: Field::Value(status(eval.status)),
                    blocked_by: Field::Value(
                        eval.blocked_by.iter().copied().map(Into::into).collect(),
                    ),
                    extra: Default::default(),
                })
                .collect(),
        ),
        extra: Default::default(),
    }));
    if cancelled {
        receipts
            .retire_worker(&run.id, crate::types::FailureCode::Cancelled)
            .await?;
    }
    if run.modes.speculate
        && let Some(remote) = remote
    {
        // A blocked result cannot be retracted from a team store. Publish survivors only
        // after scheduling/withdrawal finishes, while the owned Run still owns failures.
        run.control.as_mut().expect("owned Run").activity = crate::runs::Activity::Publishing;
        let mut publishing = run.clone();
        publishing.state = crate::store::RunState::Running;
        receipts.save_run(&publishing).await?;
        for request in &requests {
            remote.publish_request(receipts, request).await?;
        }
    }
    // The reuse history is collected once per Run, before it ends so that whoever waits for
    // the end also sees a failure, which only warns.
    if let Err(error) = store::history::collect(&run.state_dir).await {
        use std::io::Write;
        let _ = writeln!(std::io::stderr().lock(), "Cache GC failed: {error}");
    }
    receipts.finish(&run, &requests).await?;
    // Saved Agent conversations past their size bound are collected; a failure only warns.
    let state = run.state_dir.clone();
    let bounds = limits.agent_sessions();
    let collected = crate::task::joined(
        tokio::task::spawn_blocking(move || crate::agent::session::collect(&state, bounds, false))
            .await,
    )
    .map_err(|e| e.to_string())
    .and_then(|collected| collected);
    if let Err(error) = collected {
        use std::io::Write;
        let _ = writeln!(
            std::io::stderr().lock(),
            "Agent session collection failed: {error}"
        );
    }
    Ok(())
}

struct TerminalFacts<'a, 'graph> {
    required_evals: &'a [(
        &'a crate::types::EvalId,
        &'a crate::graph::EvalEvaluation<'graph>,
    )],
    satisfied: bool,
    required_have_review_basis: bool,
    budget_error: Option<String>,
    not_reused: usize,
}

fn terminal_state(
    facts: TerminalFacts<'_, '_>,
    at: crate::types::Timestamp,
    cancelled: bool,
) -> store::RunState {
    use store::RunState;
    let TerminalFacts {
        required_evals,
        satisfied,
        required_have_review_basis,
        budget_error,
        not_reused,
    } = facts;
    if cancelled {
        RunState::Error {
            at,
            error: Some("Run was cancelled.".into()),
        }
    } else if let Some(error) = budget_error {
        RunState::Incomplete {
            at,
            error: Some(error),
        }
    } else if not_reused > 0 {
        RunState::Incomplete {
            at,
            error: Some(format!(
                "{not_reused} eval{} had no result to reuse and {} not executed (--reuse-only).",
                if not_reused == 1 { "" } else { "s" },
                if not_reused == 1 { "was" } else { "were" }
            )),
        }
    } else if required_evals
        .iter()
        .any(|(_, c)| c.status == EvalStatus::Error)
    {
        RunState::Error { at, error: None }
    } else if required_evals
        .iter()
        .any(|(_, c)| c.status == EvalStatus::Red)
    {
        RunState::Red { at }
    } else if required_evals
        .iter()
        .any(|(_, eval)| eval.status == EvalStatus::Deferred)
        && required_evals
            .iter()
            .all(|(_, eval)| eval.status.passes_gate())
        && required_have_review_basis
    {
        RunState::Yellow { at }
    } else if satisfied {
        RunState::Green { at }
    } else {
        RunState::Incomplete { at, error: None }
    }
}

pub(crate) fn status(status: EvalStatus) -> crate::types::RequestStatus {
    match status {
        EvalStatus::Deferred => crate::types::RequestStatus::Deferred,
        EvalStatus::Provisional => crate::types::RequestStatus::Provisional,
        EvalStatus::Green => crate::types::RequestStatus::Green,
        EvalStatus::Red => crate::types::RequestStatus::Red,
        EvalStatus::Error => crate::types::RequestStatus::Error,
        EvalStatus::Stale => crate::types::RequestStatus::Stale,
        EvalStatus::Unreviewed => crate::types::RequestStatus::Unreviewed,
        EvalStatus::Wait => crate::types::RequestStatus::WaitDependency,
        EvalStatus::Blocked => crate::types::RequestStatus::Blocked,
    }
}

/// The same gate explanation is saved during scheduling and at Run settlement.
pub(crate) fn gate_reason(eval: &crate::graph::EvalEvaluation<'_>) -> String {
    eval.blocked_reason().unwrap_or_else(|| {
        format!(
            "Waiting for current GREEN dependency evidence: {}",
            eval.unmet_gates
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )
    })
}

/// Fingerprints and remote I/O can take time; check the accepted declarations again at the
/// execution boundary instead of silently applying a different live selection or profile.
fn unchanged_declarations(
    repo: &Path,
    selection: &Selection,
    saved: &store::definitions::Definitions,
) -> Result<(), Error> {
    let check = || -> Result<(), String> {
        let environment = saved.environment();
        let config = crate::config::read_workspace_declarations_with(repo, &|name| {
            environment.get(name).cloned().flatten()
        })
        .map_err(|e| e.to_string())?;
        let current =
            store::definitions::Definitions::from_view(&crate::query::graph(&config, selection)?)
                .map_err(|e| e.to_string())?;
        if serde_json::to_string(&current).map_err(|e| e.to_string())?
            != serde_json::to_string(saved).map_err(|e| e.to_string())?
        {
            return Err("declarations changed after acceptance.".into());
        }
        Ok(())
    };
    check().map_err(Error::InputChanged)
}

#[cfg(test)]
mod tests {
    use super::Error;
    use crate::types::FailureCode;

    #[test]
    fn failure_identity_comes_from_the_variant_not_owner_diagnostic_text() {
        let preparation = Error::Preparation("INPUT_CHANGED: text from a command".into());
        assert_eq!(preparation.code(), FailureCode::PreparationFailed);
        let changed = Error::InputChanged("declarations changed after acceptance.".into());
        assert_eq!(changed.code(), FailureCode::InputChanged);
        assert_eq!(
            changed.to_string(),
            "INPUT_CHANGED: declarations changed after acceptance."
        );
    }
}
