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
}

impl From<Provider> for Api {
    fn from(provider: Provider) -> Self {
        match provider {
            Provider::Openai => Self::OpenaiResponses,
            Provider::Anthropic => Self::Anthropic,
            Provider::Gemini => Self::Gemini,
            Provider::Codex => Self::Codex,
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
            // Codex takes the Responses efforts as they are, as Pi's openai-codex does; Chat
            // Completions' `reasoning_effort` takes the same names.
            Self::OpenaiResponses | Self::OpenaiChat | Self::Codex => matches!(
                reasoning,
                "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
            ),
            Self::Anthropic => matches!(reasoning, "low" | "medium" | "high" | "max"),
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

    /// Whether final answers use OpenAI's structured-output subset (closed objects, every
    /// property required, numeric and array bounds, size limits) rather than the smaller
    /// keyword set Anthropic and Gemini share.
    pub(crate) fn openai_schema(self) -> bool {
        matches!(self, Self::OpenaiResponses | Self::OpenaiChat | Self::Codex)
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
