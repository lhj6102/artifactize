//! Execution profiles and reasoning controls declared by Evals.

use super::{
    Backend, ModelId,
    location::Location,
    validation::{self, positive_integer, present, script, text, timeout},
};
use serde::{Deserialize, Serialize};

/// Declared effort is exact: unsupported backend efforts fail rather than remap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Reasoning {
    None,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}
impl Reasoning {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
            Self::Max => "max",
        }
    }
}
impl std::fmt::Display for Reasoning {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum Profile {
    Agent {
        backend: Backend,
        model: ModelId,
        #[serde(default, deserialize_with = "present")]
        reasoning: Option<Reasoning>,
        #[serde(
            rename(serialize = "timeoutMs", deserialize = "timeout_ms"),
            default,
            deserialize_with = "timeout",
            serialize_with = "validation::milliseconds::serialize"
        )]
        timeout_ms: Option<std::time::Duration>,
        #[serde(
            rename(serialize = "maxToolCalls", deserialize = "max_tool_calls"),
            default,
            deserialize_with = "positive_integer"
        )]
        max_tool_calls: Option<u64>,
        #[serde(
            rename(serialize = "maxTokens", deserialize = "max_tokens"),
            default,
            deserialize_with = "positive_integer"
        )]
        max_tokens: Option<u64>,
    },
    Human {},
    Runtime {
        #[serde(serialize_with = "crate::platform::serialize_program")]
        command: String,
        args: Vec<crate::scope::Argument>,
        #[serde(
            rename(serialize = "timeoutMs", deserialize = "timeout_ms"),
            default,
            deserialize_with = "timeout",
            serialize_with = "validation::milliseconds::serialize"
        )]
        timeout_ms: Option<std::time::Duration>,
    },
}

/// Fields that an `.artf` profile alias invocation (`!alias(field = value, ...)`) may override.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(super) struct AgentProfileOverrides {
    #[serde(default, deserialize_with = "present")]
    pub backend: Option<Backend>,
    #[serde(default, deserialize_with = "present")]
    pub model: Option<ModelId>,
    #[serde(default, deserialize_with = "present")]
    pub reasoning: Option<Reasoning>,
    #[serde(default, deserialize_with = "timeout")]
    pub timeout_ms: Option<std::time::Duration>,
    #[serde(default, deserialize_with = "positive_integer")]
    pub max_tool_calls: Option<u64>,
    #[serde(default, deserialize_with = "positive_integer")]
    pub max_tokens: Option<u64>,
}

impl Profile {
    pub(super) fn apply_overrides(&self, overrides: AgentProfileOverrides) -> Result<Self, String> {
        let Self::Agent {
            backend,
            model,
            reasoning,
            timeout_ms,
            max_tool_calls,
            max_tokens,
        } = self
        else {
            return Err("Profile aliases support only agent profiles (kind = \"agent\").".into());
        };
        Ok(Self::Agent {
            backend: overrides.backend.unwrap_or_else(|| backend.clone()),
            model: overrides.model.unwrap_or_else(|| model.clone()),
            reasoning: overrides.reasoning.or(*reasoning),
            timeout_ms: overrides.timeout_ms.or(*timeout_ms),
            max_tool_calls: overrides.max_tool_calls.or(*max_tool_calls),
            max_tokens: overrides.max_tokens.or(*max_tokens),
        })
    }

    pub(super) fn validate(&self, location: &Location<'_, '_>) -> Result<(), String> {
        match self {
            Self::Agent {
                backend,
                model,
                reasoning,
                ..
            } => {
                location
                    .child("model")
                    .check(text(model, "Agent profile model"))?;
                location.child("backend").check(backend.validate_eval())?;
                // A custom provider's API is known only where it runs, so its effort is
                // checked there.
                if let (Some(reasoning), Some(provider)) = (reasoning, backend.builtin()) {
                    location.child("reasoning").check(
                        crate::llm::Api::from(provider).validate_reasoning(reasoning.as_str()),
                    )?;
                }
                Ok(())
            }
            Self::Human {} => Ok(()),
            Self::Runtime { command, args, .. } => location.check(script(command, args)),
        }
    }

    pub fn kind(&self) -> ProfileKind {
        match self {
            Self::Agent { .. } => ProfileKind::Agent,
            Self::Human {} => ProfileKind::Human,
            Self::Runtime { .. } => ProfileKind::Runtime,
        }
    }
}

/// The kind of reviewer a profile names, without its execution options.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, clap::ValueEnum,
)]
#[serde(rename_all = "lowercase")]
pub enum ProfileKind {
    /// Runtime evals: a command's exit code.
    Runtime,
    /// Agent evals: a model's review.
    Agent,
    /// Human evals: a person's sign-off.
    Human,
}

impl ProfileKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Runtime => "runtime",
            Self::Agent => "agent",
            Self::Human => "human",
        }
    }
}
