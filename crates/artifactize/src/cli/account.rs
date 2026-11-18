//! Provider sign-in, sign-out and model listing.

use std::io::{self, Write};

use artifactize_core::config::{Backend, Provider};
use serde_json::json;

use super::{Context, cancellation_listener, print_json};

pub(super) async fn login(context: Context, provider: Provider) -> Result<u8, String> {
    let (state, repo) = (context.state_dir.as_deref(), context.repo.as_deref());
    if provider == Provider::ClaudeAgent {
        return claude_agent_login(&context).await;
    }
    if provider != Provider::Codex {
        let key = super::secret::read(&format!("{provider} API key"))?.parse()?;
        artifactize_core::auth::providers::save_key(state, repo, provider, key).await?;
        if context.json {
            print_json(&json!({ "provider": provider, "saved": true }))?;
        } else {
            writeln!(io::stdout().lock(), "Saved the {provider} API key.")
                .map_err(|e| e.to_string())?;
        }
        return Ok(0);
    }
    artifactize_core::auth::codex::login(state, repo).await?;
    if context.json {
        print_json(&json!({ "provider": "codex", "signed_in": true }))?;
    } else {
        writeln!(io::stdout().lock(), "Signed in with Codex.").map_err(|e| e.to_string())?;
    }
    Ok(0)
}

/// Run Claude Code's own sign-in, then say how Claude Code reports it.
async fn claude_agent_login(context: &Context) -> Result<u8, String> {
    use artifactize_core::auth::claude_agent::{self, Status};
    let (cancellation, listener) = cancellation_listener()?;
    // An interrupt stops the sign-in, and with it Claude Code.
    let signed_in = tokio::select! {
        signed_in = claude_agent::login() => signed_in,
        () = cancellation.cancelled() => Err("Claude Code sign-in was cancelled.".to_owned()),
    };
    listener.abort();
    signed_in?;
    let status = claude_agent::status().await;
    if context.json {
        print_json(&json!({ "provider": Provider::ClaudeAgent, "signed_in": true }))?;
        return Ok(0);
    }
    let said = match status {
        Status::SignedIn { method, plan } => match plan {
            Some(plan) => format!("Claude Code is signed in with {method} ({plan})."),
            None => format!("Claude Code is signed in with {method}."),
        },
        _ => "Claude Code signed in.".to_owned(),
    };
    writeln!(io::stdout().lock(), "{said}").map_err(|e| e.to_string())?;
    Ok(0)
}

pub(super) async fn logout(context: Context, provider: Provider) -> Result<u8, String> {
    if provider == Provider::ClaudeAgent {
        artifactize_core::auth::claude_agent::logout().await?;
        if context.json {
            print_json(&json!({ "provider": provider, "signed_in": false }))?;
        } else {
            writeln!(io::stdout().lock(), "Signed out of Claude Code.")
                .map_err(|e| e.to_string())?;
        }
        return Ok(0);
    }
    if provider != Provider::Codex {
        artifactize_core::auth::providers::remove_key(
            context.state_dir.as_deref(),
            context.repo.as_deref(),
            provider,
        )
        .await?;
        if context.json {
            print_json(&json!({ "provider": provider, "saved": false }))?;
        } else {
            writeln!(io::stdout().lock(), "Forgot the saved {provider} API key.")
                .map_err(|e| e.to_string())?;
        }
        return Ok(0);
    }
    let revoked = artifactize_core::auth::codex::logout(
        context.state_dir.as_deref(),
        context.repo.as_deref(),
    )
    .await?;
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

pub(super) async fn models(context: Context, provider: Backend) -> Result<u8, String> {
    let listing = artifactize_core::llm::models::list(
        provider,
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
