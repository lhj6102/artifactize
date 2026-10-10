//! Repository discovery and verification workflows, without text rendering.

use super::{Context, cancellation_listener, print_json};
use serde_json::json;
use std::{
    io::{self, Write},
    path::PathBuf,
    time::Duration,
};

use super::{PolicyArgs, SelectionArgs, outcome_code};
use crate::{config::read_workspace_config, project::selection::Selection};

/// Execution-only arguments, separate from selection and dependency policy.
pub(super) struct VerifyLimits {
    pub jobs: u32,
    pub max_executions: Option<u64>,
    pub timeout_ms: Option<Duration>,
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
        timeout_ms,
        reuse_only,
    } = limits;
    let selection = selection.resolve()?;
    let options = crate::project::VerifyOptions {
        jobs: jobs as usize,
        max_executions,
        wait_timeout: timeout_ms.unwrap_or(crate::project::DEFAULT_HUMAN_WAIT),
        reuse_only: reuse_only.into_iter().collect(),
        announce_run: true,
        ..policy.options()
    };
    let (cancellation, listener) = cancellation_listener()?;
    let result = crate::project::verify(
        &context.repo.unwrap_or_else(|| PathBuf::from(".")),
        context.state_dir.as_deref(),
        &selection,
        &options,
        cancellation,
    )
    .await;
    listener.abort();
    let view = result?;
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
        artifact_id: artifact_id.into(),
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
    let config = read_workspace_config(&repo).map_err(|error| error.to_string())?;
    let mut stdout = io::stdout().lock();
    if context.json {
        writeln!(
            stdout,
            "{}",
            json!({ "ok": true, "artifacts": config.artifacts.len(), "evals": config.evals.len() })
        )
    } else {
        writeln!(stdout, "Folder configuration is valid.")
    }
    .map_err(|error| error.to_string())?;
    Ok(0)
}
