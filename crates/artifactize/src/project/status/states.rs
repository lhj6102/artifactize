//! Current-validation output states, distinct from saved Run/request verdicts.
use crate::graph::{ArtifactStatus, EvalStatus};
use serde::Serialize;
use std::fmt;

// Variant order preserves the former BTreeMap<&str, _> lexicographic count output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ArtifactCondition {
    Basis,
    Blocked,
    Error,
    Incomplete,
    Pass,
    Red,
    Stale,
    Unreviewed,
    WaitDependency,
}
impl From<ArtifactStatus> for ArtifactCondition {
    fn from(status: ArtifactStatus) -> Self {
        match status {
            ArtifactStatus::Basis => Self::Basis,
            ArtifactStatus::Green => Self::Pass,
            ArtifactStatus::Incomplete => Self::Incomplete,
            ArtifactStatus::Error => Self::Error,
            ArtifactStatus::Red => Self::Red,
            ArtifactStatus::Blocked => Self::Blocked,
            ArtifactStatus::Wait => Self::WaitDependency,
            ArtifactStatus::Stale => Self::Stale,
            ArtifactStatus::Unreviewed => Self::Unreviewed,
        }
    }
}
impl fmt::Display for ArtifactCondition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Basis => "BASIS",
            Self::Blocked => "BLOCKED",
            Self::Error => "ERROR",
            Self::Incomplete => "INCOMPLETE",
            Self::Pass => "PASS",
            Self::Red => "RED",
            Self::Stale => "STALE",
            Self::Unreviewed => "UNREVIEWED",
            Self::WaitDependency => "WAIT_DEPENDENCY",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EvalCondition {
    Blocked,
    Error,
    Pass,
    Red,
    Stale,
    Unreviewed,
    WaitDependency,
    WaitingHuman,
}
impl From<EvalStatus> for EvalCondition {
    fn from(status: EvalStatus) -> Self {
        match status {
            EvalStatus::Green => Self::Pass,
            EvalStatus::Red => Self::Red,
            EvalStatus::Error => Self::Error,
            EvalStatus::Stale => Self::Stale,
            EvalStatus::Unreviewed => Self::Unreviewed,
            EvalStatus::Wait => Self::WaitDependency,
            EvalStatus::Blocked => Self::Blocked,
        }
    }
}
impl fmt::Display for EvalCondition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Blocked => "BLOCKED",
            Self::Error => "ERROR",
            Self::Pass => "PASS",
            Self::Red => "RED",
            Self::Stale => "STALE",
            Self::Unreviewed => "UNREVIEWED",
            Self::WaitDependency => "WAIT_DEPENDENCY",
            Self::WaitingHuman => "WAITING_HUMAN",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VerifyAction {
    Execute,
    Derive,
    Reuse,
    Wait,
    Blocked,
}
impl fmt::Display for VerifyAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Execute => "execute",
            Self::Derive => "derive",
            Self::Reuse => "reuse",
            Self::Wait => "wait",
            Self::Blocked => "blocked",
        })
    }
}
