//! Mutually exclusive saved success/failure envelopes, with the original JSON keys.
use crate::agent::verdict::ValidatedResult;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default)]
pub enum End {
    #[default]
    Empty,
    /// The review's typed verdict; only the owner-defined fields stay JSON.
    Completed(ValidatedResult),
    Failed {
        code: Option<crate::agent::error::Code>,
        message: Option<String>,
    },
}
#[derive(Debug, Clone, Default)]
pub enum Answer {
    #[default]
    Empty,
    Completed(String),
    Failed {
        code: Option<crate::agent::error::Code>,
        message: Option<String>,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Wire<T> {
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_code: Option<crate::agent::error::Code>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}
// Every field is optional, whatever the result type.
impl<T> Default for Wire<T> {
    fn default() -> Self {
        Self {
            result: None,
            text: None,
            error_code: None,
            error: None,
        }
    }
}
impl End {
    pub fn result(&self) -> Option<&ValidatedResult> {
        if let Self::Completed(value) = self {
            Some(value)
        } else {
            None
        }
    }
    pub fn failure(&self) -> Option<(Option<crate::agent::error::Code>, &str)> {
        if let Self::Failed {
            code,
            message: Some(message),
        } = self
        {
            Some((*code, message))
        } else {
            None
        }
    }
}
impl Answer {
    pub fn text(&self) -> Option<&str> {
        if let Self::Completed(value) = self {
            Some(value)
        } else {
            None
        }
    }
    pub fn failure(&self) -> Option<(Option<crate::agent::error::Code>, &str)> {
        if let Self::Failed {
            code,
            message: Some(message),
        } = self
        {
            Some((*code, message))
        } else {
            None
        }
    }
}
impl Serialize for End {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let wire = match self {
            Self::Empty => Wire::default(),
            Self::Completed(value) => Wire {
                result: Some(value.clone()),
                ..Wire::default()
            },
            Self::Failed { code, message } => Wire {
                error_code: *code,
                error: message.clone(),
                ..Wire::default()
            },
        };
        wire.serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for End {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = Wire::<ValidatedResult>::deserialize(deserializer)?;
        match (wire.result, wire.error_code, wire.error) {
            (Some(value), None, None) => Ok(Self::Completed(value)),
            (Some(_), _, _) => Err(serde::de::Error::custom(
                "review end cannot succeed and fail",
            )),
            (None, None, None) => Ok(Self::Empty),
            (None, code, message) => Ok(Self::Failed { code, message }),
        }
    }
}
impl Serialize for Answer {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let wire: Wire<Value> = match self {
            Self::Empty => Wire::default(),
            Self::Completed(value) => Wire {
                text: Some(value.clone()),
                ..Wire::default()
            },
            Self::Failed { code, message } => Wire {
                error_code: *code,
                error: message.clone(),
                ..Wire::default()
            },
        };
        wire.serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for Answer {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = Wire::<Value>::deserialize(deserializer)?;
        match (wire.text, wire.error_code, wire.error) {
            (Some(value), None, None) => Ok(Self::Completed(value)),
            (Some(_), _, _) => Err(serde::de::Error::custom("answer cannot succeed and fail")),
            (None, None, None) => Ok(Self::Empty),
            (None, code, message) => Ok(Self::Failed { code, message }),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_success_error_and_empty_envelopes_round_trip() {
        for wire in [
            serde_json::json!({}),
            serde_json::json!({"result":{"verdict":"GREEN"}}),
            serde_json::json!({"errorCode":"TIMEOUT","error":"timed out"}),
        ] {
            assert_eq!(
                serde_json::to_value(serde_json::from_value::<End>(wire.clone()).unwrap()).unwrap(),
                wire
            );
        }
        for wire in [
            serde_json::json!({}),
            serde_json::json!({"text":"answer"}),
            serde_json::json!({"errorCode":"TIMEOUT","error":"timed out"}),
        ] {
            assert_eq!(
                serde_json::to_value(serde_json::from_value::<Answer>(wire.clone()).unwrap())
                    .unwrap(),
                wire
            );
        }
        assert!(
            serde_json::from_value::<Answer>(serde_json::json!({"text":"answer","error":"failed"}))
                .is_err()
        );
        assert!(
            serde_json::from_value::<End>(
                serde_json::json!({"result":{"verdict":"RED"},"error":"failed"})
            )
            .is_err()
        );
        assert!(serde_json::from_value::<End>(serde_json::json!({"result":{}})).is_err());
    }
}
