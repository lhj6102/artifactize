//! The `claude-agent` backend: the user's own Claude Code, signed in with `claude auth login`,
//! runs each review. artifactize starts `claude -p` in stream-json mode, offers the eval's
//! tools as one MCP server it hosts in its own process, and reads the conversation back. It
//! never reads Claude Code's login, performs OAuth itself, or changes Claude Code's requests.

pub mod channel;
pub mod command;
pub mod mcp;
pub mod wire;

use std::time::Duration;

use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

pub use channel::MINIMUM_VERSION;

use super::models::ListedModel;
use crate::agent::error::Failure;

/// Bound one error text from Claude Code shown in output and saved records.
const SHOWN_CHARS: usize = 2000;
/// A probe starts a session and reads its `initialize` answer; Claude Code answers in a few
/// seconds, so a stalled one stops well before the thirty seconds a model listing may take.
const PROBE_TIMEOUT: Duration = Duration::from_secs(25);

/// Claude Code's text cleaned for a terminal and clipped.
pub(crate) fn shown(text: &str) -> String {
    let cleaned = super::clean_text(text.trim());
    match cleaned.char_indices().nth(SHOWN_CHARS) {
        Some((end, _)) => format!("{}…", &cleaned[..end]),
        None => cleaned,
    }
}

/// What a session's `initialize` reports, without sending a message: Claude Code's version,
/// the signed-in account and the models it offers.
pub struct Probe {
    pub version: Option<String>,
    pub account: Option<wire::Account>,
    pub models: Vec<wire::Model>,
}

/// Open a Claude Code session and read its `initialize` answer; no tokens are used.
pub async fn probe(cancellation: &CancellationToken) -> Result<Probe, Failure> {
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let cwd = crate::platform::temporary_directory();
    let (mut channel, running) =
        channel::start(&command::Launch::default(), &cwd, deadline, cancellation).await?;
    let initialized = tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(Failure::cancelled()),
        _ = tokio::time::sleep_until(deadline) => Err(Failure::timeout()),
        initialized = channel.initialize(None) => initialized,
    };
    let stderr = running.stop(channel).await;
    let initialized = initialized.map_err(|failure| with_stderr(failure, &stderr))?;
    Ok(Probe {
        version: initialized.claude_code_version,
        account: initialized.account,
        models: initialized.models,
    })
}

/// The models Claude Code offers, by the id `--model` takes, without a model call. An alias
/// such as `opus` lists the model it runs; `default` is listed only when no other entry runs
/// its model.
pub async fn models() -> Result<Vec<ListedModel>, String> {
    let probe = probe(&CancellationToken::new())
        .await
        .map_err(|failure| failure.message)?;
    Ok(listed(probe.models))
}

fn listed(models: Vec<wire::Model>) -> Vec<ListedModel> {
    let entries: Vec<(String, wire::Model)> = models
        .into_iter()
        .map(|model| {
            (
                model
                    .resolved_model
                    .clone()
                    .unwrap_or_else(|| model.value.clone()),
                model,
            )
        })
        .collect();
    let mut listed: Vec<ListedModel> = Vec::new();
    for (id, model) in &entries {
        if model.value == "default"
            && entries
                .iter()
                .any(|(other, entry)| other == id && entry.value != "default")
        {
            continue;
        }
        let Ok(slug) = id.parse::<crate::config::ModelId>() else {
            continue;
        };
        if listed.iter().any(|entry| entry.slug == slug) {
            continue;
        }
        listed.push(ListedModel {
            slug,
            display_name: model.display_name.clone().unwrap_or_else(|| id.clone()),
        });
    }
    listed
}

/// A failure with what Claude Code wrote on stderr, when it explains more.
pub(crate) fn with_stderr(failure: Failure, stderr: &str) -> Failure {
    if stderr.is_empty() {
        failure
    } else {
        Failure {
            message: format!("{} Claude Code: {stderr}", failure.message),
            ..failure
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(value: &str, resolved: &str, name: &str) -> wire::Model {
        wire::Model {
            value: value.into(),
            resolved_model: Some(resolved.into()),
            display_name: Some(name.into()),
        }
    }

    #[test]
    fn aliases_list_the_model_they_run_once() {
        let listed = listed(vec![
            model("default", "claude-opus-5-5", "Default (recommended)"),
            model("opus", "claude-opus-5-5", "Opus 5.5"),
            model("fable", "claude-fable-5-2", "Fable 5.2"),
            model("claude-fable-5-1", "claude-fable-5-1", "Fable 5.1"),
        ]);
        let listed: Vec<(String, String)> = listed
            .into_iter()
            .map(|model| (model.slug.to_string(), model.display_name))
            .collect();
        assert_eq!(
            listed,
            [
                ("claude-opus-5-5".to_owned(), "Opus 5.5".to_owned()),
                ("claude-fable-5-2".into(), "Fable 5.2".into()),
                ("claude-fable-5-1".into(), "Fable 5.1".into()),
            ]
        );
    }
}
