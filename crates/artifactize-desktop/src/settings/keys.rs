//! API keys, and the models each backend lists.
use crate::{App, Failure};
use artifactize_core::{config::Backend, llm};
use serde::{Deserialize, Serialize};
use specta::Type;

/// A backend, as the CLI names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Provider {
    Openai,
    Anthropic,
    Codex,
}

impl From<Provider> for Backend {
    fn from(provider: Provider) -> Self {
        match provider {
            Provider::Openai => Backend::Openai,
            Provider::Anthropic => Backend::Anthropic,
            Provider::Codex => Backend::Codex,
        }
    }
}

/// Whether an API-key backend's key is in the environment; never its value.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct KeyStatus {
    pub provider: Provider,
    /// The environment variable the CLI reads.
    pub variable: String,
    pub present: bool,
}

/// The OpenAI and Anthropic keys, as `doctor` reports them.
#[tauri::command]
#[specta::specta]
pub fn settings_keys() -> Vec<KeyStatus> {
    [Provider::Openai, Provider::Anthropic]
        .into_iter()
        .map(|provider| {
            let backend = provider.into();
            KeyStatus {
                provider,
                variable: llm::key_variable(backend)
                    .expect("an API-key backend")
                    .into(),
                present: llm::key_present(backend),
            }
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub slug: String,
    pub display_name: String,
}

/// `models <provider>`: what the backend offers to this key or account.
#[tauri::command]
#[specta::specta]
pub async fn models_list(
    app: tauri::State<'_, App>,
    provider: Provider,
) -> Result<Vec<Model>, Failure> {
    let listing = llm::models::list(
        provider.into(),
        Some(&app.launch.state),
        app.launch.repo.as_deref(),
    )
    .await
    .map_err(Failure::Account)?;
    Ok(listing
        .models
        .into_iter()
        .map(|model| Model {
            slug: model.slug.to_string(),
            display_name: model.display_name,
        })
        .collect())
}
