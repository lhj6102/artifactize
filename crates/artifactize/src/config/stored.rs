//! Profiles saved by schema 5. Unlike declarations, their optional fields may be null.
//! Missing and null remain distinct: profile comparison drives the otherProfile tally.

use super::{ArtifactName, Backend, ModelId, Profile, ProfileKind, Reasoning};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::time::Duration;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Field<T> {
    #[default]
    Missing,
    Null,
    Value(T),
}
impl<T> Field<T> {
    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Value(value) => Some(value),
            _ => None,
        }
    }
    pub(crate) fn missing(&self) -> bool {
        matches!(self, Self::Missing)
    }
}
impl<T: Serialize> Serialize for Field<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Value(value) => value.serialize(serializer),
            _ => serializer.serialize_none(),
        }
    }
}
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Field<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Option::<T>::deserialize(deserializer)?.map_or(Self::Null, Self::Value))
    }
}
impl<T> From<Option<T>> for Field<T> {
    fn from(value: Option<T>) -> Self {
        value.map_or(Self::Null, Self::Value)
    }
}

/// Read schema-5 payloads without inventing an instruction field for older audits.
/// Declarations still require a String; omission/null tolerance belongs only here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredPayload {
    #[serde(default, skip_serializing_if = "Field::missing")]
    instruction: Field<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl StoredPayload {
    pub fn instruction(&self) -> &str {
        match &self.instruction {
            Field::Value(value) => value,
            Field::Missing | Field::Null => "",
        }
    }
}

impl From<&Option<super::EvalPayload>> for StoredPayload {
    fn from(payload: &Option<super::EvalPayload>) -> Self {
        payload.as_ref().map_or_else(
            || Self {
                instruction: Field::Missing,
                extra: Default::default(),
            },
            Self::from,
        )
    }
}

impl From<&super::EvalPayload> for StoredPayload {
    fn from(payload: &super::EvalPayload) -> Self {
        Self {
            instruction: Field::Value(payload.instruction.clone()),
            extra: payload.extra.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum StoredProfile {
    Agent {
        backend: Backend,
        model: ModelId,
        #[serde(default, skip_serializing_if = "Field::missing")]
        reasoning: Field<Reasoning>,
        #[serde(
            rename = "timeoutMs",
            default,
            skip_serializing_if = "Field::missing",
            with = "milliseconds"
        )]
        timeout_ms: Field<Duration>,
        #[serde(
            rename = "maxToolCalls",
            default,
            skip_serializing_if = "Field::missing"
        )]
        max_tool_calls: Field<u64>,
        #[serde(rename = "maxTokens", default, skip_serializing_if = "Field::missing")]
        max_tokens: Field<u64>,
    },
    Human {},
    Dependency {
        #[serde(rename = "dependsOn")]
        depends_on: Vec<ArtifactName>,
    },
    Runtime {
        command: String,
        args: Vec<String>,
        #[serde(
            rename = "timeoutMs",
            default,
            skip_serializing_if = "Field::missing",
            with = "milliseconds"
        )]
        timeout_ms: Field<Duration>,
    },
}
impl StoredProfile {
    pub fn kind(&self) -> ProfileKind {
        match self {
            Self::Agent { .. } => ProfileKind::Agent,
            Self::Human {} => ProfileKind::Human,
            Self::Dependency { .. } => ProfileKind::Dependency,
            Self::Runtime { .. } => ProfileKind::Runtime,
        }
    }
}
impl From<&Profile> for StoredProfile {
    fn from(profile: &Profile) -> Self {
        match profile {
            Profile::Agent {
                backend,
                model,
                reasoning,
                timeout_ms,
                max_tool_calls,
                max_tokens,
            } => Self::Agent {
                backend: *backend,
                model: model.clone(),
                reasoning: (*reasoning).into(),
                timeout_ms: (*timeout_ms).into(),
                max_tool_calls: (*max_tool_calls).into(),
                max_tokens: (*max_tokens).into(),
            },
            Profile::Human {} => Self::Human {},
            Profile::Dependency { depends_on } => Self::Dependency {
                depends_on: depends_on.clone(),
            },
            Profile::Runtime {
                command,
                args,
                timeout_ms,
            } => Self::Runtime {
                command: command.clone(),
                args: args.clone(),
                timeout_ms: (*timeout_ms).into(),
            },
        }
    }
}
mod milliseconds {
    use super::*;
    pub fn serialize<S: Serializer>(
        value: &Field<Duration>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Field::Value(value) => serializer.serialize_u128(value.as_millis()),
            _ => serializer.serialize_none(),
        }
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Field<Duration>, D::Error> {
        // Saved profiles can contain null; declarations use the strict timeout parser.
        let value = Option::<serde_json::Number>::deserialize(deserializer)?;
        value.map_or(Ok(Field::Null), |value| {
            super::super::validation::timeout_number::<D::Error>(value).map(Field::Value)
        })
    }
}

impl From<Option<&super::EvalPayload>> for StoredPayload {
    fn from(payload: Option<&super::EvalPayload>) -> Self {
        payload.map_or_else(
            || Self {
                instruction: Field::Missing,
                extra: Default::default(),
            },
            Self::from,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn stored_payloads_preserve_schema_5_missing_null_and_owner_context() {
        for value in [
            json!({}),
            json!({"instruction":null,"owner":[true,null,42]}),
            json!({"instruction":"Inspect {input}.","owner":{"nested":[]}}),
        ] {
            let stored: StoredPayload = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(serde_json::to_value(&stored).unwrap(), value);
            assert_eq!(
                stored.instruction(),
                value["instruction"].as_str().unwrap_or_default()
            );
            if value["instruction"].is_null() {
                assert!(serde_json::from_value::<super::super::EvalPayload>(value).is_err());
            }
        }
    }

    #[test]
    fn stored_profiles_preserve_missing_and_null_without_weakening_config() {
        for value in [
            json!({"kind":"agent","backend":"openai","model":"fixture"}),
            json!({
                "kind":"agent",
                "backend":"openai",
                "model":"fixture",
                "reasoning":null,
                "timeoutMs":null,
                "maxToolCalls":null,
                "maxTokens":null,
            }),
            json!({"kind":"runtime","command":"fixture","args":[],"timeoutMs":null}),
            json!({"kind":"runtime","command":"fixture","args":[],"timeoutMs":1000}),
            json!({"kind":"human"}),
        ] {
            let stored: StoredProfile = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(serde_json::to_value(&stored).unwrap(), value);
            if value
                .get("timeoutMs")
                .is_some_and(serde_json::Value::is_null)
            {
                let mut declaration = value;
                let object = declaration.as_object_mut().unwrap();
                for (camel, snake) in [
                    ("timeoutMs", "timeout_ms"),
                    ("maxToolCalls", "max_tool_calls"),
                    ("maxTokens", "max_tokens"),
                ] {
                    if let Some(value) = object.remove(camel) {
                        object.insert(snake.into(), value);
                    }
                }
                // Only the timeout is invalid, so another optional field cannot mask it.
                for field in ["reasoning", "max_tool_calls", "max_tokens"] {
                    object.remove(field);
                }
                let error = serde_json::from_value::<Profile>(declaration)
                    .unwrap_err()
                    .to_string();
                assert!(
                    error.contains("null") && !error.contains("unknown field"),
                    "{error}"
                );
            }
        }
        let missing: StoredProfile =
            serde_json::from_value(json!({"kind":"runtime","command":"fixture","args":[]}))
                .unwrap();
        let null: StoredProfile = serde_json::from_value(
            json!({"kind":"runtime","command":"fixture","args":[],"timeoutMs":null}),
        )
        .unwrap();
        assert_ne!(missing, null);
        let config: Profile =
            serde_json::from_value(json!({"kind":"agent","backend":"openai","model":"fixture"}))
                .unwrap();
        assert_eq!(
            serde_json::to_value(StoredProfile::from(&config)).unwrap(),
            json!({
                "kind":"agent",
                "backend":"openai",
                "model":"fixture",
                "reasoning":null,
                "timeoutMs":null,
                "maxToolCalls":null,
                "maxTokens":null,
            })
        );
    }
}
