use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use tokio_util::sync::CancellationToken;

use super::{Stops, now};
use crate::{
    agent, cache,
    config::{Eval, Profile, RepoConfig},
    human, process,
    runtime::{self, Outcome},
    scope,
    store::{Execution, Receipts, Request},
};

pub(super) enum Prepared {
    Runtime(runtime::Command),
    Agent {
        state: PathBuf,
        session: crate::types::SessionId,
        /// Where the review's conversation is saved; `None` when saving is off.
        saving: Option<agent::session::Saving>,
    },
    Human,
}

#[expect(
    clippy::too_many_arguments,
    reason = "the request with its execution and preparation, and the Run's output, fingerprint bound, backend stops and cancellation"
)]
pub(super) async fn execute(
    config: Arc<RepoConfig>,
    receipts: Receipts,
    mut request: Request,
    mut execution: Execution,
    prepared: Result<Prepared, String>,
    run_dir: PathBuf,
    parallelism: &cache::Parallelism,
    stops: &Stops,
    cancellation: CancellationToken,
) -> Result<Request, String> {
    if matches!(prepared, Ok(Prepared::Human)) && !cancellation.is_cancelled() {
        request.state = crate::store::RequestState::WaitingHuman;
        request.started_at = Some(now());
        request.execution_id = Some(execution.id.clone());
        request.provenance = Some(execution.provenance.clone());
        request.queue = None;
        request.blocked_reason = Some("Waiting for a Human claim and submission.".into());
        execution.state = request.state.clone().try_into()?;
        receipts.wait_for_human(&execution, &request).await?;
        return Ok(request);
    }
    let outcome = match prepared {
        Ok(Prepared::Human) => {
            request.human_definition = None;
            Err((
                process::Error::Cancelled.to_string(),
                crate::types::FailureCode::Cancelled,
            ))
        }
        Ok(Prepared::Agent {
            state,
            session,
            saving,
        }) => {
            let eval = config
                .evals
                .iter()
                .find(|eval| eval.id == request.eval_id)
                .expect("included eval");
            let mut recorder = agent::session::Recorder::new(saving.as_ref(), &request, &session);
            let review = agent::execute(
                &config,
                eval,
                &run_dir,
                &state,
                &mut recorder,
                cancellation.clone(),
            )
            .await;
            // The result names its saved conversation, and so does every reuse of it.
            if let Some(reference) = recorder.reference() {
                request.session = Some(reference.clone());
                if let Some(producer) = &mut execution.producer {
                    producer.session = Some(reference.clone());
                }
            }
            request.usage = Some(review.attempts);
            review
                .result
                .map(Into::into)
                .map_err(|failure| (failure.message, failure.code.into()))
        }
        Ok(Prepared::Runtime(command)) => {
            let receipts = receipts.clone();
            let mut registered = request.clone();
            let (send, mut receive) = tokio::sync::oneshot::channel();
            let outcome =
                runtime::execute(command, cancellation.clone(), move |child| async move {
                    registered.child = Some(child.into());
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
            runtime_result(outcome)
        }
        Err(error) => {
            request.human_definition = None;
            Err((error, crate::types::FailureCode::PreparationFailed))
        }
    };
    let outcome = if outcome.is_ok() {
        let eval = config
            .evals
            .iter()
            .find(|eval| eval.id == request.eval_id)
            .expect("included eval");
        match cache::validate_file_inputs(&config, eval) {
            Ok(()) => outcome,
            Err(error) => Err((error, crate::types::FailureCode::InputChanged)),
        }
    } else {
        outcome
    };
    let outcome = if outcome.is_ok()
        && let Some(expected) = &request.key
    {
        let eval = config
            .evals
            .iter()
            .find(|eval| eval.id == request.eval_id)
            .expect("included eval");
        match cache::recheck(&config, eval, &run_dir, parallelism, cancellation.clone()).await {
            Ok(Some(value)) if &value == expected => outcome,
            Ok(_) => Err((
                "Fingerprint changed during review.".into(),
                crate::types::FailureCode::InputChanged,
            )),
            Err(error) => Err((
                error,
                if cancellation.is_cancelled() {
                    crate::types::FailureCode::Cancelled
                } else {
                    crate::types::FailureCode::FingerprintRecheckFailed
                },
            )),
        }
    } else {
        outcome
    };
    let outcome = if cancellation.is_cancelled() {
        Err((
            process::Error::Cancelled.to_string(),
            crate::types::FailureCode::Cancelled,
        ))
    } else {
        outcome
    };
    request.state = match outcome {
        Ok(result) => crate::store::RequestState::completed(result, now()),
        Err((error, code)) => crate::store::RequestState::failed(error, Some(code), now()),
    };
    execution.state = request.state.clone().try_into()?;
    execution.usage = request.usage.clone();
    execution.provenance.completed_at = request.completed_at();
    request.execution_id = Some(execution.id.clone());
    request.provenance = Some(execution.provenance.clone());
    // Admission sees a backend stop before completing frees this review's slot.
    stops.record(&request);
    receipts.complete_execution(&execution, &request).await?;
    Ok(request)
}

fn runtime_result(
    outcome: Outcome,
) -> Result<crate::store::ExecutionResult, (String, crate::types::FailureCode)> {
    match outcome {
        Outcome::Completed(result) => Ok(crate::store::ExecutionResult::Runtime(
            crate::store::RuntimeResult {
                verdict: result.verdict,
                exit_code: crate::store::ResultField::Value(result.exit_code),
                stdout: crate::store::ResultField::Value(
                    String::from_utf8_lossy(&result.output.stdout).into_owned(),
                ),
                stderr: crate::store::ResultField::Value(
                    String::from_utf8_lossy(&result.output.stderr).into_owned(),
                ),
                duration: crate::store::ResultField::Value(result.output.duration),
                truncated: crate::store::ResultField::Value(result.output.truncated),
                fields: serde_json::Map::new(),
            },
        )),
        Outcome::OperationalError(error) => {
            let code = match &error {
                runtime::Error::Process(process::Error::Cancelled) => {
                    crate::types::FailureCode::Cancelled
                }
                runtime::Error::Process(process::Error::Timeout) => {
                    crate::types::FailureCode::Timeout
                }
                runtime::Error::Process(process::Error::Spawn(_)) => {
                    crate::types::FailureCode::SpawnFailed
                }
                runtime::Error::AbnormalExit { .. } => crate::types::FailureCode::AbnormalExit,
                _ => crate::types::FailureCode::RuntimeError,
            };
            Err((error.to_string(), code))
        }
    }
}

pub(super) fn prepare(
    config: &crate::config::RepoConfig,
    eval: &Eval,
    run_dir: &Path,
    state: &Path,
    saving: Option<&agent::session::Saving>,
    request: &mut Request,
) -> Result<Prepared, String> {
    cache::validate_file_inputs(config, eval)?;
    if matches!(eval.declaration.profile(), Profile::Human {}) {
        request.human_definition = Some(human::definition(config, eval)?);
        request.run_dir = Some(run_dir.to_path_buf());
        return Ok(Prepared::Human);
    }
    if matches!(eval.declaration.profile(), Profile::Agent { .. }) {
        // Saved with the RUNNING request, before the review's first turn.
        let session = agent::session_id()?;
        request.run_dir = Some(run_dir.to_path_buf());
        request.session_id = Some(session.clone());
        return Ok(Prepared::Agent {
            state: state.into(),
            session,
            saving: saving.cloned(),
        });
    }
    let Profile::Runtime {
        command,
        args,
        timeout_ms,
    } = eval.declaration.profile()
    else {
        return Err("Evals of this kind are not supported yet without cached evidence.".into());
    };
    let scope = scope::eval_scope(config, eval).map_err(|e| e.to_string())?;
    let args =
        scope::resolve_argv(config, &scope, &eval.target, args).map_err(|e| e.to_string())?;
    let cwd = scope::scoped_path(&config.root, config.artifacts[&eval.target].folder())
        .map_err(|e| e.to_string())?;
    let program = scope::executable(&config.root, &scope, &eval.target, command)?;
    let mut prepared = runtime::Command::prepare(
        program,
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
