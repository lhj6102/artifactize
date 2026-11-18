//! Saved Run listing and bounded wait workflows.

use artifactize_core::types::RunId;
use clap::Subcommand;

use super::{Context, INTERRUPTED_EXIT, print_json};
use std::{
    io::{self, Write},
    path::PathBuf,
};

use super::{outcome_code, render::WAIT_TIMEOUT_EXIT};
use std::time::Duration;

/// Ten minutes let normal reviews finish without binding a terminal to a long-lived Run.
/// This is only an observation deadline; it never cancels the Run.
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
        writeln!(
            out,
            "ID\tREPO\tCREATED\tCOMPLETED\tSTATUS\tREQUEST COUNTS\tCOMMENT"
        )
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
                "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                run.id,
                artifactize_core::platform::path_text(&run.repo_path),
                run.created_at,
                run.completed_at
                    .map_or_else(|| "-".to_owned(), |time| time.to_string()),
                run.status,
                counts,
                run.comment.as_deref().unwrap_or("-")
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(0)
}

pub(super) enum ShowMode {
    Snapshot,
    Wait { timeout: Duration },
}

impl ShowMode {
    /// Clap requires `--wait` when `--timeout-ms` is present.
    pub(super) fn from_flags(wait: bool, timeout: Option<Duration>) -> Self {
        match (wait, timeout) {
            (true, timeout) => Self::Wait {
                timeout: timeout.unwrap_or(DEFAULT_RUN_WAIT),
            },
            (false, None) => Self::Snapshot,
            (false, Some(_)) => unreachable!("clap requires --wait with --timeout-ms"),
        }
    }
}

pub(super) async fn show(
    context: Context,
    run_id: artifactize_core::types::RunId,
    mode: ShowMode,
) -> Result<u8, String> {
    let state = artifactize_core::store::state_dir(context.state_dir.as_deref())?;
    let repo = artifactize_core::store::read_run_repo(&state, &run_id).await?;
    let runs = artifactize_core::runs::Context::new(
        &repo,
        Some(&state),
        &artifactize_core::platform::current_executable().map_err(|e| e.to_string())?,
    )?;
    if let ShowMode::Wait { timeout } = mode {
        let (cancellation, listener) = super::cancellation_listener()?;
        let result =
            artifactize_core::runs::wait(&runs, &run_id, Some(timeout), cancellation.clone()).await;
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
    let code = show_outcome(mode, &view.run);
    if code == WAIT_TIMEOUT_EXIT {
        writeln!(
            io::stderr().lock(),
            "Waiting timed out; Run {run_id} is still RUNNING."
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(code)
}

fn show_outcome(mode: ShowMode, run: &artifactize_core::store::Run) -> u8 {
    match mode {
        ShowMode::Snapshot => 0,
        ShowMode::Wait { .. } if run.status().is_open() => WAIT_TIMEOUT_EXIT,
        ShowMode::Wait { .. } => outcome_code(run),
    }
}

pub(super) async fn cancel(
    context: Context,
    id: artifactize_core::types::RunId,
) -> Result<u8, String> {
    let state = artifactize_core::store::state_dir(context.state_dir.as_deref())?;
    let repo = artifactize_core::store::read_run_repo(&state, &id).await?;
    let runs = artifactize_core::runs::Context::new(
        &repo,
        Some(&state),
        &artifactize_core::platform::current_executable().map_err(|e| e.to_string())?,
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
    let repo = artifactize_core::store::read_run_repo(&state, &id).await?;
    let runs = artifactize_core::runs::Context::new(
        &repo,
        Some(&state),
        &artifactize_core::platform::current_executable().map_err(|e| e.to_string())?,
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

/// Keep default Run history output readable; --limit/--offset expose older records.
const DEFAULT_RUN_LIST_LIMIT: u32 = 50;

/// A zero bound would turn waiting into a snapshot, so the CLI starts at one millisecond.
const MIN_WAIT_MILLIS: u64 = 1;

/// Parse protocol milliseconds exactly once, preserving the public signed 32-bit bound.
fn timeout_duration(text: &str) -> Result<std::time::Duration, String> {
    let millis: u64 = text
        .parse()
        .map_err(|_| "timeout must be milliseconds".to_owned())?;
    if !(MIN_WAIT_MILLIS..=artifactize_core::config::MAX_TIMEOUT_MS).contains(&millis) {
        return Err(format!(
            "timeoutMs must be between {MIN_WAIT_MILLIS} and {}",
            artifactize_core::config::MAX_TIMEOUT_MS
        ));
    }
    Ok(std::time::Duration::from_millis(millis))
}

#[derive(Debug, Subcommand)]
pub enum RunCommand {
    /// Request cancellation; committed results are kept and cleanup settles asynchronously.
    Cancel { run_id: RunId },
    /// List saved Runs, newest first; defaults to the current repository.
    List {
        /// Explicitly restrict results to --repo (the default).
        #[arg(long, conflicts_with = "all")]
        repo_only: bool,
        /// List Runs from every repository in the shared state.
        #[arg(long)]
        all: bool,
        /// Maximum number of Runs to return.
        #[arg(long, value_name = "N", default_value_t = DEFAULT_RUN_LIST_LIMIT)]
        limit: u32,
        /// Skip this many Runs before returning results.
        #[arg(long, value_name = "N", default_value = "0")]
        offset: u32,
    },
    /// Stream live updates, saved eval results and final validation as JSON lines.
    Watch { run_id: RunId },
    /// Read the full saved audit as JSON, even without --json.
    Show {
        run_id: RunId,
        /// Follow a RUNNING Run until it finishes and exit with its outcome code.
        #[arg(long)]
        wait: bool,
        /// Wait timeout; never cancels the Run.
        #[arg(
            long,
            value_name = "MS",
            requires = "wait",
            value_parser = timeout_duration,
            help = format!("Wait timeout; never cancels the Run (default {}).", DEFAULT_RUN_WAIT.as_millis())
        )]
        timeout_ms: Option<std::time::Duration>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_open_snapshot_is_success_but_an_expired_wait_exits_three() {
        let run = serde_json::from_value(serde_json::json!({
            "id":"run-open", "repoPath":".", "stateDir":"state", "status":"WAITING_HUMAN",
            "createdAt":"2026-01-01T00:00:00Z", "selection":{"kind":"all"},
            "validation":null, "definitions":null
        }))
        .unwrap();
        assert_eq!(show_outcome(ShowMode::Snapshot, &run), 0);
        assert_eq!(
            show_outcome(
                ShowMode::Wait {
                    timeout: DEFAULT_RUN_WAIT
                },
                &run
            ),
            3
        );
    }
}
