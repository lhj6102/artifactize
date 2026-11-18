//! Saved verification reports, Run outcome exit codes and shared output.

use std::io::{self, Write};

use super::FAILURE_EXIT;

/// Public exit 1 reports a RED verdict, not a command or runtime failure.
const RED_EXIT: u8 = 1;

/// Public exit 3 means the wait deadline elapsed without cancelling a Run; preserve the
/// established code so existing automation can distinguish waiting from an execution error.
pub(super) const WAIT_TIMEOUT_EXIT: u8 = 3;
/// Public exit 4 means validation is incomplete, rather than RED or an operational error;
/// retain the existing Run-outcome protocol used by verify and run show --wait.
const INCOMPLETE_EXIT: u8 = 4;
/// Distinct terminal outcome: all machine work passed, but real sign-offs remain.
const YELLOW_EXIT: u8 = 5;

pub(super) fn verify(
    view: &artifactize_core::store::RunView,
    json_output: bool,
    observed_at: time::OffsetDateTime,
) -> Result<(), String> {
    if json_output {
        print_json(&artifactize_core::query::run_output(view, observed_at))?;
    } else {
        let (summary, saved) = artifactize_core::query::run_summary(view, observed_at);
        let stopped = stopped_backend_lines(view);
        let mut stdout = io::stdout().lock();
        writeln!(
            stdout,
            "Run: {}\nExecution: {}\nState: {}",
            view.run.id,
            view.run.status(),
            artifactize_core::platform::path_text(&view.run.state_dir)
        )
        .map_err(|e| e.to_string())?;
        if let Some(comment) = &view.run.comment {
            writeln!(stdout, "Comment: {comment}").map_err(|e| e.to_string())?;
        }
        if let Some(error) = view.run.error() {
            writeln!(stdout, "Reason: {error}").map_err(|e| e.to_string())?;
        }
        for request in &view.requests {
            writeln!(
                stdout,
                "  {} [{}]: {}{}{}",
                request.eval_id,
                request.id,
                request.status(),
                reuse_marker(request),
                request
                    .error()
                    .or(request.blocked_reason.as_deref())
                    .map_or(String::new(), |reason| format!(" — {reason}"))
            )
            .map_err(|e| e.to_string())?;
        }
        for unreadable in &view.unreadable {
            writeln!(stdout, "  {unreadable}").map_err(|error| error.to_string())?;
        }
        for line in stopped {
            writeln!(stdout, "{line}").map_err(|e| e.to_string())?;
        }
        writeln!(
            stdout,
            "Summary: executed {}, reused {}{}",
            kinds(&summary.executed),
            kinds(&summary.reused.kinds),
            match summary.reused.other_profile {
                0 => String::new(),
                count => format!("; {count} produced by another profile"),
            }
        )
        .map_err(|e| e.to_string())?;
        let spent = &summary.usage.usage;
        if !spent.is_empty() || !saved.is_empty() {
            writeln!(
                stdout,
                "Usage: spent {}; saved {}",
                counters(spent),
                counters(&saved)
            )
            .map_err(|e| e.to_string())?;
        }
        writeln!(
            stdout,
            "Validation: {}",
            if view
                .run
                .validation
                .snapshot()
                .and_then(|v| v.satisfied.value())
                == Some(&true)
            {
                "SATISFIED"
            } else {
                "NOT SATISFIED"
            }
        )
        .map_err(|e| e.to_string())?;
        if let Some(obligations) = view
            .run
            .validation
            .snapshot()
            .and_then(|v| v.obligations.value())
        {
            for artifact in obligations {
                writeln!(stdout, "  Unmet obligation: {artifact}").map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

/// Summarize why work did not start after a backend stopped, in the saved stop order.
fn stopped_backend_lines(view: &artifactize_core::store::RunView) -> Vec<String> {
    view.run
        .stopped_backends
        .iter()
        .map(|stopped| {
            let skipped = view
                .requests
                .iter()
                .filter(|request| {
                    request.error_code()
                        == Some(artifactize_core::types::FailureCode::BackendStopped)
                        && request.options.backend.as_ref() == Some(&stopped.backend)
                })
                .count();
            format!(
                "Stopped backend {} after {} in {}: {skipped} review{} not started.",
                stopped.backend,
                stopped.error_code,
                stopped.eval_id,
                if skipped == 1 { "" } else { "s" },
            )
        })
        .collect()
}

/// Where a reused result came from: its source Run, plus the producer for a remote result,
/// or the reviewer and the authenticated publisher for a remote Human sign-off.
fn reuse_marker(request: &artifactize_core::store::Request) -> String {
    let Some(source) = artifactize_core::query::source(request) else {
        return String::new();
    };
    let profile = if request.profile == request.requested_profile {
        String::new()
    } else {
        format!(
            ", profile {}",
            artifactize_core::query::profile_name(&request.profile, &request.options)
        )
    };
    match &request.origin {
        None => format!(" (reused from {}{profile})", source.run_id),
        Some(origin) if request.profile.kind() == artifactize_core::config::ProfileKind::Human => {
            format!(
                " (reused from remote: Human sign-off by {}, published by {}, {})",
                request.reviewer.as_deref().unwrap_or("unknown"),
                origin.publisher,
                source.run_id
            )
        }
        Some(_) => format!(
            " (reused from remote: {}, {}{profile})",
            request
                .producer
                .as_ref()
                .map_or("unknown producer", |producer| producer.name.as_str()),
            source.run_id
        ),
    }
}

/// `N (runtime R, agent A, human H)` from a summary tally.
fn kinds(tally: &artifactize_core::query::Kinds) -> String {
    format!(
        "{} (runtime {}, agent {}, human {})",
        tally.total, tally.runtime, tally.agent, tally.human
    )
}

fn counters(totals: &std::collections::BTreeMap<String, u64>) -> String {
    let pairs: Vec<_> = totals
        .iter()
        .map(|(key, value)| format!("{key} {value}"))
        .collect();
    if pairs.is_empty() {
        "none".into()
    } else {
        pairs.join(", ")
    }
}

pub(super) fn tags(tags: &[String]) -> String {
    if tags.is_empty() {
        String::new()
    } else {
        format!(" [{}]", tags.join(", "))
    }
}

/// Run outcome exit codes shared by `verify` and `run show --wait`.
pub(super) fn outcome_code(run: &artifactize_core::store::Run) -> u8 {
    match run.status() {
        artifactize_core::types::RunStatus::Green => 0,
        artifactize_core::types::RunStatus::Red => RED_EXIT,
        artifactize_core::types::RunStatus::Incomplete => INCOMPLETE_EXIT,
        artifactize_core::types::RunStatus::Yellow => YELLOW_EXIT,
        artifactize_core::types::RunStatus::Running
        | artifactize_core::types::RunStatus::WaitingHuman
        | artifactize_core::types::RunStatus::Error => FAILURE_EXIT,
    }
}

pub(super) fn print_json(value: &impl serde::Serialize) -> Result<(), String> {
    // Buffered: a large document is written in blocks, not one write per line.
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    serde_json::to_writer_pretty(&mut stdout, value).map_err(|e| e.to_string())?;
    writeln!(stdout)
        .and_then(|()| stdout.flush())
        .map_err(|e| e.to_string())
}
