//! Saved Run listing and bounded wait workflows.

use super::{Context, INTERRUPTED_EXIT, print_json};
use std::{
    io::{self, Write},
    path::PathBuf,
};

use super::{outcome_code, render::WAIT_TIMEOUT_EXIT};
use std::time::Duration;

/// An optional run-show bound affects observation only, never the Run lifetime.
const DEFAULT_RUN_WAIT: Duration = Duration::from_secs(600);

pub(super) async fn list(
    context: Context,
    all: bool,
    limit: u32,
    offset: u32,
) -> Result<u8, String> {
    let state = artifactize_core::store::state_dir(context.state_dir.as_deref())?;
    let repo = (!all).then(|| context.repo.unwrap_or_else(|| PathBuf::from(".")));
    let runs = artifactize_core::store::read_runs(&state, repo.as_deref(), limit, offset).await?;
    if context.json {
        print_json(&runs)?;
    } else {
        let mut out = io::stdout().lock();
        writeln!(out, "ID\tREPO\tCREATED\tCOMPLETED\tSTATUS\tREQUEST COUNTS")
            .map_err(|e| e.to_string())?;
        for run in runs {
            let counts = run
                .counts
                .iter()
                .map(|(state, count)| format!("{state}={count}"))
                .collect::<Vec<_>>()
                .join(" ");
            writeln!(
                out,
                "{}\t{}\t{}\t{}\t{}\t{}",
                run.id,
                artifactize_core::platform::path_text(&run.repo_path),
                run.created_at,
                run.completed_at
                    .map_or_else(|| "-".to_owned(), |time| time.to_string()),
                run.status,
                counts
            )
            .map_err(|e| e.to_string())?;
            for unreadable in &run.unreadable {
                writeln!(out, "  {unreadable}").map_err(|error| error.to_string())?;
            }
        }
    }
    Ok(0)
}

pub(super) async fn show(
    context: Context,
    run_id: artifactize_core::types::RunId,
    wait: bool,
    timeout_ms: Option<Duration>,
) -> Result<u8, String> {
    let state = artifactize_core::store::state_dir(context.state_dir.as_deref())?;
    let initial = artifactize_core::store::read_run(&state, &run_id).await?;
    let runs = artifactize_core::runs::Context::new(
        &initial.run.repo_path,
        Some(&state),
        &std::env::current_exe().map_err(|e| e.to_string())?,
    )?;
    if wait {
        let (cancellation, listener) = super::cancellation_listener()?;
        let result = artifactize_core::runs::wait(
            &runs,
            &run_id,
            Some(timeout_ms.unwrap_or(DEFAULT_RUN_WAIT)),
            cancellation.clone(),
        )
        .await;
        listener.abort();
        if cancellation.is_cancelled() {
            return Ok(INTERRUPTED_EXIT);
        }
        result?;
    }
    let view = artifactize_core::runs::view(&runs, &run_id).await?;
    print_json(&artifactize_core::query::run_output(
        &view,
        time::OffsetDateTime::now_utc(),
    ))?;
    if !wait {
        return Ok(0);
    }
    if view.run.status().is_open() {
        writeln!(
            io::stderr().lock(),
            "Waiting timed out; Run {run_id} is still RUNNING."
        )
        .map_err(|e| e.to_string())?;
        return Ok(WAIT_TIMEOUT_EXIT);
    }
    Ok(outcome_code(&view.run))
}

pub(super) async fn cancel(
    context: Context,
    id: artifactize_core::types::RunId,
) -> Result<u8, String> {
    let state = artifactize_core::store::state_dir(context.state_dir.as_deref())?;
    let view = artifactize_core::store::read_run(&state, &id).await?;
    let runs = artifactize_core::runs::Context::new(
        &view.run.repo_path,
        Some(&state),
        &std::env::current_exe().map_err(|e| e.to_string())?,
    )?;
    print_json(&artifactize_core::runs::cancel(&runs, &id).await?)?;
    Ok(0)
}

/// One JSON object per line, flushed immediately for pipes and background-agent hosts.
pub(super) async fn watch(
    context: Context,
    id: artifactize_core::types::RunId,
) -> Result<u8, String> {
    let state = artifactize_core::store::state_dir(context.state_dir.as_deref())?;
    let view = artifactize_core::store::read_run(&state, &id).await?;
    let runs = artifactize_core::runs::Context::new(
        &view.run.repo_path,
        Some(&state),
        &std::env::current_exe().map_err(|e| e.to_string())?,
    )?;
    let (cancellation, listener) = super::cancellation_listener()?;
    let result =
        artifactize_core::runs::watch(&runs, &id, None, cancellation.clone(), |event| async move {
            let mut out = io::stdout().lock();
            serde_json::to_writer(&mut out, &event).map_err(|e| e.to_string())?;
            writeln!(out)
                .and_then(|()| out.flush())
                .map_err(|e| e.to_string())
        })
        .await;
    listener.abort();
    if cancellation.is_cancelled() {
        return Ok(INTERRUPTED_EXIT);
    }
    result?;
    Ok(outcome_code(
        &artifactize_core::runs::view(&runs, &id).await?.run,
    ))
}
