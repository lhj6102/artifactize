//! Payload-carrying lifecycle states; the legacy flat JSON is parsed at the edge.

use super::ExecutionResult;
use crate::types::{ExecutionStatus, FailureCode, RequestStatus, RunStatus, Timestamp};

/// Non-active, non-verdict request states historically allow an optional completion time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingRequest {
    Queued,
    Blocked,
    BudgetExhausted,
    Stale,
    Unreviewed,
    WaitDependency,
}

#[derive(Debug, Clone)]
pub enum ExecutionState {
    Running,
    WaitingHuman,
    Completed {
        result: ExecutionResult,
        at: Timestamp,
    },
    Failed {
        error: String,
        code: Option<FailureCode>,
        at: Timestamp,
    },
}

#[derive(Debug, Clone)]
pub enum RequestState {
    Running,
    WaitingHuman,
    Pending {
        kind: PendingRequest,
        at: Option<Timestamp>,
    },
    Completed {
        result: ExecutionResult,
        at: Timestamp,
    },
    Failed {
        error: String,
        code: Option<FailureCode>,
        at: Timestamp,
    },
}

impl RequestState {
    /// Construct a pending state without inventing result or error data.
    pub fn pending(status: RequestStatus, at: Option<Timestamp>) -> Result<Self, String> {
        Self::from_parts(status, None, None, None, at)
    }

    pub fn completed(result: ExecutionResult, at: Timestamp) -> Self {
        Self::Completed { result, at }
    }

    pub fn failed(error: String, code: Option<FailureCode>, at: Timestamp) -> Self {
        Self::Failed { error, code, at }
    }

    pub(super) fn from_parts(
        status: RequestStatus,
        result: Option<ExecutionResult>,
        error: Option<String>,
        code: Option<FailureCode>,
        at: Option<Timestamp>,
    ) -> Result<Self, String> {
        super::validation::result(
            status,
            result.as_ref(),
            error.as_deref(),
            code.map(FailureCode::as_str),
        )?;
        match status {
            RequestStatus::Green | RequestStatus::Red => Ok(Self::Completed {
                result: result.expect("validated verdict"),
                at: at.ok_or("A finished request needs its completion time.")?,
            }),
            RequestStatus::Error => Ok(Self::Failed {
                error: error.expect("validated error"),
                code,
                at: at.ok_or("A finished request needs its completion time.")?,
            }),
            RequestStatus::Running | RequestStatus::WaitingHuman if at.is_some() => {
                Err("An active request cannot have a completion time.".into())
            }
            RequestStatus::Running => Ok(Self::Running),
            RequestStatus::WaitingHuman => Ok(Self::WaitingHuman),
            _ => Ok(Self::Pending {
                kind: match status {
                    RequestStatus::Queued => PendingRequest::Queued,
                    RequestStatus::Blocked => PendingRequest::Blocked,
                    RequestStatus::BudgetExhausted => PendingRequest::BudgetExhausted,
                    RequestStatus::Stale => PendingRequest::Stale,
                    RequestStatus::Unreviewed => PendingRequest::Unreviewed,
                    RequestStatus::WaitDependency => PendingRequest::WaitDependency,
                    _ => unreachable!("terminal and active states handled above"),
                },
                at,
            }),
        }
    }

    pub fn status(&self) -> RequestStatus {
        match self {
            Self::Running => RequestStatus::Running,
            Self::WaitingHuman => RequestStatus::WaitingHuman,
            Self::Completed { result, .. } => match result.verdict() {
                crate::runtime::Verdict::Green => RequestStatus::Green,
                crate::runtime::Verdict::Red => RequestStatus::Red,
            },
            Self::Failed { .. } => RequestStatus::Error,
            Self::Pending { kind, .. } => match kind {
                PendingRequest::Queued => RequestStatus::Queued,
                PendingRequest::Blocked => RequestStatus::Blocked,
                PendingRequest::BudgetExhausted => RequestStatus::BudgetExhausted,
                PendingRequest::Stale => RequestStatus::Stale,
                PendingRequest::Unreviewed => RequestStatus::Unreviewed,
                PendingRequest::WaitDependency => RequestStatus::WaitDependency,
            },
        }
    }
    pub fn result(&self) -> Option<&ExecutionResult> {
        match self {
            Self::Completed { result, .. } => Some(result),
            _ => None,
        }
    }
    pub fn error(&self) -> Option<&str> {
        match self {
            Self::Failed { error, .. } => Some(error),
            _ => None,
        }
    }
    pub fn error_code(&self) -> Option<FailureCode> {
        match self {
            Self::Failed { code, .. } => *code,
            _ => None,
        }
    }
    pub fn completed_at(&self) -> Option<Timestamp> {
        match self {
            Self::Completed { at, .. } | Self::Failed { at, .. } => Some(*at),
            Self::Pending { at, .. } => *at,
            _ => None,
        }
    }
}

impl ExecutionState {
    pub(super) fn from_parts(
        status: ExecutionStatus,
        result: Option<ExecutionResult>,
        error: Option<String>,
        code: Option<FailureCode>,
        at: Option<Timestamp>,
    ) -> Result<Self, String> {
        let state = RequestState::from_parts(status.into(), result, error, code, at)?;
        Self::try_from(state)
    }
    pub fn status(&self) -> ExecutionStatus {
        match self {
            Self::Running => ExecutionStatus::Running,
            Self::WaitingHuman => ExecutionStatus::WaitingHuman,
            Self::Completed { result, .. } => match result.verdict() {
                crate::runtime::Verdict::Green => ExecutionStatus::Green,
                crate::runtime::Verdict::Red => ExecutionStatus::Red,
            },
            Self::Failed { .. } => ExecutionStatus::Error,
        }
    }
    pub fn result(&self) -> Option<&ExecutionResult> {
        match self {
            Self::Completed { result, .. } => Some(result),
            _ => None,
        }
    }
    pub fn error(&self) -> Option<&str> {
        match self {
            Self::Failed { error, .. } => Some(error),
            _ => None,
        }
    }
    pub fn error_code(&self) -> Option<FailureCode> {
        match self {
            Self::Failed { code, .. } => *code,
            _ => None,
        }
    }
    pub fn completed_at(&self) -> Option<Timestamp> {
        match self {
            Self::Completed { at, .. } | Self::Failed { at, .. } => Some(*at),
            _ => None,
        }
    }
}
impl TryFrom<RequestState> for ExecutionState {
    type Error = String;
    fn try_from(state: RequestState) -> Result<Self, String> {
        match state {
            RequestState::Running => Ok(Self::Running),
            RequestState::WaitingHuman => Ok(Self::WaitingHuman),
            RequestState::Completed { result, at } => Ok(Self::Completed { result, at }),
            RequestState::Failed { error, code, at } => Ok(Self::Failed { error, code, at }),
            RequestState::Pending { .. } => {
                Err("A pending request has no execution lifecycle state.".into())
            }
        }
    }
}
impl From<ExecutionState> for RequestState {
    fn from(state: ExecutionState) -> Self {
        match state {
            ExecutionState::Running => Self::Running,
            ExecutionState::WaitingHuman => Self::WaitingHuman,
            ExecutionState::Completed { result, at } => Self::Completed { result, at },
            ExecutionState::Failed { error, code, at } => Self::Failed { error, code, at },
        }
    }
}

/// Run completion data exists only in a finished lifecycle state.
#[derive(Debug, Clone)]
pub enum RunState {
    Running,
    Green {
        at: Timestamp,
    },
    Red {
        at: Timestamp,
    },
    Error {
        at: Timestamp,
        error: Option<String>,
    },
    Incomplete {
        at: Timestamp,
        error: Option<String>,
    },
}

impl RunState {
    pub(super) fn from_parts(
        status: RunStatus,
        at: Option<Timestamp>,
        error: Option<String>,
    ) -> Result<Self, String> {
        match status {
            RunStatus::Running if at.is_none() && error.is_none() => Ok(Self::Running),
            RunStatus::Running => {
                Err("A running Run cannot be completed or carry an error.".into())
            }
            RunStatus::Green | RunStatus::Red if error.is_some() || at.is_none() => Err(
                "A completed verdict Run needs a completion time and cannot carry an error.".into(),
            ),
            RunStatus::Green => Ok(Self::Green {
                at: at.expect("validated completion time"),
            }),
            RunStatus::Red => Ok(Self::Red {
                at: at.expect("validated completion time"),
            }),
            RunStatus::Error => Ok(Self::Error {
                at: at.ok_or("A finished Run needs its completion time.")?,
                error,
            }),
            RunStatus::Incomplete => Ok(Self::Incomplete {
                at: at.ok_or("A finished Run needs its completion time.")?,
                error,
            }),
        }
    }
    pub fn status(&self) -> RunStatus {
        match self {
            Self::Running => RunStatus::Running,
            Self::Green { .. } => RunStatus::Green,
            Self::Red { .. } => RunStatus::Red,
            Self::Error { .. } => RunStatus::Error,
            Self::Incomplete { .. } => RunStatus::Incomplete,
        }
    }
    pub fn completed_at(&self) -> Option<Timestamp> {
        match self {
            Self::Running => None,
            Self::Green { at }
            | Self::Red { at }
            | Self::Error { at, .. }
            | Self::Incomplete { at, .. } => Some(*at),
        }
    }
    pub fn error(&self) -> Option<&str> {
        match self {
            Self::Error { error, .. } | Self::Incomplete { error, .. } => error.as_deref(),
            _ => None,
        }
    }
}
