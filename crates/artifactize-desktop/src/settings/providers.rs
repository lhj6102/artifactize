//! Providers: the built-in providers' keys, the custom providers of this machine, and the
//! models each lists, as `login`, `logout`, `provider` and `models` do.
use crate::{App, Failure};
use artifactize_core::{
    auth::providers::{self, KeyChange},
    config, llm,
};
use serde::{Deserialize, Serialize};
use specta::Type;

/// A built-in provider, by the name the CLI and an eval's `backend` give it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum Provider {
    Codex,
    ClaudeAgent,
    Antigravity,
    Openai,
    Anthropic,
    Gemini,
}

impl From<Provider> for config::Provider {
    fn from(provider: Provider) -> Self {
        match provider {
            Provider::Codex => Self::Codex,
            Provider::Antigravity => Self::Antigravity,
            Provider::ClaudeAgent => Self::ClaudeAgent,
            Provider::Openai => Self::Openai,
            Provider::Anthropic => Self::Anthropic,
            Provider::Gemini => Self::Gemini,
        }
    }
}

impl From<config::Provider> for Provider {
    fn from(provider: config::Provider) -> Self {
        match provider {
            config::Provider::Codex => Self::Codex,
            config::Provider::Antigravity => Self::Antigravity,
            config::Provider::ClaudeAgent => Self::ClaudeAgent,
            config::Provider::Openai => Self::Openai,
            config::Provider::Anthropic => Self::Anthropic,
            config::Provider::Gemini => Self::Gemini,
        }
    }
}

/// The API a custom endpoint speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum EndpointApi {
    OpenaiResponses,
    OpenaiChat,
    Anthropic,
    Gemini,
}

impl From<EndpointApi> for llm::EndpointApi {
    fn from(api: EndpointApi) -> Self {
        match api {
            EndpointApi::OpenaiResponses => Self::OpenaiResponses,
            EndpointApi::OpenaiChat => Self::OpenaiChat,
            EndpointApi::Anthropic => Self::Anthropic,
            EndpointApi::Gemini => Self::Gemini,
        }
    }
}

impl From<llm::EndpointApi> for EndpointApi {
    fn from(api: llm::EndpointApi) -> Self {
        match api {
            llm::EndpointApi::OpenaiResponses => Self::OpenaiResponses,
            llm::EndpointApi::OpenaiChat => Self::OpenaiChat,
            llm::EndpointApi::Anthropic => Self::Anthropic,
            llm::EndpointApi::Gemini => Self::Gemini,
        }
    }
}

/// Where a built-in provider's key comes from; never its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum KeySource {
    /// Its environment variable, which is used first.
    Environment,
    /// Saved in Settings or with `artifactize login`.
    Saved,
    Absent,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct KeyStatus {
    pub provider: Provider,
    /// The environment variable read first.
    pub variable: String,
    pub source: KeySource,
    /// Whether a key is saved here, also while the variable comes first.
    pub saved: bool,
}

/// What a custom provider reaches; an endpoint's key only as whether one is saved.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Target {
    Alias {
        provider: Provider,
    },
    Endpoint {
        api: EndpointApi,
        url: String,
        #[serde(default)]
        key: bool,
    },
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CustomProvider {
    pub name: String,
    pub target: Target,
}

/// The providers of this machine.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Providers {
    /// The built-in providers, in the order Settings lists them.
    pub builtin: Vec<Provider>,
    pub alias_targets: Vec<Provider>,
    pub keys: Vec<KeyStatus>,
    pub custom: Vec<CustomProvider>,
}

/// What saving a custom endpoint does with its key.
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(tag = "kind", content = "key", rename_all = "camelCase")]
pub enum KeyInput {
    /// Save this key; it is never returned.
    Set(String),
    /// Keep the key the endpoint already has.
    Keep,
    /// No key.
    None,
}

fn location(app: &App) -> (Option<&std::path::Path>, Option<&std::path::Path>) {
    (Some(&app.launch.state), app.launch.settings_repo())
}

/// `provider list`: the built-in keys and the custom providers, never a key's value.
#[tauri::command]
#[specta::specta]
pub fn providers_list(app: tauri::State<'_, App>) -> Result<Providers, Failure> {
    let (state, repo) = location(&app);
    let list = providers::list(state, repo).map_err(Failure::Account)?;
    Ok(Providers {
        builtin: config::Provider::ALL.into_iter().map(Into::into).collect(),
        alias_targets: list.alias_targets.into_iter().map(Into::into).collect(),
        keys: list
            .keys
            .into_iter()
            .map(|key| KeyStatus {
                provider: key.provider().into(),
                variable: key.variable().into(),
                source: match key.source() {
                    providers::KeySource::Environment => KeySource::Environment,
                    providers::KeySource::Saved => KeySource::Saved,
                    providers::KeySource::Absent => KeySource::Absent,
                },
                saved: key.saved(),
            })
            .collect(),
        custom: list
            .custom
            .into_iter()
            .map(|(name, custom)| CustomProvider {
                name: name.to_string(),
                target: match custom {
                    providers::CustomView::Alias { provider } => Target::Alias {
                        provider: provider.into(),
                    },
                    providers::CustomView::Endpoint { api, url, key } => Target::Endpoint {
                        api: api.into(),
                        url: url.as_str().to_owned(),
                        key,
                    },
                },
            })
            .collect(),
    })
}

/// `login <provider>`: save an API key. The key arrives in the IPC body only; it is never
/// logged, echoed or returned.
#[tauri::command]
#[specta::specta]
pub async fn provider_key_save(
    app: tauri::State<'_, App>,
    provider: Provider,
    key: String,
) -> Result<(), Failure> {
    let (state, repo) = location(&app);
    let key = key.parse().map_err(Failure::Invalid)?;
    providers::save_key(state, repo, provider.into(), key)
        .await
        .map_err(Failure::Account)
}

/// `logout <provider>`: forget the saved key; the environment variable is untouched.
#[tauri::command]
#[specta::specta]
pub async fn provider_key_remove(
    app: tauri::State<'_, App>,
    provider: Provider,
) -> Result<(), Failure> {
    let (state, repo) = location(&app);
    providers::remove_key(state, repo, provider.into())
        .await
        .map_err(Failure::Account)
}

/// `provider add`: add the custom provider `name`, or replace it.
#[tauri::command]
#[specta::specta]
pub async fn provider_custom_save(
    app: tauri::State<'_, App>,
    name: String,
    target: Target,
    key: KeyInput,
) -> Result<(), Failure> {
    let (state, repo) = location(&app);
    let name: config::Backend = name.trim().parse().map_err(Failure::Invalid)?;
    let custom = match target {
        Target::Alias { provider } => providers::Custom::Alias {
            provider: provider.into(),
        },
        Target::Endpoint { api, url, .. } => providers::Custom::Endpoint {
            api: api.into(),
            url: url.parse().map_err(Failure::Invalid)?,
            key: None,
        },
    };
    let key = match key {
        KeyInput::Set(key) => KeyChange::Set(key.parse().map_err(Failure::Invalid)?),
        KeyInput::Keep => KeyChange::Keep,
        KeyInput::None => KeyChange::None,
    };
    providers::save_custom(state, repo, name, custom, key)
        .await
        .map_err(Failure::Account)
}

/// `provider remove`: remove the custom provider `name`.
#[tauri::command]
#[specta::specta]
pub async fn provider_custom_remove(
    app: tauri::State<'_, App>,
    name: String,
) -> Result<(), Failure> {
    let (state, repo) = location(&app);
    let name: config::Backend = name.parse().map_err(Failure::Invalid)?;
    providers::remove_custom(state, repo, &name)
        .await
        .map_err(Failure::Account)
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub slug: String,
    pub display_name: String,
    pub efforts: Vec<String>,
    pub default_effort: Option<String>,
}

/// Model discovery reports unavailable setup separately from a failed listing.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ModelsResult {
    Models {
        models: Vec<Model>,
    },
    NotSetUp {
        reason: String,
    },
    Failed {
        error: String,
    },
    CliMissing {
        reason: String,
    },
    CliTooOld {
        version: String,
        required_version: String,
        error: String,
    },
}

/// `models <provider>`: this provider's models or why it could not list them.
#[tauri::command]
#[specta::specta]
pub async fn models_list(
    app: tauri::State<'_, App>,
    provider: String,
) -> Result<ModelsResult, Failure> {
    use llm::models::Availability as A;
    let backend = provider.parse().map_err(Failure::Invalid)?;
    Ok(
        match llm::models::availability(
            backend,
            Some(&app.launch.state),
            app.launch.settings_repo(),
        )
        .await
        {
            A::Models(models) => ModelsResult::Models {
                models: models
                    .into_iter()
                    .map(|model| Model {
                        slug: model.slug.to_string(),
                        display_name: model.display_name,
                        efforts: model.efforts,
                        default_effort: model.default_effort,
                    })
                    .collect(),
            },
            A::NotSetUp { reason } => ModelsResult::NotSetUp { reason },
            A::Failed { error } => ModelsResult::Failed { error },
            A::CliMissing { reason } => ModelsResult::CliMissing { reason },
            A::CliTooOld {
                version,
                required_version,
                error,
            } => ModelsResult::CliTooOld {
                version: version.to_string(),
                required_version: required_version.to_string(),
                error,
            },
        },
    )
}
