//! Built-in providers and names of machine-configured backends.

use super::identifier;
use serde::{Deserialize, Serialize};

/// A built-in provider: one artifactize reaches with a sign-in or an API key and no other setup.
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
    /// The user’s own Google Antigravity CLI and its login (Chat only).
    Antigravity,
    /// The OpenAI API (OPENAI_API_KEY or a saved key).
    Openai,
    /// The Anthropic API (ANTHROPIC_API_KEY or a saved key).
    Anthropic,
    /// The Gemini API (GEMINI_API_KEY or a saved key).
    Gemini,
}

impl Provider {
    /// In the order Settings lists them.
    pub const ALL: [Self; 6] = [
        Self::Codex,
        Self::ClaudeAgent,
        Self::Antigravity,
        Self::Openai,
        Self::Anthropic,
        Self::Gemini,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Antigravity => "antigravity",
            Self::Anthropic => "anthropic",
            Self::ClaudeAgent => "claude-agent",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
            Self::Openai => "openai",
        }
    }

    pub(crate) fn validate_eval(self) -> Result<(), String> {
        match self {
            Self::Antigravity => Err("Antigravity is available for Chat only: Google's consumer plans don't cover automated evaluation and the CLI can't be confined to an eval's tools; use the `gemini` API-key backend".into()),
            _ => Ok(()),
        }
    }

    fn named(name: &str) -> Option<Self> {
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
}

impl From<Provider> for Backend {
    fn from(provider: Provider) -> Self {
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
