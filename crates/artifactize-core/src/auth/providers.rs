//! API keys saved for the built-in providers and the custom providers of this machine, in
//! `$STATE/auth/providers.json` beside the other credentials. A project only names a
//! provider in `backend`; what the name reaches is set up here, by the person who runs it.

use std::{collections::BTreeMap, path::Path};

use serde::{Deserialize, Serialize};
use url::Url;

use super::storage::{Location, Storage, Tokens};
use crate::{
    config::{Backend, Provider},
    llm::EndpointApi,
};

const FILE: &str = "providers.json";
/// Serialize read-modify-write updates across processes, as Codex refreshes are.
const LOCK: &str = "providers";
/// Bound a pasted key: provider keys are a few hundred bytes at most.
const MAX_KEY_BYTES: usize = 4096;

/// An API key; never printed, logged or returned to a caller outside this crate.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ApiKey(String);

impl ApiKey {
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for ApiKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ApiKey(…)")
    }
}

impl std::str::FromStr for ApiKey {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let key = value.trim();
        if (1..=MAX_KEY_BYTES).contains(&key.len())
            && key.bytes().all(|byte| byte.is_ascii_graphic())
        {
            Ok(Self(key.to_owned()))
        } else {
            Err("An API key is 1–4096 printable ASCII characters without spaces.".into())
        }
    }
}

/// A custom endpoint's API root: http(s), without credentials, a query or a fragment, which
/// belong in its key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct EndpointUrl(String);

impl EndpointUrl {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::str::FromStr for EndpointUrl {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let url = Url::parse(value.trim()).map_err(|_| format!("{value:?} is not a URL."))?;
        if !matches!(url.scheme(), "http" | "https") || url.host().is_none() {
            return Err("An endpoint URL uses http:// or https:// and names a host.".into());
        }
        if !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(
                "An endpoint URL has no credentials, query or fragment; the key goes in its own field."
                    .into(),
            );
        }
        Ok(Self(url.as_str().trim_end_matches('/').to_owned()))
    }
}

impl<'de> Deserialize<'de> for EndpointUrl {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// What a custom provider reaches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum Custom {
    /// A built-in provider under another name, with its sign-in or key.
    Alias { provider: Provider },
    /// An API at a URL, with an optional key.
    Endpoint {
        api: EndpointApi,
        url: EndpointUrl,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        key: Option<ApiKey>,
    },
}

/// A custom provider as Settings and `provider list` show it: never its key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum CustomView {
    Alias {
        provider: Provider,
    },
    Endpoint {
        api: EndpointApi,
        url: EndpointUrl,
        key: bool,
    },
}

impl From<&Custom> for CustomView {
    fn from(custom: &Custom) -> Self {
        match custom {
            Custom::Alias { provider } => Self::Alias {
                provider: *provider,
            },
            Custom::Endpoint { api, url, key } => Self::Endpoint {
                api: *api,
                url: url.clone(),
                key: key.is_some(),
            },
        }
    }
}

/// Where a built-in provider's key comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum KeySource {
    Environment,
    Saved,
    Absent,
}

/// A built-in API-key provider's key, as Settings and `doctor` show it.
#[derive(Debug, Clone, Serialize)]
pub struct KeyStatus {
    pub provider: Provider,
    /// The environment variable read first.
    pub variable: &'static str,
    pub source: KeySource,
}

/// The providers this machine has set up.
#[derive(Debug, Clone, Serialize)]
pub struct Providers {
    pub keys: Vec<KeyStatus>,
    pub custom: BTreeMap<Backend, CustomView>,
    /// Why nothing can be saved in this state, which holds nothing yet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refused: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    keys: BTreeMap<Provider, ApiKey>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    custom: BTreeMap<Backend, Custom>,
}

/// The environment variable that holds an API-key provider's key; `None` for Codex, which
/// signs in instead.
pub fn key_variable(provider: Provider) -> Option<&'static str> {
    match provider {
        Provider::Openai => Some("OPENAI_API_KEY"),
        Provider::Anthropic => Some("ANTHROPIC_API_KEY"),
        Provider::Gemini => Some("GEMINI_API_KEY"),
        Provider::Codex => None,
    }
}

/// The key in `provider`'s variable. A set variable is the key in use: an invalid value is
/// an error, never a reason to fall back to a saved key from another account.
fn environment_key(provider: Provider) -> Option<Result<ApiKey, String>> {
    let variable = key_variable(provider)?;
    let value = crate::platform::environment::var_text(variable)?;
    Some(
        value
            .parse()
            .map_err(|error| format!("{variable} is set but invalid: {error}")),
    )
}

/// The saved keys and custom providers, and why none can be saved here. Environment keys
/// work in any state, so refused storage is an error only once it holds something.
fn read(state: Option<&Path>, repo: Option<&Path>) -> Result<(Saved, Option<String>), String> {
    let location = Location::find(state, repo)?;
    if let Some(refusal) = location.refusal(Tokens::Providers) {
        return match crate::platform::entry_exists(&location.directory.join(FILE)) {
            Ok(false) => Ok((Saved::default(), Some(refusal))),
            _ => Err(refusal),
        };
    }
    let saved = Storage::inspect(state, repo, Tokens::Providers)?
        .read(FILE)?
        .unwrap_or_default();
    Ok((saved, None))
}

/// What `state` has set up, without any key.
pub fn list(state: Option<&Path>, repo: Option<&Path>) -> Result<Providers, String> {
    let (saved, refused) = read(state, repo)?;
    Ok(Providers {
        keys: Provider::ALL
            .into_iter()
            .filter_map(|provider| {
                let variable = key_variable(provider)?;
                let source = if environment_key(provider).is_some() {
                    KeySource::Environment
                } else if saved.keys.contains_key(&provider) {
                    KeySource::Saved
                } else {
                    KeySource::Absent
                };
                Some(KeyStatus {
                    provider,
                    variable,
                    source,
                })
            })
            .collect(),
        custom: saved
            .custom
            .iter()
            .map(|(name, custom)| (name.clone(), custom.into()))
            .collect(),
        refused,
    })
}

/// A built-in provider's key: its environment variable first, then the saved key.
pub(crate) fn key(
    provider: Provider,
    state: Option<&Path>,
    repo: Option<&Path>,
) -> Result<Option<ApiKey>, String> {
    if let Some(key) = environment_key(provider) {
        return key.map(Some);
    }
    Ok(read(state, repo)?.0.keys.remove(&provider))
}

/// The custom provider `name`, if this machine has one.
pub(crate) fn custom(
    name: &Backend,
    state: Option<&Path>,
    repo: Option<&Path>,
) -> Result<Option<Custom>, String> {
    Ok(read(state, repo)?.0.custom.remove(name))
}

async fn update(
    state: Option<&Path>,
    repo: Option<&Path>,
    change: impl FnOnce(&mut Saved) -> Result<(), String>,
) -> Result<(), String> {
    let storage = Storage::new(state, repo, Tokens::Providers)?;
    let _lock = storage.lock(LOCK).await?;
    let mut saved: Saved = storage.read(FILE)?.unwrap_or_default();
    change(&mut saved)?;
    storage.save(FILE, &saved)
}

/// Save `key` for an API-key provider.
pub async fn save_key(
    state: Option<&Path>,
    repo: Option<&Path>,
    provider: Provider,
    key: ApiKey,
) -> Result<(), String> {
    if key_variable(provider).is_none() {
        return Err(format!(
            "{provider} signs in instead of using a key: artifactize login {provider}."
        ));
    }
    update(state, repo, |saved| {
        saved.keys.insert(provider, key);
        Ok(())
    })
    .await
}

/// Forget the saved key of `provider`; its environment variable is untouched.
pub async fn remove_key(
    state: Option<&Path>,
    repo: Option<&Path>,
    provider: Provider,
) -> Result<(), String> {
    update(state, repo, |saved| {
        saved.keys.remove(&provider);
        Ok(())
    })
    .await
}

/// How a custom provider is saved: its key is replaced, kept from the provider it replaces,
/// or left out.
pub enum KeyChange {
    Set(ApiKey),
    Keep,
    None,
}

/// Add the custom provider `name`, or replace it.
pub async fn save_custom(
    state: Option<&Path>,
    repo: Option<&Path>,
    name: Backend,
    mut custom: Custom,
    key: KeyChange,
) -> Result<(), String> {
    if let Some(provider) = name.builtin() {
        return Err(format!(
            "{provider} is a built-in provider; give the custom provider another name."
        ));
    }
    update(state, repo, |saved| {
        if let Custom::Endpoint { key: slot, .. } = &mut custom {
            *slot = match key {
                KeyChange::Set(key) => Some(key),
                KeyChange::Keep => match saved.custom.get(&name) {
                    Some(Custom::Endpoint { key, .. }) => key.clone(),
                    _ => None,
                },
                KeyChange::None => None,
            };
        }
        saved.custom.insert(name, custom);
        Ok(())
    })
    .await
}

/// Remove the custom provider `name`.
pub async fn remove_custom(
    state: Option<&Path>,
    repo: Option<&Path>,
    name: &Backend,
) -> Result<(), String> {
    update(state, repo, |saved| {
        saved
            .custom
            .remove(name)
            .map(drop)
            .ok_or_else(|| format!("No custom provider is named {name}."))
    })
    .await
}
