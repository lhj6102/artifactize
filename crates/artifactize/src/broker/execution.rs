use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use serde_json::json;
use tokio_util::sync::CancellationToken;

use super::now;
use crate::{
    cache,
    config::{Eval, Profile, RepoConfig},
    process,
    runtime::{self, Outcome, Verdict},
    scope,
    store::{Execution, Receipts, Request},
};

pub(super) async fn execute(
    config: Arc<RepoConfig>,
    receipts: Receipts,
    mut request: Request,
    mut execution: Execution,
    prepared: Result<runtime::Command, String>,
    run_dir: PathBuf,
    cancellation: CancellationToken,
) -> Result<Request, String> {
    let outcome = match prepared {
        Ok(command) => {
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
        match cache::identity(&config, &request.target, &run_dir, cancellation.clone()).await {
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
    let outcome = if cancellation.is_cancelled() {
        Some(Outcome::OperationalError(runtime::Error::Process(
            process::Error::Cancelled,
        )))
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
    execution.status = request.status.clone();
    execution.result = request.result.clone();
    execution.error = request.error.clone();
    execution.error_code = request.error_code.clone();
    execution.usage = request.usage.clone();
    execution.completed_at = request.completed_at.clone();
    execution.provenance.completed_at = request.completed_at.clone();
    request.execution_id = Some(execution.id.clone());
    request.provenance = Some(execution.provenance.clone());
    receipts.complete_execution(&execution, &request).await?;
    Ok(request)
}

pub(super) fn prepare(
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
