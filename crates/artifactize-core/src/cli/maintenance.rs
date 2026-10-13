//! Readiness, maintenance and account command workflows.

use super::{Context, cancellation_listener, print_json};
use serde_json::json;
use std::{
    io::{self, Write},
    path::PathBuf,
};

pub(super) async fn tools(
    context: Context,
    options: crate::diagnostics::ToolCheckOptions,
) -> Result<u8, String> {
    let (cancellation, listener) = cancellation_listener()?;
    let result = crate::diagnostics::check_tools(
        &context.repo.unwrap_or_else(|| PathBuf::from(".")),
        context.state_dir.as_deref(),
        &options,
        cancellation,
    )
    .await;
    listener.abort();
    let report = result?;
    print_json(&report)?;
    Ok(u8::from(!report.ok))
}

pub(super) async fn login(context: Context) -> Result<u8, String> {
    crate::auth::codex::login(context.state_dir.as_deref(), context.repo.as_deref()).await?;
    if context.json {
        print_json(&json!({ "provider": "codex", "signed_in": true }))?;
    } else {
        writeln!(io::stdout().lock(), "Signed in with Codex.").map_err(|e| e.to_string())?;
    }
    Ok(0)
}

pub(super) async fn logout(context: Context) -> Result<u8, String> {
    let revoked =
        crate::auth::codex::logout(context.state_dir.as_deref(), context.repo.as_deref()).await?;
    if !revoked {
        writeln!(
            io::stderr().lock(),
            "Local tokens removed; their revocation was not confirmed."
        )
        .map_err(|e| e.to_string())?;
    }
    if context.json {
        print_json(&json!({ "provider": "codex", "signed_in": false, "revoked": revoked }))?;
    } else {
        writeln!(io::stdout().lock(), "Signed out of Codex.").map_err(|e| e.to_string())?;
    }
    Ok(0)
}

pub(super) async fn doctor(context: Context) -> Result<u8, String> {
    let report =
        crate::diagnostics::doctor(context.state_dir.as_deref(), context.repo.as_deref()).await?;
    if context.json {
        print_json(&report)?;
    } else {
        let mut out = io::stdout().lock();
        writeln!(
            out,
            "State directory: {}",
            crate::platform::path_text(&report.state_dir)
        )
        .map_err(|e| e.to_string())?;
        for check in &report.checks {
            writeln!(out, "{} {}: {}", check.status, check.name, check.message)
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(u8::from(!report.ok))
}

pub(super) async fn prune(
    context: Context,
    older_than: Option<std::time::Duration>,
    dry_run: bool,
) -> Result<u8, String> {
    let state = match context.state_dir {
        Some(state) => state,
        None => crate::store::state_home().map_err(|e| e.to_string())?,
    };
    let report = crate::store::prune::prune(&state, context.repo.as_deref(), older_than, dry_run)?;
    if context.json {
        print_json(&report)?;
    } else {
        let mut out = io::stdout().lock();
        for path in &report.removed {
            writeln!(out, "Removed {}", crate::platform::path_text(path))
                .map_err(|e| e.to_string())?;
        }
        for path in &report.would_remove {
            writeln!(out, "Would remove {}", crate::platform::path_text(path))
                .map_err(|e| e.to_string())?;
        }
        for id in &report.skipped_runs {
            writeln!(out, "Skipped Run {id}").map_err(|e| e.to_string())?;
        }
        for id in &report.removed_sessions {
            writeln!(out, "Removed Agent session {id}").map_err(|e| e.to_string())?;
        }
        for id in &report.would_remove_sessions {
            writeln!(out, "Would remove Agent session {id}").map_err(|e| e.to_string())?;
        }
        writeln!(out, "Database audit and repository files were preserved.")
            .map_err(|e| e.to_string())?;
    }
    Ok(0)
}

pub(super) async fn models(context: Context, provider: super::ModelProvider) -> Result<u8, String> {
    let listing = crate::llm::models::list(
        provider.backend(),
        context.state_dir.as_deref(),
        context.repo.as_deref(),
    )
    .await?;
    if context.json {
        print_json(&listing)?;
    } else {
        let mut out = io::stdout().lock();
        for model in listing.models {
            writeln!(out, "{}\t{}", model.slug, model.display_name).map_err(|e| e.to_string())?;
        }
    }
    Ok(0)
}
