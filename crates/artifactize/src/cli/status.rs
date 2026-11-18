//! Read and display the current workspace's verification status.

use std::{
    io::{self, Write},
    path::PathBuf,
};

use artifactize_core::project::{StatusView, selection::Selection};

use super::{Context, PolicyArgs, cancellation_listener, print_json, render::tags};

pub(super) async fn execute(
    context: Context,
    selection: Selection,
    policy: PolicyArgs,
) -> Result<u8, String> {
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
        render(&view).map_err(|error| error.to_string())?;
    }
    Ok(u8::from(!view.satisfied))
}

fn render(view: &StatusView) -> io::Result<()> {
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
                format!(" [file: {}]", artifact.path)
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
