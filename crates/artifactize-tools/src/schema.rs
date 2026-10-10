use jsonschema::Validator;
use serde_json::Value;

/// Bound model-generated arguments before JSON Schema validation.
pub const ARGUMENT_LIMIT: usize = 64 * 1024;

/// Bound owner schemas before compilation and provider prompt construction.
const MAX_SCHEMA_BYTES: usize = 8 * 1024 * 1024;
/// Show a handful of actionable failures, rather than every error in an untrusted argument.
const MAX_DIAGNOSTICS: usize = 5;
/// Leave room for a truncation marker in a short diagnostic line.
const MAX_QUOTED_BYTES: usize = 320;
/// Reserve the rest of MAX_QUOTED_BYTES for the explicit truncated-value marker.
const QUOTED_PREFIX_BYTES: usize = 300;

pub fn compile(schema: &Value) -> Result<Validator, String> {
    if !schema.is_object() || schema["type"] != "object" {
        return Err("Tool inputSchema must declare type object.".into());
    }
    if serde_json::to_vec(schema).map_err(|e| e.to_string())?.len() > MAX_SCHEMA_BYTES {
        return Err("Tool inputSchema exceeds 8 MiB.".into());
    }
    jsonschema::validator_for(schema).map_err(|error| {
        format!(
            "Invalid tool inputSchema at {}.",
            quoted(&error.schema_path().to_string())
        )
    })
}

pub fn validate(validator: &Validator, args: &Value) -> Result<(), String> {
    if !args.is_object()
        || serde_json::to_vec(args).map_err(|e| e.to_string())?.len() > ARGUMENT_LIMIT
    {
        return Err("Tool arguments must be an object of at most 64 KiB.".into());
    }
    let mut errors = validator.iter_errors(args).take(MAX_DIAGNOSTICS).peekable();
    if errors.peek().is_none() {
        return Ok(());
    }
    let mut message = "Tool arguments do not match the registered input schema.".to_owned();
    for error in errors {
        message.push_str(&format!(
            "\n- instancePath {}: constraint {} failed.",
            quoted(&error.instance_path().to_string()),
            quoted(&error.schema_path().to_string()),
        ));
    }
    Err(message)
}

pub fn quoted(value: &str) -> String {
    let mut value = serde_json::to_string(value).expect("string is JSON");
    if value.len() > MAX_QUOTED_BYTES {
        let mut end = QUOTED_PREFIX_BYTES;
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        value.truncate(end);
        value.push_str("… (truncated)");
    }
    value
}
