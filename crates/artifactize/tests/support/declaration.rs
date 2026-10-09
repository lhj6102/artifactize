//! Write real TOML declarations while keeping JSON builders convenient in tests.

use std::{fs, io, path::Path};

use serde_json::Value;

/// Test builders may use an eval array; only the writer turns ids into table keys.
pub fn to_toml(mut value: Value) -> Result<String, String> {
    if let Some(evals) = value.get_mut("evals")
        && let Some(items) = evals.as_array_mut()
    {
        let mut table = serde_json::Map::new();
        for mut eval in std::mem::take(items) {
            let id = eval
                .as_object_mut()
                .and_then(|eval| eval.remove("id"))
                .and_then(|id| id.as_str().map(str::to_owned))
                .ok_or("Test eval requires a string id.")?;
            if table.insert(id.clone(), eval).is_some() {
                return Err(format!(
                    "Duplicate local Eval in {}: {id}",
                    value["name"].as_str().unwrap_or("?")
                ));
            }
        }
        *evals = Value::Object(table);
    }
    toml::to_string_pretty(&value).map_err(|error| error.to_string())
}

/// Read a declaration back into the array-shaped test builder for small mutations.
pub fn read(contents: impl AsRef<[u8]>) -> Result<Value, String> {
    let source = std::str::from_utf8(contents.as_ref()).map_err(|error| error.to_string())?;
    let mut value: Value = toml::from_str(source).map_err(|error| error.to_string())?;
    if let Some(evals) = value.get_mut("evals")
        && let Some(table) = evals.as_object_mut()
    {
        let mut items = Vec::new();
        for (id, mut eval) in std::mem::take(table) {
            eval.as_object_mut()
                .ok_or("Eval must be a table.")?
                .insert("id".into(), Value::String(id));
            items.push(eval);
        }
        *evals = Value::Array(items);
    }
    Ok(value)
}

/// Ordinary fixture files are untouched; JSON declaration builders become TOML.
pub fn write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> io::Result<()> {
    let path = path.as_ref();
    if path.file_name().is_some_and(|name| name == "index.artf")
        && let Ok(value) = serde_json::from_slice(contents.as_ref())
    {
        let declaration = to_toml(value).map_err(io::Error::other)?;
        fs::write(path, declaration)
    } else {
        fs::write(path, contents)
    }
}
