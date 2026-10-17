//! Repository discovery and verification workflows, without text rendering.

use super::{Context, INTERRUPTED_EXIT, cancellation_listener, print_json};

use std::{
    io::{self, Write},
    path::PathBuf,
};

use super::{PolicyArgs, SelectionArgs, outcome_code};
use artifactize_core::{config::read_workspace_config, project::selection::Selection};

/// What `verify` does after submitting the Run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Follow {
    /// Print the Run id and return at once.
    Submit,
    /// Observe until the Run ends, then render its result.
    Wait,
    /// Stream JSON-line updates until the Run ends.
    Watch,
}

impl Follow {
    /// Normalizes the mutually exclusive `--wait` and `--watch` flags.
    pub(super) fn from_flags(wait: bool, watch: bool) -> Self {
        match (wait, watch) {
            (_, true) => Self::Watch,
            (true, false) => Self::Wait,
            (false, false) => Self::Submit,
        }
    }
}

/// Execution-only arguments, separate from selection and dependency policy.
pub(super) struct VerifyLimits {
    pub modes: artifactize_core::runs::RunModes,
    pub jobs: Option<u32>,
    pub max_executions: Option<u64>,
    pub follow: Follow,
    pub request_key: Option<artifactize_core::types::RequestKey>,
    pub reuse_only: Vec<artifactize_core::config::ProfileKind>,
}

pub(super) async fn verify(
    context: Context,
    selection: SelectionArgs,
    policy: PolicyArgs,
    limits: VerifyLimits,
) -> Result<u8, String> {
    let VerifyLimits {
        modes,
        jobs,
        max_executions,
        follow,
        request_key,
        reuse_only,
    } = limits;
    let selection = selection.resolve()?;
    let options = artifactize_core::project::VerifyOptions {
        modes,
        jobs: jobs.map(|limit| limit as usize),
        max_executions,
        reuse_only: reuse_only.into_iter().collect(),
        ..policy.options()
    };
    let repo = context.repo.unwrap_or_else(|| PathBuf::from("."));
    let runs = artifactize_core::runs::Context::new(
        &repo,
        context.state_dir.as_deref(),
        &std::env::current_exe().map_err(|e| e.to_string())?,
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
    if let Ok(log) = std::fs::File::open(
        runs.state()
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
    let result = artifactize_core::project::status(
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
    artifact: Option<artifactize_core::types::ArtifactName>,
    impact: bool,
) -> Result<u8, String> {
    let config = read_workspace_config(&context.repo.unwrap_or_else(|| PathBuf::from(".")))
        .map_err(|error| error.to_string())?;
    let selection = artifact.map_or(Selection::All, |artifact_id| Selection::Artifact {
        artifact_id,
    });
    let selection = if impact {
        artifactize_core::query::impact(&config, &selection)?
    } else {
        selection
    };
    let view = artifactize_core::query::graph(&config, &selection)?;
    if context.json {
        print_json(&view)?;
    } else {
        super::render::graph(&view).map_err(|error| error.to_string())?;
    }
    Ok(0)
}

pub(super) async fn check(context: Context) -> Result<u8, String> {
    let repo = context.repo.unwrap_or_else(|| PathBuf::from("."));
    let report = artifactize_core::inspect::config_check(&repo)?;
    if context.json {
        print_json(&report)?;
    } else {
        writeln!(io::stdout().lock(), "Folder configuration is valid.")
            .map_err(|error| error.to_string())?;
    }
    Ok(0)
}

fn context_for_watch(runs: &artifactize_core::runs::Context, json: bool) -> Context {
    Context {
        repo: Some(runs.repo().to_path_buf()),
        state_dir: Some(runs.state().to_path_buf()),
        json,
    }
}
