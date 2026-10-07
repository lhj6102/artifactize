use std::{collections::BTreeSet, path::Path, sync::Arc, time::Duration};

use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::{
    broker::{self, now},
    cache,
    config::{DependencyGates, ProfileKind, read_workspace_config},
    graph::{EvalStatus, Evidence, Graph},
    project::selection::{ProfileSelection, Selection, select_profiles},
    remote::Session,
    store::{self, ExecutionOptions, Receipts, Request, Run, RunView},
    workspace,
};

/// How long a Run waits for Human results unless told otherwise.
pub const DEFAULT_HUMAN_WAIT: Duration = Duration::from_secs(600);

#[derive(Debug, Clone)]
pub struct VerifyOptions {
    pub jobs: usize,
    /// Fingerprints computed at once, in preparation and end-of-review rechecks; `None` uses
    /// the available CPUs.
    pub fingerprint_jobs: Option<usize>,
    /// Print `Run: RUN_ID` to stderr as soon as the Run is saved, before any eval runs.
    pub announce_run: bool,
    pub max_executions: Option<u64>,
    /// How long this Run waits for Human submissions; the timeout never cancels a review.
    pub wait_timeout: Duration,
    /// Kinds whose evals only reuse a result, from the cache or the review store; one with
    /// nothing to reuse is not executed, and the Run ends INCOMPLETE.
    pub reuse_only: BTreeSet<ProfileKind>,
    pub profile: Option<ProfileSelection>,
    pub recursive: bool,
    /// Force only explicitly selected Evals, never recursive dependencies.
    pub force: bool,
    /// None uses root reviewPolicy; Some(false) explicitly enforces GREEN gates.
    pub ignore_gates: Option<bool>,
}

impl Default for VerifyOptions {
    fn default() -> Self {
        Self {
            jobs: 4,
            fingerprint_jobs: None,
            announce_run: false,
            max_executions: None,
            wait_timeout: DEFAULT_HUMAN_WAIT,
            reuse_only: BTreeSet::new(),
            profile: None,
            recursive: false,
            force: false,
            ignore_gates: None,
        }
    }
}

/// Executes READY evals in the foreground, reusing completed results by fingerprint.
pub async fn verify(
    repo: &Path,
    state_dir: Option<&Path>,
    selection: &Selection,
    options: &VerifyOptions,
    cancellation: CancellationToken,
) -> Result<RunView, String> {
    if options.jobs == 0 {
        return Err("jobs must be at least 1.".into());
    }
    let parallelism = super::fingerprint_parallelism(options)?;
    let wait_timeout_ms = u32::try_from(options.wait_timeout.as_millis())
        .ok()
        .filter(|ms| (1..=2_147_483_647).contains(ms))
        .ok_or("wait timeout must be between 1 and 2147483647 ms.")?;
    let config = read_workspace_config(repo).map_err(|e| e.to_string())?;
    let config = Arc::new(select_profiles(
        config,
        selection,
        options.profile.as_ref(),
        options.recursive,
    )?);
    let ignore_gates = options.ignore_gates.unwrap_or_else(|| {
        config
            .artifacts
            .values()
            .find(|artifact| artifact.path.as_os_str().is_empty())
            .and_then(|artifact| artifact.review_policy.as_ref())
            .is_some_and(|policy| matches!(policy.dependency_gates, Some(DependencyGates::Ignore)))
    });
    let graph = Graph::new(&config).map_err(|e| e.to_string())?;
    let selected = selection.resolve(&config)?;
    let selected_ids: BTreeSet<_> = selected.evals.iter().map(|eval| eval.id.as_str()).collect();
    let required: BTreeSet<_> = graph
        .dependency_closure(&selected.roots)
        .map_err(|e| e.to_string())?
        .into_iter()
        .collect();
    let evals = selection.included_evals(&config, options.recursive)?;
    let definitions = serde_json::to_value(crate::query::graph(&config, selection)?)
        .map_err(|error| error.to_string())?;
    let state = store::state_dir(state_dir)?;
    let limits = crate::limits::Limits::read(&state)?;
    let receipts = Receipts::open(&state, &config.root).await?;
    let runs = workspace::prepare_directory(&state.join("runs"), &config.root)
        .map_err(|e| e.to_string())?;
    let fingerprints = cache::prepare(
        &config,
        required.iter().copied(),
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
        ));
    }
    if let Some(remote) = &remote
        && !options.force
    {
        remote
            .refresh(
                &receipts,
                keys.values().map(|key| key.value.clone()).collect(),
            )
            .await?;
    }
    let directory = tempfile::Builder::new()
        .prefix("run-")
        .tempdir_in(runs)
        .map_err(|e| e.to_string())?;
    let id = directory
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let _ = directory.keep();
    let mut run = Run {
        id: id.parse()?,
        repo_path: config.root.clone(),
        state_dir: state,
        status: crate::types::RunStatus::Running,
        created_at: now(),
        completed_at: None,
        selection: selection.clone(),
        profile: options.profile.clone(),
        definitions,
        jobs: options.jobs,
        fingerprint_jobs: Some(parallelism.limit()),
        max_executions: options.max_executions,
        executions_started: 0,
        wait_timeout_ms: Some(wait_timeout_ms),
        wait_timed_out: false,
        reuse_only: options.reuse_only.clone(),
        recursive: options.recursive,
        force: options.force,
        ignore_gates,
        validation: Value::Null,
        error: None,
        stopped_backends: Vec::new(),
    };
    let mut requests: Vec<_> = evals
        .iter()
        .enumerate()
        .map(|(ordinal, eval)| Request {
            id: format!("{}-{}", run.id, ordinal + 1)
                .parse()
                .expect("generated request id is a safe segment"),
            run_id: run.id.clone(),
            eval_id: eval.id.clone(),
            target: eval.target.clone(),
            title: eval.declaration.title.clone(),
            profile: (&eval.declaration.profile).into(),
            requested_profile: (&eval.declaration.profile).into(),
            eval_def_hash: cache::eval_definition_hash(&eval.declaration),
            options: ExecutionOptions::new(&eval.declaration.profile, eval.variant.as_deref()),
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
            human_definition: None,
            payload: (&eval.declaration.payload).into(),
            references: json!(eval.references),
            deps: eval.deps.clone(),
            force: options.force && selected_ids.contains(eval.id.as_str()),
            fingerprint: fingerprints
                .get(eval.target.as_str())
                .map(|fingerprint| fingerprint.value.clone()),
            key: keys.get(eval.id.as_str()).map(|key| key.value.clone()),
            fingerprints: keys
                .get(eval.id.as_str())
                .map(|key| key.fingerprints.clone())
                .unwrap_or_default(),
            status: crate::types::RequestStatus::Queued,
            created_at: run.created_at.clone(),
            started_at: None,
            completed_at: None,
            cwd: config.root.join(&config.artifacts[&eval.target].path),
            run_dir: None,
            argv: None,
            child: None,
            result: None,
            error: None,
            error_code: None,
            blocked_reason: None,
        })
        .collect();
    receipts.create_run(&run, &requests).await?;
    if options.announce_run {
        use std::io::Write;
        let _ = writeln!(
            std::io::stderr().lock(),
            "Started {} (follow: artifactize monitor)",
            run.id
        );
    }
    let mut evidence = match broker::schedule(
        config.clone(),
        &graph,
        &fingerprints,
        &keys,
        &parallelism,
        &limits,
        &mut run,
        &mut requests,
        &receipts,
        remote,
        cancellation.clone(),
    )
    .await
    {
        Ok(evidence) => evidence,
        Err(error) => {
            // A failing Run, for example on a rejected remote token, does not stay RUNNING.
            run.status = crate::types::RunStatus::Error;
            run.error = Some(error.clone());
            run.completed_at = Some(now());
            let _ = receipts.save_run(&run).await;
            return Err(error);
        }
    };
    let evaluation = graph.evaluate_with_policy(&evidence, ignore_gates);
    for request in &mut requests {
        if evidence.contains_key(&request.eval_id)
            || request.status == crate::types::RequestStatus::WaitingHuman
        {
            continue;
        }
        let eval = &evaluation.evals[request.eval_id.as_str()];
        if cancellation.is_cancelled() {
            request.status = crate::types::RequestStatus::Error;
            request.error = Some("Run was cancelled.".into());
            request.error_code = Some("CANCELLED".into());
            request.completed_at = Some(now());
            evidence.insert(request.eval_id.clone(), Evidence::OperationalError);
        } else if request.status == crate::types::RequestStatus::BudgetExhausted
            && eval.can_execute()
        {
            continue;
        } else {
            request.status = status(eval.status);
            request.blocked_reason = Some(format!(
                "{}: {}",
                if eval.status == EvalStatus::Blocked {
                    "Dependency verdict RED"
                } else {
                    "Waiting for current GREEN dependency evidence"
                },
                eval.unmet_gates.join(", ")
            ));
        }
    }
    let evaluation = graph.evaluate_with_policy(&evidence, ignore_gates);
    let required_evals: Vec<_> = evaluation
        .evals
        .iter()
        .filter(|(id, _)| required.contains(&graph.eval_target(id).unwrap()))
        .collect();
    let satisfied = required.iter().all(|id| evaluation.artifacts[id].satisfied);
    let budget_exhausted = requests
        .iter()
        .any(|request| request.status == crate::types::RequestStatus::BudgetExhausted);
    // Only a --reuse-only eval that had nothing to reuse leaves stale evidence in a Run.
    let not_reused = evidence
        .values()
        .filter(|evidence| matches!(evidence, Evidence::Stale))
        .count();
    run.status = if cancellation.is_cancelled() {
        crate::types::RunStatus::Error
    } else if run.wait_timed_out || budget_exhausted || not_reused > 0 {
        crate::types::RunStatus::Incomplete
    } else if required_evals
        .iter()
        .any(|(_, c)| c.status == EvalStatus::Error)
    {
        crate::types::RunStatus::Error
    } else if required_evals
        .iter()
        .any(|(_, c)| c.status == EvalStatus::Red)
    {
        crate::types::RunStatus::Red
    } else if satisfied {
        crate::types::RunStatus::Green
    } else {
        crate::types::RunStatus::Incomplete
    };
    if cancellation.is_cancelled() {
        run.error = Some("Run was cancelled.".into());
    } else if run.wait_timed_out {
        run.error =
            Some("Human wait timed out; pending requests remain available for submission.".into());
    } else if budget_exhausted {
        run.error = Some(broker::budget_reason(&run));
    } else if not_reused > 0 {
        run.error = Some(format!(
            "{not_reused} eval{} had no result to reuse and {} not executed (--reuse-only).",
            if not_reused == 1 { "" } else { "s" },
            if not_reused == 1 { "was" } else { "were" }
        ));
    }
    run.completed_at = Some(now());
    run.validation = json!({
        "selection":run.selection,
        "recursive":run.recursive,
        "force":run.force,
        "ignoreGates":run.ignore_gates,
        "selectedEvalIds":selected.evals.iter().map(|eval| &eval.id).collect::<Vec<_>>(),
        "includedEvalIds":evals.iter().map(|eval| &eval.id).collect::<Vec<_>>(),
        "satisfied":satisfied && !cancellation.is_cancelled(),
        "obligations":evaluation.obligations.iter().filter(|id| required.contains(*id)).collect::<Vec<_>>(),
        "artifacts":required.iter().map(|id| {
            let a = &evaluation.artifacts[id];
            let mut artifact = json!({"id":id,"status":format!("{:?}",a.status).to_uppercase(),"passed":a.passed,"total":a.total,"satisfied":a.satisfied});
            if let Some(fingerprint) = fingerprints.get(id) {
                artifact["fingerprintKind"] = json!(if fingerprint.manifest.is_some() { "content" } else { "script" });
                artifact["fingerprint"] = json!(fingerprint.value);
            }
            artifact
        }).collect::<Vec<_>>(),
        "evals":required_evals.iter().map(|(id, eval)| json!({"id":id,"status":status(eval.status),"blockedBy":eval.unmet_gates})).collect::<Vec<_>>(),
    });
    receipts.finish(&run, &requests).await?;
    // Saved Agent conversations past their size bound are collected; a failure only warns.
    let state = run.state_dir.clone();
    let bounds = limits.agent_sessions();
    let collected =
        tokio::task::spawn_blocking(move || crate::agent::session::collect(&state, bounds, false))
            .await
            .map_err(|e| e.to_string())
            .and_then(|collected| collected);
    if let Err(error) = collected {
        use std::io::Write;
        let _ = writeln!(
            std::io::stderr().lock(),
            "Agent session collection failed: {error}"
        );
    }
    store::read_run(&run.state_dir, &run.id).await
}

fn status(status: EvalStatus) -> crate::types::RequestStatus {
    match status {
        EvalStatus::Green => crate::types::RequestStatus::Green,
        EvalStatus::Red => crate::types::RequestStatus::Red,
        EvalStatus::Error => crate::types::RequestStatus::Error,
        EvalStatus::Stale => crate::types::RequestStatus::Stale,
        EvalStatus::Unreviewed => crate::types::RequestStatus::Unreviewed,
        EvalStatus::Wait => crate::types::RequestStatus::WaitDependency,
        EvalStatus::Blocked => crate::types::RequestStatus::Blocked,
    }
}
