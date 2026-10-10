//! Text/JSON projections and Run outcome exit codes.

use serde_json::json;
use std::io::{self, Write};

pub(super) fn verify(view: &crate::store::RunView, json_output: bool) -> Result<(), String> {
    if json_output {
        print_json(&crate::query::run_output(
            view,
            time::OffsetDateTime::now_utc(),
        ))?;
    } else {
        let mut stdout = io::stdout().lock();
        writeln!(
            stdout,
            "Run: {}\nExecution: {}\nState: {}",
            view.run.id,
            view.run.status,
            view.run.state_dir.display()
        )
        .map_err(|e| e.to_string())?;
        if let Some(error) = &view.run.error {
            writeln!(stdout, "Reason: {error}").map_err(|e| e.to_string())?;
        }
        for request in &view.requests {
            writeln!(
                stdout,
                "  {} [{}]: {}{}{}",
                request.eval_id,
                request.id,
                request.status,
                reuse_marker(request),
                request
                    .error
                    .as_ref()
                    .or(request.blocked_reason.as_ref())
                    .map_or(String::new(), |reason| format!(" — {reason}"))
            )
            .map_err(|e| e.to_string())?;
        }
        for stopped in &view.run.stopped_backends {
            let skipped = view
                .requests
                .iter()
                .filter(|request| {
                    request.error_code.as_deref() == Some(crate::agent::error::BACKEND_STOPPED)
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
        let output = crate::query::run_output(view, time::OffsetDateTime::now_utc());
        let summary = &output["summary"];
        writeln!(
            stdout,
            "Summary: executed {}, reused {}{}",
            kinds(&summary["executed"]),
            kinds(&summary["reused"]),
            match summary["reused"]["otherProfile"].as_u64() {
                Some(0) | None => String::new(),
                Some(count) => format!("; {count} produced by another profile"),
            }
        )
        .map_err(|e| e.to_string())?;
        if let Some(count) = summary["derived"].as_u64().filter(|count| *count > 0) {
            writeln!(stdout, "Derived: {count} dependency evals (no execution).")
                .map_err(|e| e.to_string())?;
        }
        let usage = &output["usage"];
        if *usage != json!({"spent":{},"saved":{}}) {
            writeln!(
                stdout,
                "Usage: spent {}; saved {}",
                counters(&usage["spent"]),
                counters(&usage["saved"])
            )
            .map_err(|e| e.to_string())?;
        }
        writeln!(
            stdout,
            "Validation: {}",
            if view.run.validation["satisfied"] == true {
                "SATISFIED"
            } else {
                "NOT SATISFIED"
            }
        )
        .map_err(|e| e.to_string())?;
        if let Some(obligations) = view.run.validation["obligations"].as_array() {
            for artifact in obligations.iter().filter_map(serde_json::Value::as_str) {
                writeln!(stdout, "  Unmet obligation: {artifact}").map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

/// Where a reused result came from: its source Run, plus the producer for a remote result,
/// or the reviewer and the authenticated publisher for a remote Human sign-off.
fn reuse_marker(request: &crate::store::Request) -> String {
    if request.profile.kind() == crate::config::ProfileKind::Dependency {
        return " (derived)".into();
    }
    let Some(source) = crate::query::source(request) else {
        return String::new();
    };
    let profile = if request.profile == request.requested_profile {
        String::new()
    } else {
        format!(
            ", profile {}",
            crate::query::profile_name(&request.profile, &request.options)
        )
    };
    match &request.origin {
        None => format!(" (reused from {}{profile})", source.run_id),
        Some(origin) if request.profile.kind() == crate::config::ProfileKind::Human => format!(
            " (reused from remote: Human sign-off by {}, published by {}, {})",
            request.reviewer.as_deref().unwrap_or("unknown"),
            origin.publisher,
            source.run_id
        ),
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
fn kinds(tally: &serde_json::Value) -> String {
    format!(
        "{} (runtime {}, agent {}, human {})",
        tally["total"], tally["runtime"], tally["agent"], tally["human"]
    )
}

fn counters(totals: &serde_json::Value) -> String {
    let pairs: Vec<_> = totals
        .as_object()
        .into_iter()
        .flatten()
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

pub(super) fn status(view: &crate::project::StatusView) -> io::Result<()> {
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
    for artifact in &view.artifacts {
        writeln!(
            out,
            "  Artifact {}{}{}: {} ({}/{} Evals)",
            artifact.id,
            tags(&artifact.tags),
            if artifact.kind == crate::config::ArtifactKind::File {
                format!(" [file: {}]", crate::platform::path_text(&artifact.path))
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
    if view.counts.derive > 0 {
        writeln!(
            out,
            "  derive {} dependency evals from current evidence (no execution or reuse).",
            view.counts.derive
        )?;
    }
    if view.counts.wait > 0 {
        writeln!(
            out,
            "  wait: needs a result verify has not produced yet (a dependency it executes, or a live execution); status cannot predict it."
        )?;
    }
    Ok(())
}

pub(super) fn graph(view: &crate::query::GraphView<'_>) -> io::Result<()> {
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
            component.artifacts.join(", ")
        )?;
        for id in &component.artifacts {
            let artifact = view.artifacts[id];
            writeln!(
                out,
                "    Artifact {id}{}{}{} ({})",
                tags(&artifact.tags),
                if artifact.kind == crate::config::ArtifactKind::File {
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
                    artifact.path.display().to_string()
                }
            )?;
            for eval in view.evals.iter().filter(|eval| eval.target == *id) {
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
        use crate::scope::RelationKind;
        let detail = match &edge.relation.kind {
            RelationKind::Child { path } => format!("child path={path}"),
            RelationKind::Mount { alias } => format!("mount alias={alias}"),
            RelationKind::Dependency { eval_id, name } => {
                format!("dependency eval={eval_id} name={name}")
            }
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
pub(super) fn outcome_code(run: &crate::store::Run) -> u8 {
    if run.wait_timed_out {
        return 3;
    }
    match run.status.as_str() {
        "GREEN" => 0,
        "RED" => 1,
        "INCOMPLETE" => 4,
        _ => 2,
    }
}

pub(super) fn print_json(value: &impl serde::Serialize) -> Result<(), String> {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer_pretty(&mut stdout, value).map_err(|e| e.to_string())?;
    writeln!(stdout).map_err(|e| e.to_string())
}
