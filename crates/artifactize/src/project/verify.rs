use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use serde_json::{Value, json};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio_util::sync::CancellationToken;

use crate::{
    config::{Critic, DependencyGates, Profile, read_workspace_config},
    graph::{CriticStatus, Evidence, Graph},
    process,
    project::selection::{ProfileSelection, Selection, select_profiles},
    runtime::{self, Outcome, Verdict},
    scope,
    store::{self, Receipts, Request, Run, RunView},
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
    /// Force only explicitly selected Critics, never recursive dependencies.
    pub force: bool,
    /// None uses root reviewPolicy; Some(false) explicitly enforces GREEN gates.
    pub ignore_gates: Option<bool>,
}

/// Executes foreground and sequentially. There is no cross-run evidence reuse.
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
    let selected_ids: BTreeSet<_> = selected
        .critics
        .iter()
        .map(|critic| critic.id.as_str())
        .collect();
    let required: BTreeSet<_> = graph
        .dependency_closure(&selected.roots)
        .map_err(|e| e.to_string())?
        .into_iter()
        .collect();
    let critics = selection.included_critics(&config, options.recursive)?;
    for critic in &critics {
        let unsupported = match critic.declaration.profile {
            Profile::Agent { .. } => Some("Agent Critics are not supported yet (P5)"),
            Profile::Human { .. } => Some("Human Critics are not supported yet (P6)"),
            Profile::Runtime { .. } => None,
        };
        if let Some(message) = unsupported {
            return Err(format!("{}: {message}.", critic.id));
        }
    }
    let state = store::receipts_dir(&config.root, state_dir)?;
    let home = store::canonical_target(&store::state_home().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    store::outside_workspace(&config.root, &home).map_err(|e| e.to_string())?;
    let receipts = Receipts::open(&state, &config.root).await?;
    let runs =
        store::prepare_directory(&state.join("runs"), &config.root).map_err(|e| e.to_string())?;
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
    let mut requests: Vec<_> = critics
        .iter()
        .enumerate()
        .map(|(ordinal, critic)| Request {
            id: format!("{}-{}", run.id, ordinal + 1),
            run_id: run.id.clone(),
            critic_id: critic.id.clone(),
            target: critic.target.clone(),
            title: critic.declaration.title.clone(),
            profile: serde_json::to_value(&critic.declaration.profile).expect("profile is JSON"),
            payload: json!(critic.declaration.payload),
            references: json!(critic.references),
            deps: critic.deps.clone(),
            force: options.force && selected_ids.contains(critic.id.as_str()),
            status: "QUEUED".into(),
            created_at: run.created_at.clone(),
            started_at: None,
            completed_at: None,
            cwd: config.root.join(&config.artifacts[&critic.target].path),
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
        let evaluation = graph.evaluate_with_policy(&evidence, ignore_gates);
        let Some(index) = critics
            .iter()
            .position(|critic| evaluation.critics[critic.id.as_str()].can_execute())
        else {
            break;
        };
        let critic = critics[index];
        let request = &mut requests[index];
        request.status = "RUNNING".into();
        request.started_at = Some(now());
        receipts.save_request(request).await?;
        let prepared = prepare(&config, critic, &run_dir, request);
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
        match outcome {
            Some(Outcome::Completed(result)) => {
                request.status = match result.verdict {
                    Verdict::Green => "GREEN",
                    Verdict::Red => "RED",
                }
                .into();
                evidence.insert(critic.id.clone(), Evidence::Current(result.verdict));
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
                evidence.insert(critic.id.clone(), Evidence::OperationalError);
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
        request.completed_at = Some(now());
        receipts.save_request(request).await?;
    }
    let evaluation = graph.evaluate_with_policy(&evidence, ignore_gates);
    for request in &mut requests {
        if evidence.contains_key(&request.critic_id) {
            continue;
        }
        let critic = &evaluation.critics[request.critic_id.as_str()];
        if cancellation.is_cancelled() {
            request.status = "ERROR".into();
            request.error = Some("Run was cancelled.".into());
            request.error_code = Some("CANCELLED".into());
            request.completed_at = Some(now());
            evidence.insert(request.critic_id.clone(), Evidence::OperationalError);
        } else {
            request.status = status(critic.status).into();
            request.blocked_reason = Some(format!(
                "{}: {}",
                if critic.status == CriticStatus::Blocked {
                    "Dependency verdict RED"
                } else {
                    "Waiting for current GREEN dependency evidence"
                },
                critic.unmet_gates.join(", ")
            ));
        }
    }
    let evaluation = graph.evaluate_with_policy(&evidence, ignore_gates);
    let required_critics: Vec<_> = evaluation
        .critics
        .iter()
        .filter(|(id, _)| required.contains(&graph.critic_target(id).unwrap()))
        .collect();
    let satisfied = required.iter().all(|id| evaluation.artifacts[id].satisfied);
    run.status = if cancellation.is_cancelled()
        || required_critics
            .iter()
            .any(|(_, c)| c.status == CriticStatus::Error)
    {
        "ERROR"
    } else if required_critics
        .iter()
        .any(|(_, c)| c.status == CriticStatus::Red)
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
        "selectedCriticIds":selected.critics.iter().map(|critic| &critic.id).collect::<Vec<_>>(),
        "includedCriticIds":critics.iter().map(|critic| &critic.id).collect::<Vec<_>>(),
        "satisfied":satisfied && !cancellation.is_cancelled(),
        "obligations":evaluation.obligations.iter().filter(|id| required.contains(*id)).collect::<Vec<_>>(),
        "artifacts":required.iter().map(|id| {
            let a = &evaluation.artifacts[id];
            json!({"id":id,"status":format!("{:?}",a.status).to_uppercase(),"passed":a.passed,"total":a.total,"satisfied":a.satisfied})
        }).collect::<Vec<_>>(),
        "critics":required_critics.iter().map(|(id, critic)| json!({"id":id,"status":status(critic.status),"blockedBy":critic.unmet_gates})).collect::<Vec<_>>(),
    });
    receipts.finish(&run, &requests).await?;
    Ok(RunView { run, requests })
}

fn prepare(
    config: &crate::config::RepoConfig,
    critic: &Critic,
    run_dir: &Path,
    request: &mut Request,
) -> Result<runtime::Command, String> {
    let Profile::Runtime {
        command,
        args,
        timeout_ms,
    } = &critic.declaration.profile
    else {
        unreachable!("profiles checked before submission")
    };
    let scope = scope::critic_scope(config, critic).map_err(|e| e.to_string())?;
    let args =
        scope::resolve_argv(config, &scope, &critic.target, args).map_err(|e| e.to_string())?;
    let cwd = scope
        .resolve_input(&config.root, &critic.target, "")
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

fn status(status: CriticStatus) -> &'static str {
    match status {
        CriticStatus::Green => "GREEN",
        CriticStatus::Red => "RED",
        CriticStatus::Error => "ERROR",
        CriticStatus::Stale => "STALE",
        CriticStatus::Unreviewed => "UNREVIEWED",
        CriticStatus::Wait => "WAIT_DEPENDENCY",
        CriticStatus::Blocked => "BLOCKED",
    }
}
