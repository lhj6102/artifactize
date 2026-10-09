//! Preserve unknown nested saved-profile and selector fields without dynamic core parsing.
use crate::{config::StoredProfile, project::selection::Selection};
use serde::{Deserialize, Deserializer, Serialize, Serializer, ser::SerializeMap};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct Profile {
    pub known: StoredProfile,
    pub extra: Map<String, Value>,
}
impl<'de> Deserialize<'de> for Profile {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut fields = Map::<String, Value>::deserialize(deserializer)?;
        let names: &[&str] = match fields.get("kind").and_then(Value::as_str) {
            Some("agent") => &[
                "kind",
                "backend",
                "model",
                "reasoning",
                "timeoutMs",
                "maxToolCalls",
                "maxTokens",
            ],
            Some("dependency") => &["kind", "dependsOn"],
            Some("runtime") => &["kind", "command", "args", "timeoutMs"],
            _ => &["kind"],
        };
        let known = split(&mut fields, names);
        let known =
            serde_json::from_value(Value::Object(known)).map_err(serde::de::Error::custom)?;
        Ok(Self {
            known,
            extra: fields,
        })
    }
}
impl Serialize for Profile {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize(&self.known, &self.extra, serializer)
    }
}

#[derive(Debug, Clone)]
pub struct SavedSelection {
    pub known: Selection,
    pub extra: Map<String, Value>,
}
impl<'de> Deserialize<'de> for SavedSelection {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut fields = Map::<String, Value>::deserialize(deserializer)?;
        let names: &[&str] = match fields.get("kind").and_then(Value::as_str) {
            Some("artifact") => &["kind", "artifactId"],
            Some("eval") => &["kind", "evalId"],
            Some("artifacts") => &["kind", "artifactIds"],
            Some("evals") => &["kind", "evalIds"],
            _ => &["kind"],
        };
        let known = serde_json::from_value(Value::Object(split(&mut fields, names)))
            .map_err(serde::de::Error::custom)?;
        Ok(Self {
            known,
            extra: fields,
        })
    }
}
impl Serialize for SavedSelection {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize(&self.known, &self.extra, serializer)
    }
}
fn split(fields: &mut Map<String, Value>, names: &[&str]) -> Map<String, Value> {
    names
        .iter()
        .filter_map(|name| fields.remove(*name).map(|value| ((*name).into(), value)))
        .collect()
}
fn serialize<T: Serialize, S: Serializer>(
    known: &T,
    extra: &Map<String, Value>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let known: BTreeMap<String, Value> =
        serde_json::from_value(serde_json::to_value(known).map_err(serde::ser::Error::custom)?)
            .map_err(serde::ser::Error::custom)?;
    let mut object = serializer.serialize_map(Some(known.len() + extra.len()))?;
    for (name, value) in known {
        object.serialize_entry(&name, &value)?;
    }
    for (name, value) in extra {
        object.serialize_entry(name, value)?;
    }
    object.end()
}
