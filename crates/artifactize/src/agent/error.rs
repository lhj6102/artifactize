//! Why an Agent review failed, as a request `errorCode`.

use std::fmt;

/// The `errorCode` of a request whose Agent review failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    /// Credentials are missing, expired or rejected (HTTP 401 or 403).
    Authentication,
    /// A billing, credit or plan usage limit.
    Quota,
    /// Rate limited (HTTP 429) after the retries, or asked to wait past the deadline.
    RateLimit,
    /// A connection failure, an overloaded or failing server, or an interrupted stream,
    /// after the retries or once the turn had produced output.
    Transient,
    /// The review's `timeoutMs` deadline passed.
    Timeout,
    Cancelled,
    /// `maxTokens` or `maxToolCalls` was exceeded.
    ProviderBudgetExceeded,
    /// No valid verdict after the one format repair.
    InvalidResult,
    /// Any other provider failure: a rejected request, an unknown or different model,
    /// an incomplete response or a malformed tool call.
    ProviderError,
    /// Anything else, such as unusable configuration or tools.
    AgentError,
}

/// The `errorCode` of a review a Run did not start because its backend stopped.
pub const BACKEND_STOPPED: &str = "BACKEND_STOPPED";

impl Code {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Authentication => "AUTHENTICATION",
            Self::Quota => "QUOTA",
            Self::RateLimit => "RATE_LIMIT",
            Self::Transient => "TRANSIENT",
            Self::Timeout => "TIMEOUT",
            Self::Cancelled => "CANCELLED",
            Self::ProviderBudgetExceeded => "PROVIDER_BUDGET_EXCEEDED",
            Self::InvalidResult => "INVALID_RESULT",
            Self::ProviderError => "PROVIDER_ERROR",
            Self::AgentError => "AGENT_ERROR",
        }
    }

    /// Failures that a turn without output may retry.
    pub fn retryable(self) -> bool {
        matches!(self, Self::RateLimit | Self::Transient)
    }

    /// Failures that every later review on the backend would repeat: the Run stops
    /// admitting reviews on it.
    pub fn stops_backend(code: &str) -> bool {
        code == Self::Authentication.as_str() || code == Self::Quota.as_str()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub code: Code,
    pub message: String,
}

impl Failure {
    pub fn new(code: Code, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn cancelled() -> Self {
        Self::new(Code::Cancelled, "Agent review was cancelled.")
    }

    pub fn timeout() -> Self {
        Self::new(Code::Timeout, "Agent review timed out.")
    }
}

/// An unclassified failure, such as a tool or configuration error.
impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self::new(Code::AgentError, message)
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}
