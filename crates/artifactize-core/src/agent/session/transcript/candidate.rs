//! Recognize the raw answer corresponding to an authoritative verdict.

use std::collections::BTreeMap;

/// Provider envelopes, null placeholders and JSON-encoded fields are presentation details.
/// Only a matching validated End replaces the raw answer; examples and failed answers stay.
/// An Agent answer is its issues alone; artifactize derived the verdict the End records.
pub(super) fn candidate(text: &str) -> Option<Candidate> {
    if text.len() > crate::agent::verdict::MAX_RESPONSE_BYTES {
        return None;
    }
    let mut value: serde_json::Value = serde_json::from_str(text).ok()?;
    // Conversations saved before the envelope moved to the root wrap it in `result`.
    if value.get("verdict").is_none() && value.get("issues").is_none() && value.get("findings").is_none() {
        if value.as_object()?.len() != 1 {
            return None;
        }
        value = value.get("result")?.clone();
        if let Some(text) = value.as_str() {
            value = serde_json::from_str(text).ok()?;
        }
    }
    if value.as_object()?.len() == 2 && value.get("verdictFields").is_some() {
        let verdict = value.get("verdict")?.clone();
        let mut fields = value.get("verdictFields")?.clone();
        if let Some(text) = fields.as_str() {
            fields = serde_json::from_str(text).ok()?;
        }
        let object = fields.as_object_mut()?;
        if object.get("verdict").is_some_and(|value| value != &verdict) {
            return None;
        }
        object.insert("verdict".into(), verdict);
        value = fields;
    }
    let mut fields = match value {
        serde_json::Value::Object(fields) => fields,
        _ => return None,
    };
    let verdict = match fields.remove("verdict") {
        Some(verdict) => Some(serde_json::from_value(verdict).ok()?),
        None => None,
    };
    Some(Candidate {
        verdict,
        fields: fields
            .into_iter()
            .map(|(key, value)| (key, OwnerValue::from_json(value)))
            .collect(),
    })
}

#[derive(Debug)]
pub(super) struct Candidate {
    /// `None` for issues, whose verdict the answer does not carry.
    verdict: Option<crate::runtime::Verdict>,
    fields: BTreeMap<String, OwnerValue>,
}

impl Candidate {
    pub(super) fn matches(&self, result: &crate::agent::verdict::ValidatedResult) -> bool {
        self.verdict.is_none_or(|verdict| verdict == result.verdict)
            && owner_fields_match(&self.fields, &result.fields())
    }
}

/// Older providers sometimes encoded owner values a second time. Keep the literal
/// string and its structural interpretation, both decoded before the End event.
#[derive(Debug)]
enum OwnerValue {
    Scalar(serde_json::Value),
    Text {
        literal: String,
        decoded: Option<Box<Self>>,
    },
    Object(BTreeMap<String, Self>),
    Array(Vec<Self>),
}

impl OwnerValue {
    fn from_json(value: serde_json::Value) -> Self {
        use serde_json::Value;
        match value {
            Value::String(literal) => {
                let decoded = serde_json::from_str(&literal)
                    .ok()
                    .map(Self::from_json)
                    .map(Box::new);
                Self::Text { literal, decoded }
            }
            Value::Object(fields) => Self::Object(
                fields
                    .into_iter()
                    .map(|(key, value)| (key, Self::from_json(value)))
                    .collect(),
            ),
            Value::Array(items) => Self::Array(items.into_iter().map(Self::from_json).collect()),
            value => Self::Scalar(value),
        }
    }

    fn matches(&self, result: &serde_json::Value) -> bool {
        use serde_json::Value;
        match (self, result) {
            (Self::Scalar(value), result) => value == result,
            (Self::Text { literal, decoded }, result) => {
                result.as_str() == Some(literal.as_str())
                    || decoded.as_ref().is_some_and(|value| value.matches(result))
            }
            (Self::Object(wire), Value::Object(result)) => owner_fields_match(wire, result),
            (Self::Array(wire), Value::Array(result)) => {
                wire.len() == result.len()
                    && wire
                        .iter()
                        .zip(result)
                        .all(|(wire, result)| wire.matches(result))
            }
            _ => false,
        }
    }
}

fn owner_fields_match(
    wire: &BTreeMap<String, OwnerValue>,
    result: &serde_json::Map<String, serde_json::Value>,
) -> bool {
    result
        .iter()
        .all(|(key, value)| wire.get(key).is_some_and(|wire| wire.matches(value)))
        && wire.iter().all(|(key, value)| {
            result.contains_key(key) || matches!(value, OwnerValue::Scalar(serde_json::Value::Null))
        })
}
