//! Provider-safe final-answer schemas. The original owner schema remains authoritative.
//!
//! OpenAI requires a closed object root (not a root anyOf), and all properties required.
//! Anthropic also requires closed objects, but supports fewer validation keywords:
//! https://developers.openai.com/api/docs/guides/structured-outputs#supported-schemas
//! https://platform.claude.com/docs/en/build-with-claude/structured-outputs#json-schema-support

use serde_json::{Map, Value, json};

use crate::config::Backend;

mod limits;

pub(in crate::agent) struct Structured {
    pub schema: Value,
    green: Shape,
    red: Shape,
}

impl Structured {
    pub fn new(branches: &Value, backend: Backend) -> Self {
        let (green_schema, green) = project_branch(&branches["GREEN"], backend);
        let (red_schema, red) = project_branch(&branches["RED"], backend);
        let schema = envelope(green_schema, red_schema);
        if backend != Backend::Anthropic && !limits::fits(&schema) {
            let (green_schema, green) = encoded(&branches["GREEN"]);
            let (red_schema, red) = encoded(&branches["RED"]);
            return Self {
                schema: envelope(green_schema, red_schema),
                green,
                red,
            };
        }
        Self { schema, green, red }
    }

    /// Only the provider envelope is decoded; Human submissions and stored results stay flat.
    pub fn decode(&self, mut value: Value) -> Result<Value, &'static str> {
        if value.get("verdict").is_some() {
            // The local safety net also handles nonconforming providers and older transcripts.
            return Ok(value);
        }
        let object = value
            .as_object_mut()
            .filter(|object| object.len() == 1)
            .ok_or("schema_mismatch: return an object containing only result")?;
        let mut result = object
            .remove("result")
            .ok_or("schema_mismatch: missing result")?;
        if let Some(text) = result.as_str() {
            result = serde_json::from_str(text)
                .map_err(|_| "not_json: encoded result must contain valid JSON")?;
        }
        let verdict = result
            .get("verdict")
            .and_then(Value::as_str)
            .ok_or("schema_mismatch: verdict must be GREEN or RED")?
            .to_owned();
        let shape = match verdict.as_str() {
            "GREEN" => &self.green,
            "RED" => &self.red,
            _ => return Err("schema_mismatch: verdict must be GREEN or RED"),
        };
        if result.get("verdictFields").is_some() {
            let object = result
                .as_object_mut()
                .filter(|object| object.len() == 2)
                .ok_or("schema_mismatch: result must contain verdict and verdictFields")?;
            let mut fields = object.remove("verdictFields").expect("field checked");
            shape.decode(&mut fields)?;
            let object = fields
                .as_object_mut()
                .ok_or("schema_mismatch: verdictFields must decode to an object")?;
            if object
                .get("verdict")
                .is_some_and(|value| value != &json!(verdict))
            {
                return Err("schema_mismatch: encoded verdict contradicts verdict");
            }
            object.insert("verdict".into(), json!(verdict));
            Ok(fields)
        } else {
            // Local validation remains a safety net for older/nonconforming providers.
            shape.decode(&mut result)?;
            Ok(result)
        }
    }
}

fn variant(verdict: &str, mut fields: Value) -> Value {
    if let Some(properties) = fields.get_mut("properties").and_then(Value::as_object_mut) {
        properties.remove("verdict");
        if let Some(required) = fields.get_mut("required").and_then(Value::as_array_mut) {
            required.retain(|name| name != "verdict");
        }
    }
    // Constrained output follows schema key order. Both alternatives must start with
    // the same discriminator: otherwise alphabetical owner keys can force a branch
    // before the model can choose GREEN/RED (e.g. summary vs verdict/violations).
    // verdict is a prefix of verdictFields, so it sorts first even in a sorted map.
    json!({
        "type":"object",
        "properties":{"verdict":{"type":"string","enum":[verdict]},"verdictFields":fields},
        "required":["verdict","verdictFields"],"additionalProperties":false,
    })
}

fn envelope(green: Value, red: Value) -> Value {
    json!({
        "type":"object",
        "properties":{"result":{"anyOf":[variant("GREEN",green),variant("RED",red)]}},
        "required":["result"],
        "additionalProperties":false,
    })
}

fn encoded(schema: &Value) -> (Value, Shape) {
    (
        json!({
            "type":"string",
            "description":format!("Return this value encoded as JSON text, not prose. Its original JSON Schema is {schema}. Resolve local references against the enclosing original owner schema."),
        }),
        Shape::Json,
    )
}

/// A transport codec, not a validator. It reverses only changes made by projection.
enum Shape {
    Scalar,
    Json,
    Object(Vec<Field>),
    Array(Box<Shape>),
}

struct Field {
    name: String,
    /// Strict output uses null as the placeholder for an absent, non-nullable field.
    omit_null: bool,
    shape: Shape,
}

impl Shape {
    fn is_identity(&self) -> bool {
        match self {
            Self::Scalar => true,
            Self::Json => false,
            Self::Object(fields) => fields
                .iter()
                .all(|field| !field.omit_null && field.shape.is_identity()),
            Self::Array(shape) => shape.is_identity(),
        }
    }

    fn decode(&self, value: &mut Value) -> Result<(), &'static str> {
        match self {
            Self::Json => {
                if let Some(text) = value.as_str() {
                    *value = serde_json::from_str(text)
                        .map_err(|_| "not_json: encoded owner field must contain valid JSON")?;
                }
            }
            Self::Object(fields) => {
                if let Some(object) = value.as_object_mut() {
                    for field in fields {
                        if field.omit_null && object.get(&field.name) == Some(&Value::Null) {
                            object.remove(&field.name);
                        } else if let Some(value) = object.get_mut(&field.name) {
                            field.shape.decode(value)?;
                        }
                    }
                }
            }
            Self::Array(shape) => {
                if let Some(items) = value.as_array_mut() {
                    for item in items {
                        shape.decode(item)?;
                    }
                }
            }
            Self::Scalar => {}
        }
        Ok(())
    }
}

fn project_branch(schema: &Value, backend: Backend) -> (Value, Shape) {
    let (mut projected, shape) = project(schema, backend, schema);
    if has_reference(schema) {
        // Keep reference context once, not once per reference (quadratic schema growth).
        let description = projected["description"].as_str().unwrap_or_default();
        projected["description"] = json!(format!(
            "{description}\nOriginal owner schema for local references: {schema}"
        ));
    }
    (projected, shape)
}

fn has_reference(value: &Value) -> bool {
    match value {
        Value::Object(object) => {
            object.contains_key("$ref")
                || object.contains_key("$dynamicRef")
                || object.values().any(has_reference)
        }
        Value::Array(items) => items.iter().any(has_reference),
        _ => false,
    }
}

fn project(schema: &Value, backend: Backend, root: &Value) -> (Value, Shape) {
    // Open maps, references, unions and untyped values cannot always be expressed in
    // both providers' closed, nonrecursive subset. Encode just that value as JSON text;
    // decode it before local validation, preserving arbitrary owner JSON without rejecting
    // a declaration or silently restricting its value domain.
    // Native unions need no codec when their branches leave owner values untouched.
    // Otherwise JSON text avoids ambiguous decoding between overlapping branches.
    if let Some(branches) = schema.get("anyOf").and_then(Value::as_array) {
        let projected: Vec<_> = branches
            .iter()
            .map(|branch| project(branch, backend, root))
            .collect();
        if projected.iter().all(|(_, shape)| shape.is_identity()) {
            return (
                json!({
                    "anyOf":projected.into_iter().map(|(schema, _)| schema).collect::<Vec<_>>(),
                    "description":format!("Original owner constraints (locally validated): {schema}"),
                }),
                Shape::Scalar,
            );
        }
    }
    let kind = schema.get("type").and_then(|kind| {
        kind.as_str().or_else(|| {
            let kinds = kind.as_array()?;
            let mut non_null = kinds.iter().filter(|kind| *kind != "null");
            let first = non_null.next()?.as_str()?;
            non_null.next().is_none().then_some(first)
        })
    });
    let composed = ["$ref", "$dynamicRef", "allOf", "anyOf", "oneOf"]
        .iter()
        .any(|key| schema.get(key).is_some());
    let inferred = schema
        .get("const")
        .or_else(|| schema.get("enum")?.as_array()?.first())
        .and_then(value_type);
    let kind = kind.or(inferred);
    let open_object = kind == Some("object")
        && (schema.get("additionalProperties") != Some(&Value::Bool(false))
            || schema.get("patternProperties").is_some());
    let complex_enum = schema
        .get("enum")
        .and_then(Value::as_array)
        .is_some_and(|values| {
            values.iter().any(|value| value_type(value).is_none())
                || schema.get("type").is_none()
                    && values.iter().any(|value| value_type(value) != kind)
        });
    if composed || kind.is_none() || open_object || complex_enum {
        return encoded(schema);
    }
    let mut output = Map::new();
    output.insert(
        "type".into(),
        schema
            .get("type")
            .cloned()
            .unwrap_or_else(|| json!(kind.expect("kind checked"))),
    );
    let mut deferred = Map::new();
    for (key, value) in schema.as_object().into_iter().flatten() {
        match key.as_str() {
            "type" | "properties" | "required" | "additionalProperties" | "items" => {}
            "description" | "title" | "enum" => {
                output.insert(key.clone(), value.clone());
            }
            "const" if value_type(value).is_some() => {}
            // Avoid regex dialect differences. Unsupported constraints remain visible to
            // the model as descriptions, and the unchanged local validator enforces them.
            "minimum" | "maximum" | "exclusiveMinimum" | "exclusiveMaximum" | "multipleOf"
                if backend != Backend::Anthropic
                    && matches!(kind, Some("number" | "integer"))
                    && value.is_number() =>
            {
                output.insert(key.clone(), value.clone());
            }
            "minItems" | "maxItems" if backend != Backend::Anthropic && kind == Some("array") => {
                output.insert(key.clone(), value.clone());
            }
            "minItems" if kind == Some("array") && (value == 0 || value == 1) => {
                output.insert(key.clone(), value.clone());
            }
            "$schema" | "$id" | "$defs" | "definitions" => {}
            _ => {
                deferred.insert(key.clone(), value.clone());
            }
        }
    }
    if let Some(value) = schema
        .get("const")
        .filter(|value| value_type(value).is_some())
    {
        output.insert("enum".into(), json!([value]));
    }
    if !deferred.is_empty() {
        let description = output
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default();
        output.insert(
            "description".into(),
            json!(format!(
                "{description}\nAdditional constraints (locally validated): {}",
                Value::Object(deferred)
            )),
        );
    }
    let shape = match kind {
        Some("object") => {
            let mut properties = Map::new();
            let fields = schema["properties"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(name, child)| {
                    let optional = !schema["required"]
                        .as_array()
                        .is_some_and(|required| required.contains(&json!(name)));
                    let (mut projected, shape) = project(child, backend, root);
                    let omit_null = optional && !accepts_null(child, root);
                    if optional {
                        projected = json!({"anyOf":[projected,{"type":"null"}]});
                    }
                    properties.insert(name.clone(), projected);
                    Field {
                        name: name.clone(),
                        omit_null,
                        shape,
                    }
                })
                .collect();
            output.insert(
                "required".into(),
                json!(properties.keys().collect::<Vec<_>>()),
            );
            output.insert("properties".into(), Value::Object(properties));
            output.insert("additionalProperties".into(), json!(false));
            Shape::Object(fields)
        }
        Some("array") => {
            let (items, shape) = project(
                schema.get("items").unwrap_or(&Value::Bool(true)),
                backend,
                root,
            );
            output.insert("items".into(), items);
            Shape::Array(Box::new(shape))
        }
        _ => Shape::Scalar,
    };
    (Value::Object(output), shape)
}

fn value_type(value: &Value) -> Option<&'static str> {
    match value {
        Value::String(_) => Some("string"),
        Value::Number(_) => Some("number"),
        Value::Bool(_) => Some("boolean"),
        Value::Null => Some("null"),
        _ => None,
    }
}

fn accepts_null(schema: &Value, root: &Value) -> bool {
    let mut schema = schema.clone();
    if let Some(object) = schema.as_object_mut() {
        for name in ["$defs", "definitions"] {
            if let Some(value) = root.get(name) {
                object.insert(name.into(), value.clone());
            }
        }
    }
    jsonschema::validator_for(&schema).is_ok_and(|validator| validator.is_valid(&Value::Null))
}
