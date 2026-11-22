//! The wire APIs artifactize speaks, and what each accepts.

use serde::{Deserialize, Serialize};

use crate::config::Provider;

/// The API a review's requests use: a built-in provider's, or a custom endpoint's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Api {
    OpenaiResponses,
    OpenaiChat,
    Anthropic,
    Gemini,
    Codex,
    /// The user's own Claude Code, run as a subprocess (`llm::claude_agent`).
    ClaudeAgent,
    Antigravity,
}

impl From<Provider> for Api {
    fn from(provider: Provider) -> Self {
        match provider {
            Provider::Openai => Self::OpenaiResponses,
            Provider::Anthropic => Self::Anthropic,
            Provider::Gemini => Self::Gemini,
            Provider::Codex => Self::Codex,
            Provider::Antigravity => Self::Antigravity,
            Provider::ClaudeAgent => Self::ClaudeAgent,
        }
    }
}

impl From<EndpointApi> for Api {
    fn from(api: EndpointApi) -> Self {
        match api {
            EndpointApi::OpenaiResponses => Self::OpenaiResponses,
            EndpointApi::OpenaiChat => Self::OpenaiChat,
            EndpointApi::Anthropic => Self::Anthropic,
            EndpointApi::Gemini => Self::Gemini,
        }
    }
}

impl Api {
    /// Declared effort is exact: an effort the API does not take fails rather than remaps.
    pub fn validate_reasoning(self, reasoning: &str) -> Result<(), String> {
        let supported = match self {
            Self::Antigravity => return Provider::Antigravity.validate_eval(),
            // Codex takes the Responses efforts as they are, as Pi's openai-codex does; Chat
            // Completions' `reasoning_effort` takes the same names.
            Self::OpenaiResponses | Self::OpenaiChat | Self::Codex => matches!(
                reasoning,
                "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
            ),
            Self::Anthropic => matches!(reasoning, "low" | "medium" | "high" | "max"),
            // Claude Code's `--effort` levels.
            Self::ClaudeAgent => matches!(reasoning, "low" | "medium" | "high" | "xhigh" | "max"),
            // Gemini 3's thinking levels.
            Self::Gemini => matches!(reasoning, "minimal" | "low" | "medium" | "high"),
        };
        if supported {
            Ok(())
        } else {
            Err(format!(
                "Unsupported reasoning {reasoning:?} for {self:?}; effort is never remapped."
            ))
        }
    }
}

/// Standard model-list APIs publish names but not the allowable request effort values.
/// Recognize the API's named families (including dated snapshots); unknown endpoints' model
/// IDs get no options rather than a guess. `None` leaves the default to the provider.
pub(super) fn model_efforts(
    api: Api,
    model: &str,
) -> (&'static [&'static str], Option<&'static str>) {
    const REASONING: &[&str] = &["low", "medium", "high"];
    const CLAUDE: &[&str] = &["low", "medium", "high", "max"];
    const OPENAI_NONE: &[&str] = &["none", "low", "medium", "high", "xhigh"];
    // Match a family or a dated snapshot, without assuming an unlisted variant has
    // the same capabilities.
    let named = |name: &str| {
        model == name
            || model.strip_prefix(name).is_some_and(|suffix| {
                suffix
                    .strip_prefix('-')
                    .is_some_and(|date| date.starts_with("20"))
            })
    };
    let family = |prefix: &str| model.starts_with(prefix);
    match api {
        Api::OpenaiResponses | Api::OpenaiChat
            if ["o1", "o3", "o3-mini", "o4-mini"].into_iter().any(named) =>
        {
            (REASONING, Some("medium"))
        }
        Api::OpenaiResponses if named("o3-pro") => (&["high"], Some("high")),
        Api::OpenaiResponses | Api::OpenaiChat
            if ["gpt-5", "gpt-5-mini", "gpt-5-nano"].into_iter().any(named) =>
        {
            (&["minimal", "low", "medium", "high"], Some("medium"))
        }
        Api::OpenaiResponses | Api::OpenaiChat if named("gpt-5.1") => {
            (&["none", "low", "medium", "high"], Some("none"))
        }
        Api::OpenaiResponses | Api::OpenaiChat
            if [
                "gpt-5.2",
                "gpt-5.4",
                "gpt-5.4-mini",
                "gpt-5.4-nano",
                "gpt-5.5",
            ]
            .into_iter()
            .any(named) =>
        {
            (OPENAI_NONE, Some("none"))
        }
        Api::OpenaiResponses
            if ["gpt-5.2-pro", "gpt-5.4-pro", "gpt-5.5-pro"]
                .into_iter()
                .any(named) =>
        {
            (&["medium", "high", "xhigh"], Some("medium"))
        }
        Api::Anthropic
            if ["claude-opus-4-6", "claude-opus-4-7", "claude-opus-4-8"]
                .into_iter()
                .any(named) =>
        {
            (CLAUDE, Some("high"))
        }
        Api::Anthropic if named("claude-sonnet-4-6") => (REASONING, Some("high")),
        Api::Gemini if family("gemini-3-pro-") => (&["low", "high"], Some("high")),
        Api::Gemini if family("gemini-3.1-pro-") => (REASONING, Some("high")),
        Api::Gemini if family("gemini-3-flash-") || family("gemini-3.1-flash-") => {
            (&["minimal", "low", "medium", "high"], None)
        }
        _ => (&[], None),
    }
}

/// The API a custom endpoint speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum EndpointApi {
    /// OpenAI Responses (`/responses`).
    OpenaiResponses,
    /// OpenAI Chat Completions (`/chat/completions`), which most compatible servers offer.
    OpenaiChat,
    /// Anthropic Messages (`/v1/messages`).
    Anthropic,
    /// Gemini GenerateContent.
    Gemini,
}
