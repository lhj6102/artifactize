//! Saved Run listing and bounded wait workflows.

use super::{Context, print_json};
use std::{
    io::{self, Write},
    path::PathBuf,
};

use super::outcome_code;
use std::time::Duration;

/// Match verify's ten-minute Human wait: allow interactive sign-off without an
/// unbounded CLI wait. A timeout observes the Run; it never cancels its execution.
const DEFAULT_RUN_WAIT: Duration = crate::project::DEFAULT_HUMAN_WAIT;
/// Poll often enough for responsive completion without hammering the state database.
const RUN_POLL_INTERVAL: Duration = Duration::from_millis(200);

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
    let deadline = tokio::time::Instant::now() + timeout_ms.unwrap_or(DEFAULT_RUN_WAIT);
    let mut view = crate::store::read_run(&state, &run_id).await?;
    while wait && view.run.status == crate::types::RunStatus::Running {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        tokio::time::sleep(remaining.min(RUN_POLL_INTERVAL)).await;
        view = crate::store::read_run(&state, &run_id).await?;
    }
    print_json(&crate::query::run_output(
        &view,
        time::OffsetDateTime::now_utc(),
    ))?;
    if !wait {
        return Ok(0);
    }
    if view.run.status == crate::types::RunStatus::Running {
        writeln!(
            io::stderr().lock(),
            "Waiting timed out; Run {run_id} is still RUNNING."
        )
        .map_err(|e| e.to_string())?;
        return Ok(3);
    }
    Ok(outcome_code(&view.run))
}
