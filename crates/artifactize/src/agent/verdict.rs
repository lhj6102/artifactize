use jsonschema::Validator;
use serde_json::{Map, Value, json};

use crate::{config::EvalDeclaration, tools::schema::quoted};

/// Validate a parsed Agent or Human result without repair or owner-field rewriting.
/// Errors list at most five failing instance paths, so a reviewer can correct the fields.
pub fn validate_result(eval: &EvalDeclaration, value: &Value) -> Result<Value, String> {
    let schema = VerdictSchema::new(eval.pass_schema.as_ref(), eval.fail_schema.as_ref())?;
    schema
        .validate(value)
        .map_err(|error| schema.explain(error, value))?;
    Ok(value.clone())
}

/// Bound the raw model response before parsing; prose/whitespace can exceed the semantic result.
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
/// Keep the normalized owner result bounded in saved receipts, in UTF-16 units for JSON clients.
const MAX_RESULT_CHARS: usize = 256_000;
/// Bound schema compilation and the schema sent in the review prompt.
const MAX_SCHEMA_BYTES: usize = 8 * 1024 * 1024;
/// Five diagnostics give an actionable repair without flooding the next provider turn.
const MAX_DIAGNOSTICS: usize = 5;
/// Bound each diagnostic so an owner property/value cannot dominate the repair prompt.
const MAX_DIAGNOSTIC_CHARS: usize = 200;
const RESERVED: &[&str] = &[
    "verdict",
    "reference",
    "reusedFrom",
    "executionProvenance",
    "attemptId",
    "provider",
    "model",
    "stdout",
    "stderr",
    "durationMs",
    "exitCode",
];

pub(crate) fn validate_schema(schema: &Map<String, Value>) -> Result<(), String> {
    let value = Value::Object(schema.clone());
    if schema.get("type").is_some_and(|kind| kind != "object") {
        return Err("Response schema type must be object when declared.".into());
    }
    if serde_json::to_vec(&value).expect("schema is JSON").len() > MAX_SCHEMA_BYTES {
        return Err("Response schema exceeds 8 MiB.".into());
    }
    if ["allOf", "anyOf", "oneOf", "not"]
        .iter()
        .any(|key| schema.contains_key(*key))
    {
        return Err(
            "Response schemas cannot use top-level composition; compose inside owner fields."
                .into(),
        );
    }
    if schema
        .get("additionalProperties")
        .is_some_and(|v| v != false)
    {
        return Err("Response schemas must forbid undeclared top-level fields.".into());
    }
    for field in RESERVED {
        if value["properties"].get(*field).is_some()
            || value["required"]
                .as_array()
                .is_some_and(|required| required.iter().any(|name| name.as_str() == Some(field)))
        {
            return Err(
                "Response schemas cannot declare or require reserved result fields.".into(),
            );
        }
    }
    jsonschema::validator_for(&value)
        .map(|_| ())
        .map_err(|_| "Invalid response JSON Schema.".into())
}

pub(crate) struct VerdictSchema {
    green: Validator,
    red: Validator,
    pub schema: Value,
}

impl VerdictSchema {
    pub fn new(
        pass: Option<&Map<String, Value>>,
        fail: Option<&Map<String, Value>>,
    ) -> Result<Self, String> {
        let green = branch("GREEN", pass)?;
        let red = branch("RED", fail)?;
        Ok(Self {
            green: jsonschema::validator_for(&green)
                .map_err(|_| "Invalid GREEN response JSON Schema.".to_owned())?,
            red: jsonschema::validator_for(&red)
                .map_err(|_| "Invalid RED response JSON Schema.".to_owned())?,
            schema: json!({"GREEN":green,"RED":red}),
        })
    }

    pub fn parse(&self, text: &str) -> Result<Value, &'static str> {
        if text.len() > MAX_RESPONSE_BYTES {
            return Err("over_size: final JSON exceeds 1 MiB");
        }
        if text.trim().is_empty() {
            return Err("empty: return one JSON object");
        }
        let value: Value = serde_json::from_str(text).map_err(
            |_| "not_json: return exactly one JSON object, without prose or code fences",
        )?;
        self.validate(&value)?;
        Ok(value)
    }

    /// What the single repair turn tells the model: the parse error and, when the
    /// response was a JSON object, its failing instance paths, bounded as for a Human
    /// submission. Sent to the provider only; the persisted error stays the bare code.
    pub fn repair_detail(&self, text: &str, error: &str) -> String {
        let value = (text.len() <= MAX_RESPONSE_BYTES)
            .then(|| serde_json::from_str::<Value>(text).ok())
            .flatten();
        match value {
            Some(value) => self.explain(error, &value),
            None => error.into(),
        }
    }

    fn branch(&self, value: &Value) -> Option<(&Validator, &Value)> {
        match value.get("verdict").and_then(Value::as_str) {
            Some("GREEN") => Some((&self.green, &self.schema["GREEN"])),
            Some("RED") => Some((&self.red, &self.schema["RED"])),
            _ => None,
        }
    }

    /// The error followed by bounded failing paths; undeclared fields when the schema passes.
    fn explain(&self, error: &str, value: &Value) -> String {
        let Some((validator, branch)) = self.branch(value) else {
            return error.into();
        };
        let mut paths: Vec<_> = validator
            .iter_errors(value)
            .map(|error| {
                format!(
                    "- instancePath {}: {}",
                    quoted(&bounded(error.instance_path().to_string())),
                    bounded(error.to_string())
                )
            })
            .take(MAX_DIAGNOSTICS)
            .collect();
        if paths.is_empty() {
            let fields = value.as_object().into_iter().flatten();
            paths = fields
                .filter(|(key, _)| branch["properties"].get(key.as_str()).is_none())
                .map(|(key, _)| {
                    format!("- field {} is not declared.", quoted(&bounded(key.clone())))
                })
                .take(MAX_DIAGNOSTICS)
                .collect();
        }
        std::iter::once(error.to_owned())
            .chain(paths)
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn validate(&self, value: &Value) -> Result<(), &'static str> {
        let (validator, branch) = self
            .branch(value)
            .ok_or("schema_mismatch: verdict must be GREEN or RED")?;
        // Owner schemas cannot open the result envelope through patternProperties or $ref.
        if !value.as_object().is_some_and(|object| {
            object
                .keys()
                .all(|key| branch["properties"].get(key).is_some())
        }) || !validator.is_valid(value)
        {
            return Err("schema_mismatch: result must match the selected verdict's owner schema");
        }
        // Audit and usage live in separate rows; this bounds the persisted semantic result.
        if serde_json::to_string(value)
            .expect("result is JSON")
            .encode_utf16()
            .count()
            > MAX_RESULT_CHARS
        {
            return Err("over_size: normalized result exceeds 256000 characters");
        }
        Ok(())
    }
}

fn bounded(mut text: String) -> String {
    if text.chars().count() > MAX_DIAGNOSTIC_CHARS {
        text = text.chars().take(MAX_DIAGNOSTIC_CHARS).collect::<String>() + "… (truncated)";
    }
    text
}

fn branch(verdict: &str, owner: Option<&Map<String, Value>>) -> Result<Value, String> {
    if let Some(owner) = owner {
        validate_schema(owner)?;
    }
    let mut schema = owner.cloned().unwrap_or_default();
    let properties = schema.entry("properties").or_insert_with(|| json!({}));
    properties
        .as_object_mut()
        .expect("validated properties")
        .insert("verdict".into(), json!({"type":"string","const":verdict}));
    schema
        .entry("required")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .expect("validated required")
        .push(json!("verdict"));
    schema.insert("type".into(), json!("object"));
    schema.insert("additionalProperties".into(), json!(false));
    Ok(Value::Object(schema))
}

#[cfg(test)]
mod tests;
