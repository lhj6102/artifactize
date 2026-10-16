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
    let state = crate::store::state_dir(context.state_dir.as_deref())?;
    let repo = (!all).then(|| context.repo.unwrap_or_else(|| PathBuf::from(".")));
    let runs = crate::store::read_runs(&state, repo.as_deref(), limit, offset).await?;
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
                crate::platform::path_text(&run.repo_path),
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
    run_id: crate::types::RunId,
    wait: bool,
    timeout_ms: Option<Duration>,
) -> Result<u8, String> {
    let state = crate::store::state_dir(context.state_dir.as_deref())?;
    let initial = crate::store::read_run(&state, &run_id).await?;
    let runs = crate::runs::Context::new(
        &initial.run.repo_path,
        Some(&state),
        &std::env::current_exe().map_err(|e| e.to_string())?,
    )?;
    if wait {
        let (cancellation, listener) = super::cancellation_listener()?;
        let result = crate::runs::wait(
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
    let view = crate::runs::view(&runs, &run_id).await?;
    print_json(&crate::query::run_output(
        &view,
        time::OffsetDateTime::now_utc(),
    ))?;
    if !wait {
        return Ok(0);
    }
    if view.run.status() == crate::types::RunStatus::Running {
        writeln!(
            io::stderr().lock(),
            "Waiting timed out; Run {run_id} is still RUNNING."
        )
        .map_err(|e| e.to_string())?;
        return Ok(WAIT_TIMEOUT_EXIT);
    }
    Ok(outcome_code(&view.run))
}

pub(super) async fn cancel(context: Context, id: crate::types::RunId) -> Result<u8, String> {
    let state = crate::store::state_dir(context.state_dir.as_deref())?;
    let view = crate::store::read_run(&state, &id).await?;
    let runs = crate::runs::Context::new(
        &view.run.repo_path,
        Some(&state),
        &std::env::current_exe().map_err(|e| e.to_string())?,
    )?;
    print_json(&crate::runs::cancel(&runs, &id).await?)?;
    Ok(0)
}
