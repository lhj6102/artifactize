//! Text/JSON projections and Run outcome exit codes.

use std::io::{self, Write};

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
) -> Result<(), String> {
    if json_output {
        print_json(&artifactize_core::query::run_output(
            view,
            time::OffsetDateTime::now_utc(),
        ))?;
    } else {
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
        for stopped in &view.run.stopped_backends {
            let skipped = view
                .requests
                .iter()
                .filter(|request| {
                    request.error_code()
                        == Some(artifactize_core::types::FailureCode::BackendStopped)
                        && request.options.backend.as_ref() == Some(&stopped.backend)
                })
                .count();
            writeln!(
                stdout,
                "Stopped backend {} after {} in {}: {skipped} review{} not started.",
                stopped.backend,
                stopped.error_code,
                stopped.eval_id,
                if skipped == 1 { "" } else { "s" }
            )
            .map_err(|e| e.to_string())?;
        }
        let (summary, saved) =
            artifactize_core::query::run_summary(view, time::OffsetDateTime::now_utc());
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

fn tags(tags: &[String]) -> String {
    if tags.is_empty() {
        String::new()
    } else {
        format!(" [{}]", tags.join(", "))
    }
}

pub(super) fn status(view: &artifactize_core::project::StatusView) -> io::Result<()> {
    let mut out = io::stdout().lock();
    writeln!(
        out,
        "Current validation: {}",
        if view.satisfied {
            "SATISFIED"
        } else {
            "NOT SATISFIED"
        }
    )?;
    for unreadable in &view.unreadable {
        writeln!(out, "  {unreadable}")?;
    }
    for artifact in &view.artifacts {
        writeln!(
            out,
            "  Artifact {}{}{}: {} ({}/{} Evals)",
            artifact.id,
            tags(&artifact.tags),
            if artifact.kind == artifactize_core::config::ArtifactKind::File {
                format!(
                    " [file: {}]",
                    artifactize_core::platform::path_text(&artifact.path)
                )
            } else {
                String::new()
            },
            artifact.state,
            artifact.passed,
            artifact.total
        )?;
    }
    for eval in &view.evals {
        writeln!(
            out,
            "  {}: {} — {}{}\n    {}",
            eval.id,
            eval.state,
            eval.action,
            if eval.included {
                ""
            } else {
                " (not included; use --recursive)"
            },
            eval.reason
        )?;
        if let Some(changes) = &eval.changes {
            writeln!(
                out,
                "    Fingerprint changed since Run {}: {}",
                changes.since_run_id, changes.summary
            )?;
        }
        if let Some(last) = &eval.last {
            writeln!(
                out,
                "    Last: {} (Run {}; historical, not current evidence)",
                last.verdict, last.run_id
            )?;
        }
    }
    for artifact in &view.obligations {
        writeln!(out, "  Unmet obligation: {artifact}")?;
    }
    writeln!(
        out,
        "Verify actions: will execute {}, will reuse {}, wait {}, blocked {}",
        view.counts.execute, view.counts.reuse, view.counts.wait, view.counts.blocked
    )?;
    if view.counts.wait > 0 {
        writeln!(
            out,
            "  wait: needs a result verify has not produced yet (a dependency it executes, or a live execution); status cannot predict it."
        )?;
    }
    Ok(())
}

pub(super) fn graph(view: &artifactize_core::query::GraphView<'_>) -> io::Result<()> {
    let mut out = io::stdout().lock();
    writeln!(
        out,
        "Graph: {} Artifacts, {} Evals",
        view.artifacts.len(),
        view.evals.len()
    )?;
    for component in &view.components {
        writeln!(
            out,
            "  Component {}{}: {}",
            component.id,
            if component.cyclic { " [cycle]" } else { "" },
            component
                .artifacts
                .iter()
                .map(|id| id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )?;
        for id in &component.artifacts {
            let artifact = view.artifacts[id];
            writeln!(
                out,
                "    Artifact {id}{}{}{} ({})",
                tags(&artifact.tags),
                if artifact.kind == artifactize_core::config::ArtifactKind::File {
                    " [file]"
                } else {
                    ""
                },
                if artifact.basis == Some(true) {
                    " [basis]"
                } else {
                    ""
                },
                if artifact.path.as_os_str().is_empty() {
                    ".".into()
                } else {
                    artifactize_core::platform::path_text(&artifact.path)
                }
            )?;
            for eval in view.evals.iter().filter(|eval| eval.target == **id) {
                writeln!(
                    out,
                    "      {}: {} -> {}",
                    eval.id,
                    if eval.deps.is_empty() {
                        "(no deps)".into()
                    } else {
                        eval.deps.join(", ")
                    },
                    eval.target
                )?;
            }
        }
    }
    for edge in &view.relations {
        use artifactize_core::scope::RelationKind;
        let detail = match &edge.relation.kind {
            RelationKind::Child { path } => format!("child path={path}"),
            RelationKind::Instruction { eval_id, name } => {
                format!("instruction eval={eval_id} name={name}")
            }
            RelationKind::Argument {
                eval_id,
                index,
                name,
                path,
            } => format!("argv eval={eval_id} index={index} name={name} path={path}"),
        };
        writeln!(
            out,
            "  {} -> {} [{detail}]{}",
            edge.relation.source,
            edge.relation.target,
            if edge.cyclic { " [cycle]" } else { "" }
        )?;
    }
    Ok(())
}

/// Run outcome exit codes shared by `verify` and `run show --wait`.
pub(super) fn outcome_code(run: &artifactize_core::store::Run) -> u8 {
    match run.status() {
        artifactize_core::types::RunStatus::Green => 0,
        artifactize_core::types::RunStatus::Red => 1,
        artifactize_core::types::RunStatus::Incomplete => INCOMPLETE_EXIT,
        artifactize_core::types::RunStatus::Yellow => YELLOW_EXIT,
        artifactize_core::types::RunStatus::Running
        | artifactize_core::types::RunStatus::WaitingHuman
        | artifactize_core::types::RunStatus::Error => 2,
    }
}

pub(super) fn print_json(value: &impl serde::Serialize) -> Result<(), String> {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer_pretty(&mut stdout, value).map_err(|e| e.to_string())?;
    writeln!(stdout).map_err(|e| e.to_string())
}
