//! Typed review results with the unchanged, untagged saved JSON envelope.

use std::time::Duration;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use crate::{agent::verdict::ValidatedResult, runtime::Verdict};

/// Agent, Human and derived results retain opaque owner fields. Runtime metadata is
/// parsed once when a saved record enters the core, including reduced remote records.
#[derive(Debug, Clone, PartialEq)]
pub enum ExecutionResult {
    Owner(ValidatedResult),
    Runtime(RuntimeResult),
}

/// Missing and null are distinct in existing reduced/older records. Preserve both.
#[derive(Debug, Clone, PartialEq, Default)]
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
            Self::Missing | Self::Null => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeResult {
    pub verdict: Verdict,
    pub exit_code: Field<i32>,
    pub duration: Field<Duration>,
    pub truncated: Field<bool>,
    pub stdout: Field<String>,
    pub stderr: Field<String>,
    /// Unknown fields from newer producers survive round trips unchanged.
    pub fields: Map<String, Value>,
}

impl RuntimeResult {
    pub fn summary(&self) -> Self {
        Self {
            stdout: Field::Missing,
            stderr: Field::Missing,
            fields: Map::new(),
            exit_code: if matches!(self.exit_code, Field::Missing) {
                Field::Null
            } else {
                self.exit_code.clone()
            },
            duration: if matches!(self.duration, Field::Missing) {
                Field::Null
            } else {
                self.duration.clone()
            },
            truncated: if matches!(self.truncated, Field::Missing) {
                Field::Null
            } else {
                self.truncated.clone()
            },
            verdict: self.verdict,
        }
    }
}

fn take<T: serde::de::DeserializeOwned>(
    fields: &mut Map<String, Value>,
    key: &str,
) -> Result<Field<T>, serde_json::Error> {
    match fields.remove(key) {
        None => Ok(Field::Missing),
        Some(Value::Null) => Ok(Field::Null),
        Some(value) => serde_json::from_value(value).map(Field::Value),
    }
}

fn insert<T: Serialize>(fields: &mut Map<String, Value>, key: &str, value: &Field<T>) {
    match value {
        Field::Missing => {}
        Field::Null => {
            fields.insert(key.into(), Value::Null);
        }
        Field::Value(value) => {
            fields.insert(
                key.into(),
                serde_json::to_value(value).expect("result field is JSON"),
            );
        }
    }
}

impl ExecutionResult {
    pub fn verdict(&self) -> Verdict {
        match self {
            Self::Owner(result) => result.verdict,
            Self::Runtime(result) => result.verdict,
        }
    }

    pub fn owner_fields(&self) -> &Map<String, Value> {
        match self {
            Self::Owner(result) => &result.fields,
            Self::Runtime(result) => &result.fields,
        }
    }

    /// JSON is materialized only at presentation/schema/serialization boundaries.
    pub fn to_json(&self) -> Value {
        let mut fields = self.owner_fields().clone();
        fields.insert(
            "verdict".into(),
            serde_json::to_value(self.verdict()).expect("verdict is JSON"),
        );
        if let Self::Runtime(result) = self {
            insert(&mut fields, "exitCode", &result.exit_code);
            let duration = match result.duration {
                Field::Missing => Field::Missing,
                Field::Null => Field::Null,
                Field::Value(duration) => Field::Value(
                    u64::try_from(duration.as_millis())
                        .expect("saved runtime duration fits u64 milliseconds"),
                ),
            };
            insert(&mut fields, "durationMs", &duration);
            insert(&mut fields, "truncated", &result.truncated);
            insert(&mut fields, "stdout", &result.stdout);
            insert(&mut fields, "stderr", &result.stderr);
        }
        Value::Object(fields)
    }
}

impl From<ValidatedResult> for ExecutionResult {
    fn from(result: ValidatedResult) -> Self {
        Self::Owner(result)
    }
}

impl TryFrom<Value> for ExecutionResult {
    type Error = serde_json::Error;
    fn try_from(value: Value) -> Result<Self, Self::Error> {
        let mut fields: Map<String, Value> = serde_json::from_value(value)?;
        let verdict = serde_json::from_value(fields.remove("verdict").unwrap_or(Value::Null))?;
        // Owner schemas reserve these runtime fields. `truncated` alone remains a
        // legitimate owner property and must not turn an owner result into Runtime.
        if ["exitCode", "durationMs", "stdout", "stderr"]
            .iter()
            .any(|key| fields.contains_key(*key))
        {
            let duration = match take::<u64>(&mut fields, "durationMs")? {
                Field::Missing => Field::Missing,
                Field::Null => Field::Null,
                Field::Value(ms) => Field::Value(Duration::from_millis(ms)),
            };
            Ok(Self::Runtime(RuntimeResult {
                verdict,
                exit_code: take(&mut fields, "exitCode")?,
                duration,
                truncated: take(&mut fields, "truncated")?,
                stdout: take(&mut fields, "stdout")?,
                stderr: take(&mut fields, "stderr")?,
                fields,
            }))
        } else {
            Ok(Self::Owner(ValidatedResult { verdict, fields }))
        }
    }
}

impl Serialize for ExecutionResult {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_json().serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for ExecutionResult {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(Value::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn saved_envelopes_round_trip_without_tags_or_field_changes() {
        for value in [
            json!({"verdict":"GREEN","approved":true,"nested":{"a":[1,null]},"truncated":"owner"}),
            json!({"verdict":"GREEN","derived":true}),
            json!({"verdict":"RED","exitCode":7,"durationMs":123,"stdout":"out","stderr":"err","truncated":true}),
            json!({"verdict":"RED","exitCode":7,"durationMs":123,"truncated":false}),
            json!({"verdict":"RED","exitCode":null,"durationMs":null,"truncated":null,"future":[]}),
            json!({"verdict":"GREEN","durationMs":u64::MAX}),
        ] {
            let text = serde_json::to_string(&value).unwrap();
            let result: ExecutionResult = serde_json::from_str(&text).unwrap();
            assert_eq!(serde_json::to_string(&result).unwrap(), text);
        }
    }

    #[test]
    fn verdict_and_runtime_metadata_are_parsed_at_the_edge() {
        for value in [
            json!({"verdict":"ERROR"}),
            json!({"verdict":"GREEN","exitCode":"0"}),
            json!({"verdict":"RED","durationMs":-1}),
            json!({"verdict":"RED","stdout":[],"truncated":false}),
        ] {
            assert!(ExecutionResult::try_from(value).is_err());
        }
        let result = ExecutionResult::try_from(json!({"verdict":"RED","exitCode":7,"durationMs":123,"truncated":true,"stdout":"out","stderr":"err"})).unwrap();
        let ExecutionResult::Runtime(runtime) = result else {
            panic!("runtime result");
        };
        assert_eq!(runtime.duration.value(), Some(&Duration::from_millis(123)));
        assert_eq!(
            ExecutionResult::Runtime(runtime.summary()).to_json(),
            json!({"verdict":"RED","exitCode":7,"durationMs":123,"truncated":true})
        );
    }
}
