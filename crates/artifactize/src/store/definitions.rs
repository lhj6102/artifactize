//! Typed schema-5 saved graph snapshots, distinct from strict live declarations.
//! Unknown saved fields survive round-trips; owner schemas and payload extensions stay JSON.
mod extensions;
#[cfg(test)]
mod tests;
use crate::config::{self, Field, StoredPayload};
pub use extensions::{Profile, SavedSelection};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};
use std::{collections::BTreeMap, path::PathBuf};

fn missing<T>(field: &Field<T>) -> bool {
    matches!(field, Field::Missing)
}

#[derive(Debug, Clone, Default)]
pub struct Definitions(pub Option<Graph>);
impl Definitions {
    pub fn graph(&self) -> Option<&Graph> {
        self.0.as_ref()
    }
    pub fn from_view(view: &crate::query::GraphView<'_>) -> Result<Self, serde_json::Error> {
        serde_json::from_value(serde_json::to_value(view)?)
    }
}
impl Serialize for Definitions {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for Definitions {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Option::<Graph>::deserialize(deserializer).map(Self)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Graph {
    #[serde(default, skip_serializing_if = "missing")]
    pub version: Field<u32>,
    #[serde(default, skip_serializing_if = "missing", with = "path_field")]
    pub repo_path: Field<PathBuf>,
    #[serde(default, skip_serializing_if = "missing")]
    pub selection: Field<SavedSelection>,
    #[serde(default, skip_serializing_if = "Field::missing")]
    pub artifacts: Field<BTreeMap<crate::types::ArtifactName, Artifact>>,
    #[serde(default, skip_serializing_if = "missing")]
    pub evals: Field<Vec<Eval>>,
    #[serde(default, skip_serializing_if = "missing")]
    pub relations: Field<Vec<Relation>>,
    #[serde(default, skip_serializing_if = "missing")]
    pub components: Field<Vec<Component>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
impl Graph {
    pub fn artifacts(&self) -> impl Iterator<Item = (&crate::types::ArtifactName, &Artifact)> {
        match &self.artifacts {
            Field::Value(map) => Some(map),
            _ => None,
        }
        .into_iter()
        .flatten()
    }
    pub fn artifact(&self, id: &str) -> Option<&Artifact> {
        match &self.artifacts {
            Field::Value(map) => map.get(id),
            _ => None,
        }
    }
    pub fn evals(&self) -> &[Eval] {
        match &self.evals {
            Field::Value(evals) => evals,
            _ => &[],
        }
    }
    pub fn relations(&self) -> &[Relation] {
        match &self.relations {
            Field::Value(relations) => relations,
            _ => &[],
        }
    }
    pub fn components(&self) -> &[Component] {
        match &self.components {
            Field::Value(components) => components,
            _ => &[],
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    #[serde(default, skip_serializing_if = "missing")]
    pub kind: Field<config::ArtifactKind>,
    #[serde(default, skip_serializing_if = "missing", with = "path_field")]
    pub path: Field<PathBuf>,
    #[serde(default, skip_serializing_if = "missing")]
    pub children: Field<BTreeMap<String, crate::types::ArtifactName>>,
    #[serde(default, skip_serializing_if = "missing")]
    pub name: Field<crate::types::ArtifactName>,
    #[serde(default, skip_serializing_if = "missing")]
    pub tags: Field<Vec<String>>,
    #[serde(default, skip_serializing_if = "missing")]
    pub views: Field<Views>,
    #[serde(default, skip_serializing_if = "missing")]
    pub mounts: Field<BTreeMap<String, crate::types::ArtifactName>>,
    #[serde(default, skip_serializing_if = "missing")]
    pub basis: Field<bool>,
    #[serde(default, skip_serializing_if = "missing")]
    pub fingerprint: Field<Fingerprint>,
    #[serde(default, skip_serializing_if = "missing")]
    pub review_policy: Field<Policy>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Views {
    #[serde(default, skip_serializing_if = "missing")]
    pub agent_tools: Field<BTreeMap<String, AgentTool>>,
    #[serde(default, skip_serializing_if = "missing")]
    pub human_tools: Field<BTreeMap<String, HumanTool>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AgentTool {
    Command(CommandTool),
    Builtin(BuiltinTool),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandTool {
    pub description: String,
    pub input_schema: Value,
    pub protocol: config::ToolProtocol,
    pub command: String,
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "missing", with = "duration_field")]
    pub timeout_ms: Field<std::time::Duration>,
    #[serde(default, skip_serializing_if = "missing")]
    pub execution_paths: Field<Vec<String>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuiltinTool {
    pub builtin: config::Builtin,
    #[serde(default, skip_serializing_if = "missing")]
    pub args: Field<Vec<String>>,
    #[serde(default, skip_serializing_if = "missing")]
    pub description: Field<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HumanTool {
    Command(HumanCommandTool),
    Builtin(HumanBuiltinTool),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HumanCommandTool {
    pub description: String,
    pub kind: config::HumanToolKind,
    pub command: String,
    pub args: Vec<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanBuiltinTool {
    pub builtin: config::Builtin,
    pub description: String,
    pub kind: config::HumanToolKind,
    pub args: Vec<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fingerprint {
    #[serde(default, skip_serializing_if = "missing")]
    pub script: Field<Script>,
    #[serde(default, skip_serializing_if = "missing")]
    pub files: Field<Vec<String>>,
    #[serde(default, skip_serializing_if = "missing")]
    pub ignore: Field<Vec<String>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Script {
    pub command: String,
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "missing")]
    pub files: Field<Vec<String>>,
    #[serde(default, skip_serializing_if = "missing", with = "duration_field")]
    pub timeout_ms: Field<std::time::Duration>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Policy {
    #[serde(default, skip_serializing_if = "missing")]
    pub dependency_gates: Field<config::DependencyGates>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Eval {
    pub id: crate::types::EvalId,
    pub target: crate::types::ArtifactName,
    #[serde(default, skip_serializing_if = "missing")]
    pub references: Field<BTreeMap<String, crate::types::ArtifactName>>,
    #[serde(default, skip_serializing_if = "missing")]
    pub deps: Field<Vec<crate::types::ArtifactName>>,
    #[serde(default, skip_serializing_if = "missing")]
    pub declaration: Field<Declaration>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Declaration {
    #[serde(default, skip_serializing_if = "missing")]
    pub id: Field<crate::config::LocalEvalId>,
    #[serde(default, skip_serializing_if = "missing")]
    pub title: Field<String>,
    #[serde(default, skip_serializing_if = "missing")]
    pub profile: Field<Profile>,
    #[serde(default, skip_serializing_if = "missing")]
    pub profile_variants: Field<BTreeMap<String, Profile>>,
    #[serde(default, skip_serializing_if = "missing")]
    pub payload: Field<StoredPayload>,
    #[serde(default, skip_serializing_if = "missing")]
    pub pass_schema: Field<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "missing")]
    pub fail_schema: Field<Map<String, Value>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Component {
    #[serde(default, skip_serializing_if = "missing")]
    pub id: Field<usize>,
    #[serde(default, skip_serializing_if = "missing")]
    pub artifacts: Field<Vec<crate::types::ArtifactName>>,
    #[serde(default, skip_serializing_if = "missing")]
    pub dependencies: Field<Vec<usize>>,
    #[serde(default, skip_serializing_if = "missing")]
    pub gates: Field<Vec<crate::types::EvalId>>,
    #[serde(default, skip_serializing_if = "missing")]
    pub cyclic: Field<bool>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Relation {
    pub source: crate::types::ArtifactName,
    pub target: crate::types::ArtifactName,
    #[serde(flatten)]
    pub kind: RelationKind,
    #[serde(default, skip_serializing_if = "missing")]
    pub cyclic: Field<bool>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
impl<'de> Deserialize<'de> for Relation {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Flattened internally-tagged enums do not consume their fields from a second
        // flatten map. Split at the saved boundary so reserialization never duplicates kind.
        let mut fields = Map::<String, Value>::deserialize(deserializer)?;
        let mut take = |name: &str| {
            fields
                .remove(name)
                .ok_or_else(|| serde::de::Error::custom(format!("missing relation {name}")))
        };
        let source = serde_json::from_value(take("source")?).map_err(serde::de::Error::custom)?;
        let target = serde_json::from_value(take("target")?).map_err(serde::de::Error::custom)?;
        let kind_name: String =
            serde_json::from_value(take("kind")?).map_err(serde::de::Error::custom)?;
        let names: &[&str] = match kind_name.as_str() {
            "child" => &["path"],
            "mount" => &["alias"],
            "instruction" | "dependency" => &["evalId", "name"],
            "argv" => &["evalId", "index", "name", "path"],
            _ => &[],
        };
        let unknown = names.is_empty();
        let mut kind = Map::from_iter([("kind".into(), Value::String(kind_name.clone()))]);
        for name in names {
            if let Some(value) = fields.remove(*name) {
                kind.insert((*name).into(), value);
            }
        }
        let kind = if unknown {
            RelationKind::Unknown { kind: kind_name }
        } else {
            serde_json::from_value(Value::Object(kind)).map_err(serde::de::Error::custom)?
        };
        let cyclic = match fields.remove("cyclic") {
            Some(value) => serde_json::from_value(value).map_err(serde::de::Error::custom)?,
            None => Field::Missing,
        };
        Ok(Self {
            source,
            target,
            kind,
            cyclic,
            extra: fields,
        })
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum RelationKind {
    Child {
        #[serde(default, skip_serializing_if = "missing")]
        path: Field<String>,
    },
    Mount {
        #[serde(default, skip_serializing_if = "missing")]
        alias: Field<String>,
    },
    Dependency {
        #[serde(rename = "evalId", default, skip_serializing_if = "missing")]
        eval_id: Field<crate::types::EvalId>,
        #[serde(default, skip_serializing_if = "missing")]
        name: Field<String>,
    },
    Instruction {
        #[serde(rename = "evalId")]
        eval_id: crate::types::EvalId,
        name: String,
    },
    #[serde(rename = "argv")]
    Argument {
        #[serde(rename = "evalId")]
        eval_id: crate::types::EvalId,
        index: usize,
        name: String,
        path: String,
    },
    #[serde(untagged)]
    Unknown { kind: String },
}

mod duration_field {
    use crate::config::Field;
    use serde::{Deserializer, Serializer};
    use std::time::Duration;
    pub fn serialize<S: Serializer>(
        value: &Field<Duration>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Field::Value(value) => {
                let milliseconds = value.as_millis();
                if !(1..=u128::from(crate::config::validation::MAX_TIMEOUT_MS))
                    .contains(&milliseconds)
                    || Duration::from_millis(milliseconds as u64) != *value
                {
                    return Err(serde::ser::Error::custom(
                        "Saved definition timeoutMs must be whole milliseconds from 1 through 2147483647.",
                    ));
                }
                serializer.serialize_u128(milliseconds)
            }
            _ => serializer.serialize_none(),
        }
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Field<Duration>, D::Error> {
        crate::config::validation::milliseconds::deserialize(deserializer)
            .map(|value| value.map_or(Field::Null, Field::Value))
    }
}

pub(super) mod path_field {
    use crate::config::Field;
    use serde::{Deserializer, Serializer};
    use std::path::PathBuf;
    pub fn serialize<S: Serializer>(
        value: &Field<PathBuf>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Field::Value(path) => crate::platform::path_serde::serialize(path, serializer),
            _ => serializer.serialize_none(),
        }
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Field<PathBuf>, D::Error> {
        crate::platform::path_serde::option::deserialize(deserializer)
            .map(|value| value.map_or(Field::Null, Field::Value))
    }
}
