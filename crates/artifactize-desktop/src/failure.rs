//! The one error type commands return; the frontend narrows on `kind`.
use serde::Serialize;
use specta::Type;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type, thiserror::Error)]
#[serde(tag = "kind", content = "message", rename_all = "camelCase")]
pub enum Failure {
    /// Reading the state failed; the last snapshot stays on screen.
    #[error("{0}")]
    Read(String),
    /// A Human action was refused or failed; nothing was decided.
    #[error("{0}")]
    Action(String),
    /// Owner fields are not a valid result yet; the request stays waiting and correctable.
    #[error("{0}")]
    Invalid(String),
    /// What the call names is not loaded or no longer exists.
    #[error("{0}")]
    Missing(String),
    /// A sign-in, sign-out, listing or store call failed, with the CLI's own message.
    #[error("{0}")]
    Account(String),
}
