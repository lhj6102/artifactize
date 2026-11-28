//! A person's verdict as the frontend names it.
use artifactize_core::runtime;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Verdict {
    Green,
    Red,
}

impl From<Verdict> for runtime::Verdict {
    fn from(verdict: Verdict) -> Self {
        match verdict {
            Verdict::Green => Self::Green,
            Verdict::Red => Self::Red,
        }
    }
}

impl From<runtime::Verdict> for Verdict {
    fn from(verdict: runtime::Verdict) -> Self {
        match verdict {
            runtime::Verdict::Green => Self::Green,
            runtime::Verdict::Red => Self::Red,
        }
    }
}
