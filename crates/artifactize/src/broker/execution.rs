use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use serde_json::json;
use tokio_util::sync::CancellationToken;

use super::now;
use crate::{
    agent, cache,
    config::{Eval, Profile, RepoConfig},
    human, process,
    runtime::{self, Outcome, Verdict},
    scope,
    store::{Execution, Receipts, Request},
};

pub(super) enum Prepared {
    Runtime(runtime::Command),
    Agent { state: PathBuf },
    Human,
}

pub(super) async fn execute(
    config: Arc<RepoConfig>,
    receipts: Receipts,
    mut request: Request,
    mut execution: Execution,
    prepared: Result<Prepared, String>,
    run_dir: PathBuf,
    cancellation: CancellationToken,
) -> Result<Request, String> {
    if matches!(prepared, Ok(Prepared::Human)) && !cancellation.is_cancelled() {
        request.status = "WAITING_HUMAN".into();
        request.started_at = Some(now());
        request.execution_id = Some(execution.id.clone());
        request.provenance = Some(execution.provenance.clone());
        request.blocked_reason = Some("Waiting for a Human claim and submission.".into());
        execution.status = request.status.clone();
        receipts.wait_for_human(&execution, &request).await?;
        return Ok(request);
    }
    let outcome = match prepared {
        Ok(Prepared::Human) => {
            request.human_definition = None;
            None
        }
        Ok(Prepared::Agent { state }) => {
            let eval = config
                .evals
                .iter()
                .find(|eval| eval.id == request.eval_id)
                .expect("included eval");
            let review =
                agent::execute(&config, eval, &run_dir, &state, cancellation.clone()).await;
            request.usage = Some(json!(review.attempts));
            request.tool_calls = review.tool_calls;
            match review.result {
                Ok(result) => {
                    let verdict = if result["verdict"] == "GREEN" {
                        Verdict::Green
                    } else {
                        Verdict::Red
                    };
                    request.result = Some(result);
                    Some(verdict)
                }
                Err(error) => {
                    request.error = Some(error);
                    request.error_code = Some("AGENT_ERROR".into());
                    None
                }
            }
        }
        Ok(Prepared::Runtime(command)) => {
            let receipts = receipts.clone();
            let mut registered = request.clone();
            let (send, mut receive) = tokio::sync::oneshot::channel();
            let outcome =
                runtime::execute(command, cancellation.clone(), move |child| async move {
                    registered.child = Some(json!({"pid":child.pid,"startTime":child.start_time}));
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
            runtime_result(outcome, &mut request)
        }
        Err(error) => {
            request.human_definition = None;
            request.error = Some(error);
            request.error_code = Some("PREPARATION_FAILED".into());
            None
        }
    };
    let outcome = if outcome.is_some()
        && let Some(expected) = &request.key
    {
        let eval = config
            .evals
            .iter()
            .find(|eval| eval.id == request.eval_id)
            .expect("included eval");
        match cache::recheck(&config, eval, &run_dir, cancellation.clone()).await {
            Ok(Some(value)) if &value == expected => outcome,
            Ok(_) => {
                request.error = Some("Fingerprint changed during review.".into());
                request.error_code = Some("INPUT_CHANGED".into());
                None
            }
            Err(error) => {
                request.error = Some(error);
                request.error_code = Some(
                    if cancellation.is_cancelled() {
                        "CANCELLED"
                    } else {
                        "FINGERPRINT_RECHECK_FAILED"
                    }
                    .into(),
                );
                None
            }
        }
    } else {
        outcome
    };
    let outcome = if cancellation.is_cancelled() {
        request.error = Some(process::Error::Cancelled.to_string());
        request.error_code = Some("CANCELLED".into());
        None
    } else {
        outcome
    };
    request.status = match outcome {
        Some(Verdict::Green) => "GREEN",
        Some(Verdict::Red) => "RED",
        None => {
            request.result = None;
            "ERROR"
        }
    }
    .into();
    request.completed_at = Some(now());
    execution.status = request.status.clone();
    execution.result = request.result.clone();
    execution.error = request.error.clone();
    execution.error_code = request.error_code.clone();
    execution.usage = request.usage.clone();
    execution.tool_calls = request.tool_calls.clone();
    execution.completed_at = request.completed_at.clone();
    execution.provenance.completed_at = request.completed_at.clone();
    request.execution_id = Some(execution.id.clone());
    request.provenance = Some(execution.provenance.clone());
    receipts.complete_execution(&execution, &request).await?;
    Ok(request)
}

fn runtime_result(outcome: Outcome, request: &mut Request) -> Option<Verdict> {
    match outcome {
        Outcome::Completed(result) => {
            request.result = Some(json!({
                "verdict":if result.verdict == Verdict::Green { "GREEN" } else { "RED" },
                "exitCode":result.exit_code,
                "stdout":String::from_utf8_lossy(&result.output.stdout),
                "stderr":String::from_utf8_lossy(&result.output.stderr),
                "durationMs":result.output.duration.as_millis() as u64,
                "truncated":result.output.truncated,
            }));
            Some(result.verdict)
        }
        Outcome::OperationalError(error) => {
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
            None
        }
    }
}

pub(super) fn prepare(
    config: &crate::config::RepoConfig,
    eval: &Eval,
    run_dir: &Path,
    state: &Path,
    request: &mut Request,
) -> Result<Prepared, String> {
    if matches!(eval.declaration.profile, Profile::Human {}) {
        request.human_definition = Some(human::definition(config, eval)?);
        request.run_dir = Some(run_dir.to_path_buf());
        return Ok(Prepared::Human);
    }
    if matches!(eval.declaration.profile, Profile::Agent { .. }) {
        request.run_dir = Some(run_dir.to_path_buf());
        return Ok(Prepared::Agent {
            state: state.into(),
        });
    }
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
    Ok(Prepared::Runtime(prepared))
}
