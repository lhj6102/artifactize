//! Eval execution constraints, separate from Artifact tags and evaluation criteria.

use serde::{Deserialize, Serialize};

/// A colon-separated resource path rooted at `use-computer`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct MutexTag(String);

impl std::str::FromStr for MutexTag {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut segments = value.split(':');
        if segments.next() != Some("use-computer") {
            return Err("Mutex tags must be rooted at use-computer.".into());
        }
        for segment in segments {
            super::identifier(segment, "Mutex tag segment")?;
        }
        Ok(Self(value.into()))
    }
}

impl<'de> Deserialize<'de> for MutexTag {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

impl MutexTag {
    /// Equal paths and ancestor/descendant paths conflict; siblings do not.
    pub fn conflicts(&self, other: &Self) -> bool {
        fn ancestor(parent: &str, child: &str) -> bool {
            child
                .strip_prefix(parent)
                .is_some_and(|suffix| suffix.is_empty() || suffix.starts_with(':'))
        }
        ancestor(&self.0, &other.0) || ancestor(&other.0, &self.0)
    }
}

pub(super) fn validate(tags: &[MutexTag]) -> Result<(), String> {
    if tags.len() > super::MAX_DECLARED_ITEMS
        || tags.iter().collect::<std::collections::BTreeSet<_>>().len() != tags.len()
    {
        return Err("mutex must contain at most 64 unique tags.".into());
    }
    Ok(())
}
