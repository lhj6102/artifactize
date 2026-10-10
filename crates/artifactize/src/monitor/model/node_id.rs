//! Validated tree identities and their typed navigation target.
use super::Target;
use std::{fmt, ops::Deref, str::FromStr};

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId {
    text: String,
}
impl NodeId {
    pub fn artifact(id: crate::types::ArtifactName) -> Self {
        Self {
            text: format!("a:{id}"),
        }
    }
    pub fn eval(id: crate::types::EvalId) -> Self {
        Self {
            text: format!("e:{id}"),
        }
    }
    pub fn run(id: &crate::types::RunId) -> Self {
        Self {
            text: format!("run:{id}"),
        }
    }
    pub fn as_str(&self) -> &str {
        &self.text
    }
}
impl FromStr for NodeId {
    type Err = String;
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (kind, id) = text.split_once(':').ok_or("Invalid tree node ID.")?;
        match kind {
            "a" => id.parse().map(Self::artifact),
            "e" => id.parse().map(Self::eval),
            "run" => id.parse::<crate::types::RunId>().map(|id| Self::run(&id)),
            _ => Err("Invalid tree node ID.".into()),
        }
    }
}
impl Deref for NodeId {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}
impl fmt::Display for NodeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}
impl PartialEq<str> for NodeId {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}
impl PartialEq<&str> for NodeId {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}
impl From<NodeId> for String {
    fn from(id: NodeId) -> Self {
        id.text
    }
}
impl From<&NodeId> for Target {
    fn from(id: &NodeId) -> Self {
        Target::parse(id.as_str()).expect("validated tree ID")
    }
}
