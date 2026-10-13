//! Repository discovery and verification workflows, without text rendering.

use super::{Context, cancellation_listener, print_json};

use std::{
    io::{self, Write},
    path::PathBuf,
};

use super::{PolicyArgs, SelectionArgs, outcome_code};
use crate::{config::read_workspace_config, project::selection::Selection};

/// Execution-only arguments, separate from selection and dependency policy.
pub(super) struct VerifyLimits {
    pub jobs: u32,
    pub max_executions: Option<u64>,
    pub wait: bool,
    pub request_key: Option<crate::types::RequestKey>,
    pub reuse_only: Vec<crate::config::ProfileKind>,
}

pub(super) async fn verify(
    context: Context,
    selection: SelectionArgs,
    policy: PolicyArgs,
    limits: VerifyLimits,
) -> Result<u8, String> {
    let VerifyLimits {
        jobs,
        max_executions,
        wait,
        request_key,
        reuse_only,
    } = limits;
    let selection = selection.resolve()?;
    let options = crate::project::VerifyOptions {
        jobs: jobs as usize,
        max_executions,
        reuse_only: reuse_only.into_iter().collect(),
        ..policy.options()
    };
    let repo = context.repo.unwrap_or_else(|| PathBuf::from("."));
    let runs = crate::runs::Context::new(
        &repo,
        context.state_dir.as_deref(),
        &std::env::current_exe().map_err(|e| e.to_string())?,
    )?;
    let observation = crate::runs::start(&runs, &selection, &options, request_key).await?;
    if !wait {
        if context.json {
            print_json(&serde_json::json!({"run": observation.run, "state": observation.state}))?;
        } else {
            writeln!(io::stdout().lock(), "{}", observation.run).map_err(|e| e.to_string())?;
        }
        return Ok(0);
    }
    let (cancellation, listener) = cancellation_listener()?;
    writeln!(
        io::stderr().lock(),
        "Started {} (follow: artifactize monitor)",
        observation.run
    )
    .map_err(|e| e.to_string())?;
    let result = crate::runs::wait(&runs, &observation.run, None, cancellation.clone()).await;
    listener.abort();
    if cancellation.is_cancelled() {
        return Ok(130);
    }
    result?;
    let view = crate::runs::view(&runs, &observation.run).await?;
    if let Ok(log) = std::fs::File::open(
        runs.state
            .join("runs")
            .join(view.run.id.as_str())
            .join("worker.stderr"),
    ) {
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
    super::render::verify(&view, context.json)?;
    Ok(outcome_code(&view.run))
}

pub(super) async fn status(
    context: Context,
    selection: SelectionArgs,
    policy: PolicyArgs,
) -> Result<u8, String> {
    let selection = selection.resolve()?;
    let (cancellation, listener) = cancellation_listener()?;
    let result = crate::project::status(
        &context.repo.unwrap_or_else(|| PathBuf::from(".")),
        context.state_dir.as_deref(),
        &selection,
        &policy.options(),
        cancellation,
    )
    .await;
    listener.abort();
    let view = result?;
    if context.json {
        print_json(&view)?;
    } else {
        super::render::status(&view).map_err(|error| error.to_string())?;
    }
    Ok(u8::from(!view.satisfied))
}

pub(super) async fn graph(
    context: Context,
    artifact: Option<crate::types::ArtifactName>,
) -> Result<u8, String> {
    let config = read_workspace_config(&context.repo.unwrap_or_else(|| PathBuf::from(".")))
        .map_err(|error| error.to_string())?;
    let selection = artifact.map_or(Selection::All, |artifact_id| Selection::Artifact {
        artifact_id,
    });
    let view = crate::query::graph(&config, &selection)?;
    if context.json {
        print_json(&view)?;
    } else {
        super::render::graph(&view).map_err(|error| error.to_string())?;
    }
    Ok(0)
}

pub(super) async fn check(context: Context) -> Result<u8, String> {
    let repo = context.repo.unwrap_or_else(|| PathBuf::from("."));
    let report = crate::inspect::config_check(&repo)?;
    if context.json {
        print_json(&report)?;
    } else {
        writeln!(io::stdout().lock(), "Folder configuration is valid.")
            .map_err(|error| error.to_string())?;
    }
    Ok(0)
}
