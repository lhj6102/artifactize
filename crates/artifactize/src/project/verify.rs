use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use serde_json::{Value, json};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio_util::sync::CancellationToken;

use crate::{
    cache,
    config::{DependencyGates, Eval, Profile, read_workspace_config},
    graph::{EvalStatus, Evidence, Graph},
    process,
    project::selection::{ProfileSelection, Selection, select_profiles},
    runtime::{self, Outcome, Verdict},
    scope,
    store::{self, Execution, Provenance, Receipts, Request, Run, RunView},
    workspace,
};

fn now() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .expect("UTC timestamp is representable")
}

#[derive(Debug, Default, Clone)]
pub struct VerifyOptions {
    pub profile: Option<ProfileSelection>,
    pub recursive: bool,
    /// Force only explicitly selected Evals, never recursive dependencies.
    pub force: bool,
    /// None uses root reviewPolicy; Some(false) explicitly enforces GREEN gates.
    pub ignore_gates: Option<bool>,
}

/// Executes foreground and sequentially, reusing completed explicit identities.
pub async fn verify(
    repo: &Path,
    state_dir: Option<&Path>,
    selection: &Selection,
    options: &VerifyOptions,
    cancellation: CancellationToken,
) -> Result<RunView, String> {
    let config = read_workspace_config(repo).map_err(|e| e.to_string())?;
    let config = select_profiles(
        config,
        selection,
        options.profile.as_ref(),
        options.recursive,
    )?;
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
    for eval in &evals {
        let unsupported = match eval.declaration.profile {
            Profile::Agent { .. } => Some("Agent Evals are not supported yet (P5)"),
            Profile::Human { .. } => Some("Human Evals are not supported yet (P6)"),
            Profile::Runtime { .. } => None,
        };
        if let Some(message) = unsupported
            && (config.artifacts[&eval.target].stale.is_none()
                || (options.force && selected_ids.contains(eval.id.as_str())))
        {
            return Err(format!("{}: {message}.", eval.id));
        }
    }
    let state = store::state_dir(state_dir)?;
    let receipts = Receipts::open(&state, &config.root).await?;
    let runs = workspace::prepare_directory(&state.join("runs"), &config.root)
        .map_err(|e| e.to_string())?;
    let identities = cache::prepare(
        &config,
        required.iter().copied(),
        &runs,
        cancellation.clone(),
    )
    .await?;
    for eval in &evals {
        if !matches!(eval.declaration.profile, Profile::Runtime { .. })
            && receipts
                .cached_execution(&identities[eval.target.as_str()])
                .await?
                .is_none()
        {
            return Err(format!(
                "{}: Evals of this kind are not supported yet without cached evidence.",
                eval.id
            ));
        }
    }
    let owner = process::identity(std::process::id()).map_err(|e| e.to_string())?;
    if cancellation.is_cancelled() {
        return Err("Project preparation was cancelled.".into());
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
    let run_dir = directory.keep();
    let mut run = Run {
        id,
        repo_path: config.root.clone(),
        state_dir: state,
        status: "RUNNING".into(),
        created_at: now(),
        completed_at: None,
        selection: serde_json::to_value(selection).expect("selection is JSON"),
        recursive: options.recursive,
        force: options.force,
        ignore_gates,
        validation: Value::Null,
        error: None,
    };
    let mut requests: Vec<_> = evals
        .iter()
        .enumerate()
        .map(|(ordinal, eval)| Request {
            id: format!("{}-{}", run.id, ordinal + 1),
            run_id: run.id.clone(),
            eval_id: eval.id.clone(),
            target: eval.target.clone(),
            title: eval.declaration.title.clone(),
            profile: serde_json::to_value(&eval.declaration.profile).expect("profile is JSON"),
            requested_profile: serde_json::to_value(&eval.declaration.profile)
                .expect("profile is JSON"),
            execution_id: None,
            provenance: None,
            usage: None,
            payload: json!(eval.declaration.payload),
            references: json!(eval.references),
            deps: eval.deps.clone(),
            force: options.force && selected_ids.contains(eval.id.as_str()),
            identity: identities.get(eval.target.as_str()).cloned(),
            status: "QUEUED".into(),
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
    let mut evidence = BTreeMap::new();
    loop {
        if cancellation.is_cancelled() {
            break;
        }
        for eval in config
            .evals
            .iter()
            .filter(|eval| required.contains(eval.target.as_str()))
        {
            if evidence.contains_key(&eval.id)
                || (options.force && selected_ids.contains(eval.id.as_str()))
            {
                continue;
            }
            if let Some(identity) = identities.get(eval.target.as_str())
                && let Some(execution) = receipts.cached_execution(identity).await?
            {
                evidence.insert(
                    eval.id.clone(),
                    Evidence::Current(execution.verdict().expect("completed cache entry")),
                );
                if let Some(request) = requests
                    .iter_mut()
                    .find(|request| request.eval_id == eval.id)
                {
                    cache::reuse(request, &execution, now());
                    receipts.reuse_execution(request).await?;
                }
            }
        }
        let evaluation = graph.evaluate_with_policy(&evidence, ignore_gates);
        let Some(index) = evals
            .iter()
            .position(|eval| evaluation.evals[eval.id.as_str()].can_execute())
        else {
            break;
        };
        let eval = evals[index];
        let request = &mut requests[index];
        request.status = "RUNNING".into();
        request.started_at = Some(now());
        receipts.save_request(request).await?;
        let prepared = prepare(&config, eval, &run_dir, request);
        let outcome = match prepared {
            Ok(command) => {
                receipts.save_request(request).await?;
                let receipts = receipts.clone();
                let mut registered = request.clone();
                let (send, mut receive) = tokio::sync::oneshot::channel();
                let outcome =
                    runtime::execute(command, cancellation.clone(), move |child| async move {
                        registered.child =
                            Some(json!({"pid":child.pid,"startTime":child.start_time}));
                        let _ = send.send(registered.child.clone());
                        receipts
                            .save_request(&registered)
                            .await
                            .map_err(std::io::Error::other)?;
                        Ok(())
                    })
                    .await;
                if let Ok(child) = receive.try_recv() {
                    request.child = child;
                }
                Some(outcome)
            }
            Err(error) => {
                request.error = Some(error);
                request.error_code = Some("PREPARATION_FAILED".into());
                None
            }
        };
        let outcome = if matches!(outcome, Some(Outcome::Completed(_)))
            && let Some(expected) = &request.identity
        {
            match cache::identity(&config, &eval.target, &run_dir, cancellation.clone()).await {
                Ok(value) if &value == expected => outcome,
                Ok(_) => {
                    request.error =
                        Some("Artifact input changed during review (identity differs).".into());
                    request.error_code = Some("INPUT_CHANGED".into());
                    None
                }
                Err(error) => {
                    request.error = Some(error);
                    request.error_code = Some(
                        if cancellation.is_cancelled() {
                            "CANCELLED"
                        } else {
                            "IDENTITY_RECHECK_FAILED"
                        }
                        .into(),
                    );
                    None
                }
            }
        } else {
            outcome
        };
        match outcome {
            Some(Outcome::Completed(result)) => {
                request.status = match result.verdict {
                    Verdict::Green => "GREEN",
                    Verdict::Red => "RED",
                }
                .into();
                evidence.insert(eval.id.clone(), Evidence::Current(result.verdict));
                request.result = Some(json!({
                    "verdict":request.status,
                    "exitCode":result.exit_code,
                    "stdout":String::from_utf8_lossy(&result.output.stdout),
                    "stderr":String::from_utf8_lossy(&result.output.stderr),
                    "durationMs":result.output.duration.as_millis() as u64,
                    "truncated":result.output.truncated,
                }));
            }
            failure => {
                request.status = "ERROR".into();
                evidence.insert(eval.id.clone(), Evidence::OperationalError);
                if let Some(Outcome::OperationalError(error)) = failure {
                    request.error_code = Some(
                        match &error {
                            runtime::Error::Process(process::Error::Cancelled) => "CANCELLED",
                            runtime::Error::Process(process::Error::Timeout) => "TIMEOUT",
                            runtime::Error::Process(process::Error::Spawn(_)) => "SPAWN_FAILED",
                            runtime::Error::AbnormalExit { .. } => "ABNORMAL_EXIT",
                            _ => "RUNTIME_ERROR",
                        }
                        .into(),
                    );
                    request.error = Some(error.to_string());
                }
            }
        }
        let completed_at = now();
        request.completed_at = Some(completed_at.clone());
        let provenance = Provenance {
            repo_path: config.root.clone(),
            run_id: run.id.clone(),
            request_id: request.id.clone(),
            eval_id: eval.id.clone(),
            completed_at: completed_at.clone(),
        };
        let execution = Execution {
            id: format!("execution-{}", request.id),
            identity: request.identity.clone().filter(|_| !request.force),
            owner_pid: owner.pid,
            owner_start_time: owner.start_time,
            status: request.status.clone(),
            result: request.result.clone(),
            error: request.error.clone(),
            error_code: request.error_code.clone(),
            profile: request.profile.clone(),
            usage: request.usage.clone(),
            provenance: provenance.clone(),
            started_at: request.started_at.clone().expect("execution started"),
            completed_at,
        };
        request.execution_id = Some(execution.id.clone());
        request.provenance = Some(provenance);
        receipts.complete_execution(&execution, request).await?;
    }
    let evaluation = graph.evaluate_with_policy(&evidence, ignore_gates);
    for request in &mut requests {
        if evidence.contains_key(&request.eval_id) {
            continue;
        }
        let eval = &evaluation.evals[request.eval_id.as_str()];
        if cancellation.is_cancelled() {
            request.status = "ERROR".into();
            request.error = Some("Run was cancelled.".into());
            request.error_code = Some("CANCELLED".into());
            request.completed_at = Some(now());
            evidence.insert(request.eval_id.clone(), Evidence::OperationalError);
        } else {
            request.status = status(eval.status).into();
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
    run.status = if cancellation.is_cancelled()
        || required_evals
            .iter()
            .any(|(_, c)| c.status == EvalStatus::Error)
    {
        "ERROR"
    } else if required_evals
        .iter()
        .any(|(_, c)| c.status == EvalStatus::Red)
    {
        "RED"
    } else if satisfied {
        "GREEN"
    } else {
        "INCOMPLETE"
    }
    .into();
    if cancellation.is_cancelled() {
        run.error = Some("Run was cancelled.".into());
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
            if let Some(value) = identities.get(id) {
                artifact["identity"] = json!("script");
                artifact["value"] = json!(value);
            }
            artifact
        }).collect::<Vec<_>>(),
        "evals":required_evals.iter().map(|(id, eval)| json!({"id":id,"status":status(eval.status),"blockedBy":eval.unmet_gates})).collect::<Vec<_>>(),
    });
    receipts.finish(&run, &requests).await?;
    Ok(RunView { run, requests })
}

fn prepare(
    config: &crate::config::RepoConfig,
    eval: &Eval,
    run_dir: &Path,
    request: &mut Request,
) -> Result<runtime::Command, String> {
    let Profile::Runtime {
        command,
        args,
        timeout_ms,
    } = &eval.declaration.profile
    else {
        return Err("Evals of this kind are not supported yet without cached evidence.".into());
    };
    let scope = scope::eval_scope(config, eval).map_err(|e| e.to_string())?;
    let args =
        scope::resolve_argv(config, &scope, &eval.target, args).map_err(|e| e.to_string())?;
    let cwd = scope
        .resolve_input(&config.root, &eval.target, "")
        .map_err(|e| e.to_string())?;
    let mut prepared = runtime::Command::prepare(
        command.into(),
        args.iter().map(Into::into).collect(),
        &config.root,
        run_dir,
        *timeout_ms,
    )
    .map_err(|e| e.to_string())?;
    prepared.cwd = cwd;
    request.run_dir = Some(prepared.directory().to_path_buf());
    request.argv = Some(std::iter::once(command.clone()).chain(args).collect());
    Ok(prepared)
}

fn status(status: EvalStatus) -> &'static str {
    match status {
        EvalStatus::Green => "GREEN",
        EvalStatus::Red => "RED",
        EvalStatus::Error => "ERROR",
        EvalStatus::Stale => "STALE",
        EvalStatus::Unreviewed => "UNREVIEWED",
        EvalStatus::Wait => "WAIT_DEPENDENCY",
        EvalStatus::Blocked => "BLOCKED",
    }
}
