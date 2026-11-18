//! Submit a verification Run and follow it as requested.

use super::{Context, INTERRUPTED_EXIT, cancellation_listener, print_json};

use std::{
    io::{self, Write},
    path::PathBuf,
};

use artifactize_core::project::selection::Selection;

use super::{Follow, PolicyArgs, outcome_code};

/// Per-request arguments, separate from selection and dependency policy.
pub(super) struct VerifyLimits {
    pub modes: artifactize_core::runs::RunModes,
    pub jobs: Option<std::num::NonZeroU32>,
    pub max_executions: Option<u64>,
    pub follow: Follow,
    pub request_key: Option<artifactize_core::types::RequestKey>,
    pub comment: Option<artifactize_core::types::RunComment>,
    pub reuse_only: Vec<artifactize_core::config::ProfileKind>,
}

pub(super) async fn verify(
    context: Context,
    selection: Selection,
    policy: PolicyArgs,
    limits: VerifyLimits,
) -> Result<u8, String> {
    let VerifyLimits {
        modes,
        jobs,
        max_executions,
        follow,
        request_key,
        comment,
        reuse_only,
    } = limits;
    let options = artifactize_core::project::VerifyOptions {
        modes,
        jobs: jobs.map(|limit| limit.get() as usize),
        max_executions,
        reuse_only: reuse_only.into_iter().collect(),
        comment,
        ..policy.options()
    };
    let repo = context.repo.unwrap_or_else(|| PathBuf::from("."));
    let runs = artifactize_core::runs::Context::new(
        &repo,
        context.state_dir.as_deref(),
        &artifactize_core::platform::current_executable().map_err(|e| e.to_string())?,
    )?;
    let observation =
        artifactize_core::runs::start(&runs, &selection, &options, request_key).await?;
    match follow {
        Follow::Watch => {
            return super::run::watch(context_for_watch(&runs, context.json), observation.run)
                .await;
        }
        Follow::Submit => {
            if context.json {
                print_json(
                    &serde_json::json!({"run": observation.run, "state": observation.state}),
                )?;
            } else {
                writeln!(io::stdout().lock(), "{}", observation.run).map_err(|e| e.to_string())?;
            }
            return Ok(0);
        }
        Follow::Wait => {}
    }
    let (cancellation, listener) = cancellation_listener()?;
    writeln!(
        io::stderr().lock(),
        "Started {} (follow: artifactize monitor)",
        observation.run
    )
    .map_err(|e| e.to_string())?;
    let result =
        artifactize_core::runs::wait(&runs, &observation.run, None, cancellation.clone()).await;
    listener.abort();
    if cancellation.is_cancelled() {
        return Ok(INTERRUPTED_EXIT);
    }
    result?;
    let view = artifactize_core::runs::view(&runs, &observation.run).await?;
    let diagnostics = runs
        .state()
        .join("runs")
        .join(view.run.id.as_str())
        .join("worker.stderr");
    if let Ok(log) = artifactize_core::platform::open_regular(&diagnostics) {
        use std::io::Read;
        // Operational diagnostics stay bounded independently of owner command output.
        const DIAGNOSTIC_BYTES: u64 = 64 * 1024;
        let mut bytes = Vec::new();
        let _ = log.take(DIAGNOSTIC_BYTES).read_to_end(&mut bytes);
        let _ = io::stderr().lock().write_all(&bytes);
    }
    if let Some(error) = view.run.error() {
        let _ = writeln!(io::stderr().lock(), "{error}");
    }
    let observed_at = time::OffsetDateTime::now_utc();
    super::render::verify(&view, context.json, observed_at)?;
    Ok(outcome_code(&view.run))
}

fn context_for_watch(runs: &artifactize_core::runs::Context, json: bool) -> Context {
    Context {
        repo: Some(runs.repo().to_path_buf()),
        state_dir: Some(runs.state().to_path_buf()),
        json,
    }
}
