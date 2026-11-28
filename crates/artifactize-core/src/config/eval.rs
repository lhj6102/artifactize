//! An Eval declaration: its local id, title, profile, instruction with the typed slots it was
//! written with, a runtime eval's inputs, and the priority at which an Agent eval turns RED.

use super::{ArtifactName, MutexTag, Profile, ProfileKind, identifier};
use crate::scope::LogicalTarget;
use serde::{Deserialize, Serialize};
use std::ops::Range;

/// The instruction a reviewer reads, as saved with every request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvalPayload {
    pub instruction: String,
}

/// An Eval's id within its Artifact, such as `follows-style`, validated when its declaration is
/// read.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct LocalEvalId(String);

impl<'de> Deserialize<'de> for LocalEvalId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

impl std::str::FromStr for LocalEvalId {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        identifier(value, "Eval id")?;
        Ok(Self(value.to_owned()))
    }
}

impl LocalEvalId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl PartialEq<&str> for LocalEvalId {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl std::ops::Deref for LocalEvalId {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for LocalEvalId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// An issue's priority: P0 is the most important and P3 the least. artifactize defines only
/// their order; the evaluator assigns them by its own judgement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Priority {
    P0,
    P1,
    P2,
    P3,
}

impl Priority {
    pub const ALL: [Self; 4] = [Self::P0, Self::P1, Self::P2, Self::P3];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::P0 => "P0",
            Self::P1 => "P1",
            Self::P2 => "P2",
            Self::P3 => "P3",
        }
    }
}

impl std::fmt::Display for Priority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Who a tool serves: a reviewing model or a person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Audience {
    Agent,
    Human,
}

/// What a slot of an instruction names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Slot {
    Artf {
        artifact: ArtifactName,
    },
    Path {
        artifact: ArtifactName,
        path: LogicalTarget,
    },
}

impl Slot {
    pub fn artifact(&self) -> &ArtifactName {
        match self {
            Self::Artf { artifact } | Self::Path { artifact, .. } => artifact,
        }
    }
}

/// A slot and the bytes of the rendered instruction it fills.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstructionSlot {
    #[serde(serialize_with = "range")]
    pub range: Range<usize>,
    #[serde(flatten)]
    pub slot: Slot,
}

fn range<S: serde::Serializer>(range: &Range<usize>, serializer: S) -> Result<S::Ok, S::Error> {
    [range.start, range.end].serialize(serializer)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvalDeclaration {
    pub id: LocalEvalId,
    pub title: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mutex: Vec<MutexTag>,
    #[serde(flatten)]
    body: EvalBody,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct EvalBody {
    profile: Profile,
    /// The named profile or `name!` macro it refers to, as written.
    #[serde(skip_serializing_if = "Option::is_none")]
    profile_name: Option<String>,
    /// Required for Agent and Human evals; a runtime eval's is unhashed prose.
    #[serde(skip_serializing_if = "Option::is_none")]
    payload: Option<EvalPayload>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    slots: Vec<InstructionSlot>,
    /// A runtime eval's input Artifacts.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    inputs: Vec<ArtifactName>,
    /// An Agent eval's bar: an issue at this priority or a more important one makes it RED.
    /// Without it, any issue does.
    #[serde(skip_serializing_if = "Option::is_none")]
    red_at: Option<Priority>,
}

/// What an expanded eval declares, before it is checked.
pub(super) struct Declared {
    pub id: LocalEvalId,
    pub title: String,
    pub mutex: Vec<MutexTag>,
    pub profile: Profile,
    pub profile_name: Option<String>,
    pub instruction: Option<(String, Vec<InstructionSlot>)>,
    pub inputs: Vec<ArtifactName>,
    pub red_at: Option<Priority>,
}

impl EvalDeclaration {
    /// The fields every profile needs together. Expansion has placed each field already, so
    /// what is left are rules across them.
    pub(super) fn new(declared: Declared) -> Result<Self, String> {
        let kind = declared.profile.kind();
        if !declared.mutex.is_empty() && kind == ProfileKind::Human {
            return Err("mutex is supported only on runtime and Agent evals.".into());
        }
        if kind != ProfileKind::Agent && declared.red_at.is_some() {
            return Err("red_at is supported only on Agent evals.".into());
        }
        if kind != ProfileKind::Runtime && !declared.inputs.is_empty() {
            return Err("inputs are supported only on runtime evals.".into());
        }
        super::validation::text(&declared.title, "Eval title")?;
        let (payload, slots) = match declared.instruction {
            Some((instruction, slots)) => {
                super::validation::text(&instruction, "Eval instruction")?;
                (Some(EvalPayload { instruction }), slots)
            }
            None if kind == ProfileKind::Runtime => (None, Vec::new()),
            None => return Err("Agent and Human evals need an instruction.".into()),
        };
        if kind == ProfileKind::Runtime && !slots.is_empty() {
            return Err("A runtime eval's instruction names no Artifact.".into());
        }
        Ok(Self {
            id: declared.id,
            title: declared.title,
            mutex: declared.mutex,
            body: EvalBody {
                profile: declared.profile,
                profile_name: declared.profile_name,
                payload,
                slots,
                inputs: declared.inputs,
                red_at: declared.red_at,
            },
        })
    }

    pub fn profile(&self) -> &Profile {
        &self.body.profile
    }
    pub fn profile_name(&self) -> Option<&str> {
        self.body.profile_name.as_deref()
    }
    pub fn payload(&self) -> Option<&EvalPayload> {
        self.body.payload.as_ref()
    }
    pub fn instruction(&self) -> Option<&str> {
        self.payload().map(|payload| payload.instruction.as_str())
    }
    pub fn slots(&self) -> &[InstructionSlot] {
        &self.body.slots
    }
    pub fn inputs(&self) -> &[ArtifactName] {
        &self.body.inputs
    }
    pub fn red_at(&self) -> Option<Priority> {
        self.body.red_at
    }
    #[cfg(test)]
    pub(crate) fn profile_mut(&mut self) -> &mut Profile {
        &mut self.body.profile
    }
}
