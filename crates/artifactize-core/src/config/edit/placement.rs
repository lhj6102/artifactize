//! Placement identities enter as strings at the UI/serde edge and stay typed thereafter.
use super::{ArtifactName, discovery};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum PlacementKey {
    Node(ArtifactName),
    Part {
        whole: ArtifactName,
        part: ArtifactName,
    },
}
impl PlacementKey {
    /// The file that owns this placement: the node itself or its whole.
    pub fn owner(&self) -> &ArtifactName {
        match self {
            Self::Node(name) => name,
            Self::Part { whole, .. } => whole,
        }
    }
}
impl std::str::FromStr for PlacementKey {
    type Err = String;
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text.split_once('/') {
            Some((whole, part)) => Ok(Self::Part {
                whole: discovery::definition_name(whole)?,
                part: discovery::definition_name(part)?,
            }),
            None => Ok(Self::Node(discovery::definition_name(text)?)),
        }
    }
}
impl TryFrom<String> for PlacementKey {
    type Error = String;
    fn try_from(text: String) -> Result<Self, Self::Error> {
        text.parse()
    }
}
impl From<PlacementKey> for String {
    fn from(key: PlacementKey) -> Self {
        key.to_string()
    }
}
impl std::fmt::Display for PlacementKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Node(name) => name.fmt(formatter),
            Self::Part { whole, part } => write!(formatter, "{whole}/{part}"),
        }
    }
}
