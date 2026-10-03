use jsonschema::Validator;
use serde_json::{Map, Value, json};

const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_RESULT_CHARS: usize = 256_000;
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
    "toolCalls",
];

pub(crate) fn validate_schema(schema: &Map<String, Value>) -> Result<(), String> {
    let value = Value::Object(schema.clone());
    if schema.get("type").is_some_and(|kind| kind != "object") {
        return Err("Response schema type must be object when declared.".into());
    }
    if serde_json::to_vec(&value).expect("schema is JSON").len() > 8 * 1024 * 1024 {
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
        let (validator, branch) = match value.get("verdict").and_then(Value::as_str) {
            Some("GREEN") => (&self.green, &self.schema["GREEN"]),
            Some("RED") => (&self.red, &self.schema["RED"]),
            _ => return Err("schema_mismatch: verdict must be GREEN or RED"),
        };
        // Owner schemas cannot open the result envelope through patternProperties or $ref.
        if !value.as_object().is_some_and(|object| {
            object
                .keys()
                .all(|key| branch["properties"].get(key).is_some())
        }) || !validator.is_valid(&value)
        {
            return Err("schema_mismatch: result must match the selected verdict's owner schema");
        }
        // Audit and usage live in separate rows; this bounds the persisted semantic result.
        if serde_json::to_string(&value)
            .expect("result is JSON")
            .encode_utf16()
            .count()
            > MAX_RESULT_CHARS
        {
            return Err("over_size: normalized result exceeds 256000 characters");
        }
        Ok(value)
    }
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
