//! Every status the monitor shows, typed until it is drawn: the statuses of Runs, requests
//! and Artifact validation, in their urgency order.

use crate::types::{ArtifactValidationStatus, RequestStatus, RunStatus};

/// A shown status. The variants are declared most urgent first, so the derived order is
/// the urgency order the header, Scope, Runs and headline counts use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Status {
    Error,
    Red,
    Blocked,
    Running,
    WaitingHuman,
    Queued,
    BudgetExhausted,
    WaitDependency,
    Wait,
    Stale,
    Unreviewed,
    Incomplete,
    Deferred,
    Provisional,
    Yellow,
    Green,
    Basis,
}

impl Status {
    /// The status as saved and printed, such as `WAITING_HUMAN`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "ERROR",
            Self::Red => "RED",
            Self::Blocked => "BLOCKED",
            Self::Running => "RUNNING",
            Self::WaitingHuman => "WAITING_HUMAN",
            Self::Queued => "QUEUED",
            Self::BudgetExhausted => "BUDGET_EXHAUSTED",
            Self::WaitDependency => "WAIT_DEPENDENCY",
            Self::Wait => "WAIT",
            Self::Stale => "STALE",
            Self::Unreviewed => "UNREVIEWED",
            Self::Incomplete => "INCOMPLETE",
            Self::Deferred => "DEFERRED",
            Self::Provisional => "PROVISIONAL",
            Self::Yellow => "YELLOW",
            Self::Green => "GREEN",
            Self::Basis => "BASIS",
        }
    }

    /// The one-character mark the tree, counts and headline show for the status.
    pub fn glyph(self) -> &'static str {
        match self {
            Self::Green => "✓",
            Self::Red => "✗",
            Self::Error => "!",
            Self::Running => "◐",
            Self::WaitingHuman | Self::Deferred | Self::Yellow => "?",
            Self::Provisional => "○",
            Self::Queued => "·",
            Self::Blocked => "⊘",
            Self::WaitDependency | Self::Wait => "…",
            Self::BudgetExhausted => "$",
            Self::Basis => "◇",
            Self::Stale | Self::Unreviewed | Self::Incomplete => "○",
        }
    }

    /// What a request in this status means, for its detail.
    pub fn meaning(self) -> &'static str {
        match self {
            Self::Green => "criteria met",
            Self::Red => "criteria not met",
            Self::Error => "operational failure, not a verdict",
            Self::Running => "executing",
            Self::WaitingHuman => "waiting for a Human submission",
            Self::Deferred => "sign-off deferred; execution may proceed",
            Self::Provisional => "criteria met; dependent sign-offs remain",
            Self::Yellow => "ended with only sign-offs left",
            Self::Blocked => "blocked by a RED dependency",
            Self::WaitDependency => "waiting for current GREEN dependency evidence",
            Self::BudgetExhausted => "maxExecutions reached before it started",
            _ => "not executed",
        }
    }
}

impl std::fmt::Display for Status {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl From<RunStatus> for Status {
    fn from(status: RunStatus) -> Self {
        match status {
            RunStatus::Running => Self::Running,
            RunStatus::WaitingHuman => Self::WaitingHuman,
            RunStatus::Green => Self::Green,
            RunStatus::Red => Self::Red,
            RunStatus::Error => Self::Error,
            RunStatus::Incomplete => Self::Incomplete,
            RunStatus::Yellow => Self::Yellow,
        }
    }
}

impl From<RequestStatus> for Status {
    fn from(status: RequestStatus) -> Self {
        match status {
            RequestStatus::Queued => Self::Queued,
            RequestStatus::Running => Self::Running,
            RequestStatus::WaitingHuman => Self::WaitingHuman,
            RequestStatus::Deferred => Self::Deferred,
            RequestStatus::Provisional => Self::Provisional,
            RequestStatus::Green => Self::Green,
            RequestStatus::Red => Self::Red,
            RequestStatus::Error => Self::Error,
            RequestStatus::BudgetExhausted => Self::BudgetExhausted,
            RequestStatus::Stale => Self::Stale,
            RequestStatus::Unreviewed => Self::Unreviewed,
            RequestStatus::WaitDependency => Self::WaitDependency,
            RequestStatus::Blocked => Self::Blocked,
        }
    }
}

impl From<ArtifactValidationStatus> for Status {
    fn from(status: ArtifactValidationStatus) -> Self {
        match status {
            ArtifactValidationStatus::Basis => Self::Basis,
            ArtifactValidationStatus::Deferred => Self::Deferred,
            ArtifactValidationStatus::Provisional => Self::Provisional,
            ArtifactValidationStatus::Green => Self::Green,
            ArtifactValidationStatus::Incomplete => Self::Incomplete,
            ArtifactValidationStatus::Error => Self::Error,
            ArtifactValidationStatus::Red => Self::Red,
            ArtifactValidationStatus::Blocked => Self::Blocked,
            ArtifactValidationStatus::Wait => Self::Wait,
            ArtifactValidationStatus::Stale => Self::Stale,
            ArtifactValidationStatus::Unreviewed => Self::Unreviewed,
        }
    }
}
