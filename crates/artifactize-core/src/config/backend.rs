//! Built-in eval/direct providers, strict HTTP backend types, and machine-configured backend names.

use super::identifier;
use serde::{Deserialize, Serialize};

/// A built-in direct HTTP provider (API key or Codex OAuth): never an ACP CLI or Claude Code.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    clap::ValueEnum,
)]
#[serde(rename_all = "lowercase")]
pub enum HttpProvider {
    Codex,
    Openai,
    Anthropic,
    Gemini,
}

impl HttpProvider {
    pub const ALL: [Self; 4] = [Self::Codex, Self::Openai, Self::Anthropic, Self::Gemini];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Openai => "openai",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
        }
    }

    pub fn named(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|provider| provider.as_str() == name)
    }
}

impl From<HttpProvider> for Provider {
    fn from(provider: HttpProvider) -> Self {
        match provider {
            HttpProvider::Codex => Self::Codex,
            HttpProvider::Openai => Self::Openai,
            HttpProvider::Anthropic => Self::Anthropic,
            HttpProvider::Gemini => Self::Gemini,
        }
    }
}

impl TryFrom<Provider> for HttpProvider {
    type Error = String;

    fn try_from(provider: Provider) -> Result<Self, Self::Error> {
        match provider {
            Provider::Codex => Ok(Self::Codex),
            Provider::Openai => Ok(Self::Openai),
            Provider::Anthropic => Ok(Self::Anthropic),
            Provider::Gemini => Ok(Self::Gemini),
            Provider::ClaudeAgent => {
                Err("claude-agent runs through its native CLI, not a direct HTTP provider.".into())
            }
        }
    }
}

impl std::fmt::Display for HttpProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Built-in provider names rejected at parse time by [`CustomBackendName`].
const RESERVED_BUILTIN_NAMES: [&str; 6] = [
    "codex",
    "openai",
    "anthropic",
    "gemini",
    "claude-agent",
    "antigravity",
];

/// Custom HTTP backend identifier that rejects all built-in provider names
/// (`codex`, `openai`, `anthropic`, `gemini`, `claude-agent`, `antigravity`) at parse time.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct CustomBackendName(String);

impl CustomBackendName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::str::FromStr for CustomBackendName {
    type Err = String;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        if RESERVED_BUILTIN_NAMES.contains(&name) {
            return Err(format!(
                "{name} is a built-in provider name and cannot be used as a custom HTTP backend."
            ));
        }
        identifier(name, "custom backend")?;
        Ok(Self(name.to_owned()))
    }
}

impl<'de> Deserialize<'de> for CustomBackendName {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

impl std::fmt::Display for CustomBackendName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Direct HTTP backend for Chat (`ChatTarget::Direct`): either a built-in [`HttpProvider`] or a
/// machine-local custom HTTP provider in `$STATE/auth/providers.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DirectBackend {
    Builtin {
        provider: HttpProvider,
    },
    /// Resolved at runtime exclusively against `$STATE/auth/providers.json`
    /// `Custom::Endpoint` or `Custom::Alias(HttpProvider)`.
    Custom {
        name: CustomBackendName,
    },
}

impl DirectBackend {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Builtin { provider } => provider.as_str(),
            Self::Custom { name } => name.as_str(),
        }
    }

    pub fn to_backend(&self) -> Backend {
        match self {
            Self::Builtin { provider } => Backend::from(Provider::from(*provider)),
            Self::Custom { name } => Backend(name.as_str().to_owned()),
        }
    }
}

impl std::fmt::Display for DirectBackend {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A built-in eval/direct provider: one artifactize reaches with a sign-in, an API key, or
/// isolated Claude Code (`claude-agent`). Chat-only ACP agents (including Antigravity) live
/// under `crate::acp` and are never part of `Provider`.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    clap::ValueEnum,
)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    /// The ChatGPT/Codex account (artifactize login codex).
    Codex,
    /// The user's own Claude Code and its login (claude auth login).
    #[serde(rename = "claude-agent")]
    #[value(name = "claude-agent")]
    ClaudeAgent,
    /// The OpenAI API (OPENAI_API_KEY or a saved key).
    Openai,
    /// The Anthropic API (ANTHROPIC_API_KEY or a saved key).
    Anthropic,
    /// The Gemini API (GEMINI_API_KEY or a saved key).
    Gemini,
}

impl Provider {
    /// In the order Settings → Providers lists them.
    pub const ALL: [Self; 5] = [
        Self::Codex,
        Self::ClaudeAgent,
        Self::Openai,
        Self::Anthropic,
        Self::Gemini,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::ClaudeAgent => "claude-agent",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
            Self::Openai => "openai",
        }
    }

    pub fn named(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|provider| provider.as_str() == name)
    }
}

impl std::fmt::Display for Provider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Backends that 0.5.0 removed; a declaration naming one fails with its replacements.
const REMOVED_BACKENDS: [&str; 2] = ["chatgpt", "claude"];
/// Chat-only Antigravity backend name, statically rejected in `.artf` eval declarations.
pub(crate) const ANTIGRAVITY_BACKEND: &str = "antigravity";
/// Static refusal message when an eval declaration or override names `antigravity`.
pub(crate) const ANTIGRAVITY_EVAL_REFUSAL: &str = "Antigravity is available for Chat only: Google's consumer plans don't cover automated evaluation and the CLI can't be confined to an eval's tools; use the `gemini` API-key backend";

/// What an Agent profile's `backend` names: a built-in [`Provider`], or a custom provider
/// set up on the machine that runs the eval. The project only declares the name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Backend(String);

impl Backend {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The built-in provider this names, or `None` for a custom provider.
    pub fn builtin(&self) -> Option<Provider> {
        Provider::named(&self.0)
    }

    /// Statically reject Chat-only `antigravity` in `.artf` declarations and profile overrides.
    /// Other ACP preset names are NOT statically rejected here because v11 users may already
    /// have custom HTTP backends with those names in `providers.json`.
    pub(crate) fn validate_eval(&self) -> Result<(), String> {
        if self.0 == ANTIGRAVITY_BACKEND {
            Err(ANTIGRAVITY_EVAL_REFUSAL.to_owned())
        } else {
            Ok(())
        }
    }

    /// Whether this name is reserved and cannot be used for a custom HTTP provider.
    pub(crate) fn is_reserved_custom(&self) -> bool {
        RESERVED_BUILTIN_NAMES.contains(&self.0.as_str())
    }
}

impl From<Provider> for Backend {
    fn from(provider: Provider) -> Self {
        Self(provider.as_str().to_owned())
    }
}

impl From<HttpProvider> for Backend {
    fn from(provider: HttpProvider) -> Self {
        Self(provider.as_str().to_owned())
    }
}

impl std::str::FromStr for Backend {
    type Err = String;
    fn from_str(name: &str) -> Result<Self, Self::Err> {
        if REMOVED_BACKENDS.contains(&name) {
            return Err(format!(
                r#"backend "{name}" was removed in 0.5.0; use "openai", "anthropic" or "gemini" with an API key, "codex" with a ChatGPT/Codex sign-in, or "claude-agent" with your Claude Code login"#
            ));
        }
        identifier(name, "backend")?;
        Ok(Self(name.to_owned()))
    }
}

impl<'de> Deserialize<'de> for Backend {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

impl std::fmt::Display for Backend {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}
