use jsonschema::Validator;
use serde_json::Value;

pub const ARGUMENT_LIMIT: usize = 64 * 1024;

pub(crate) fn compile(schema: &Value) -> Result<Validator, String> {
    if !schema.is_object() || schema["type"] != "object" {
        return Err("Tool inputSchema must declare type object.".into());
    }
    if serde_json::to_vec(schema).map_err(|e| e.to_string())?.len() > 8 * 1024 * 1024 {
        return Err("Tool inputSchema exceeds 8 MiB.".into());
    }
    jsonschema::validator_for(schema).map_err(|error| {
        format!(
            "Invalid tool inputSchema at {}.",
            quoted(&error.schema_path().to_string())
        )
    })
}

pub(super) fn validate(validator: &Validator, args: &Value) -> Result<(), String> {
    if !args.is_object()
        || serde_json::to_vec(args).map_err(|e| e.to_string())?.len() > ARGUMENT_LIMIT
    {
        return Err("Tool arguments must be an object of at most 64 KiB.".into());
    }
    let mut errors = validator.iter_errors(args).take(5).peekable();
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

pub(crate) fn quoted(value: &str) -> String {
    let mut value = serde_json::to_string(value).expect("string is JSON");
    if value.len() > 320 {
        let mut end = 300;
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        value.truncate(end);
        value.push_str("… (truncated)");
    }
    value
}
