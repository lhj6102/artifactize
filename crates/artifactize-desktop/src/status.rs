//! The monitor's typed statuses and row styles as the frontend receives them. Glyphs and
//! words stay in Rust; the frontend only maps tones and weights to its theme.
use artifactize::monitor;
use serde::Serialize;
use specta::Type;

/// Every status the monitor shows, most urgent first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
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
    Green,
    Basis,
}

impl From<monitor::Status> for Status {
    fn from(status: monitor::Status) -> Self {
        use monitor::Status as S;
        match status {
            S::Error => Self::Error,
            S::Red => Self::Red,
            S::Blocked => Self::Blocked,
            S::Running => Self::Running,
            S::WaitingHuman => Self::WaitingHuman,
            S::Queued => Self::Queued,
            S::BudgetExhausted => Self::BudgetExhausted,
            S::WaitDependency => Self::WaitDependency,
            S::Wait => Self::Wait,
            S::Stale => Self::Stale,
            S::Unreviewed => Self::Unreviewed,
            S::Incomplete => Self::Incomplete,
            S::Green => Self::Green,
            S::Basis => Self::Basis,
        }
    }
}

/// A status with its glyph, as badges, headlines and counts show it.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Mark {
    pub status: Status,
    pub glyph: String,
}

impl From<monitor::Status> for Mark {
    fn from(status: monitor::Status) -> Self {
        Self {
            status: status.into(),
            glyph: status.glyph().into(),
        }
    }
}

/// A badge part such as `!2`: a status and how many.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Count {
    pub mark: Mark,
    pub count: u32,
}

impl Count {
    pub fn new(status: monitor::Status, count: u64) -> Self {
        Self {
            mark: status.into(),
            count: u32::try_from(count).unwrap_or(u32::MAX),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Tone {
    Green,
    Red,
    Error,
    Running,
    Human,
    Queued,
    Blocked,
    Muted,
}

impl From<monitor::Tone> for Tone {
    fn from(tone: monitor::Tone) -> Self {
        use monitor::Tone as T;
        match tone {
            T::Green => Self::Green,
            T::Red => Self::Red,
            T::Error => Self::Error,
            T::Running => Self::Running,
            T::Human => Self::Human,
            T::Queued => Self::Queued,
            T::Blocked => Self::Blocked,
            T::Muted => Self::Muted,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Weight {
    Bold,
    Normal,
    Dim,
}

impl From<monitor::Weight> for Weight {
    fn from(weight: monitor::Weight) -> Self {
        match weight {
            monitor::Weight::Bold => Self::Bold,
            monitor::Weight::Normal => Self::Normal,
            monitor::Weight::Dim => Self::Dim,
        }
    }
}
