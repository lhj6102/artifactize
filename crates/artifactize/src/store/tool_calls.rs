use serde_json::{Value, json};

use super::receipts::Error;

/// One saved projection for the in-process Agent audit and the durable per-call audit
/// that 0.4 Claude CLI reviews wrote through MCP; those saved records stay readable.
pub(super) fn project(
    db: &rusqlite::Connection,
    execution: Option<&str>,
    saved: &[Value],
) -> Result<Vec<Value>, Error> {
    let mut records = Vec::new();
    // Read-only queries must continue to work with databases created before that table.
    if let Some(execution) = execution
        && db.table_exists(None, "tool_calls")?
    {
        let mut statement =
            db.prepare("SELECT data FROM tool_calls WHERE execution_id=? ORDER BY ordinal")?;
        records = statement
            .query_map([execution], |r| r.get::<_, String>(0))?
            .map(|row| Ok(serde_json::from_str(&row?)?))
            .collect::<Result<_, Error>>()?;
    }
    if records.is_empty() {
        records = saved.to_vec();
    }
    for (index, record) in records.iter_mut().enumerate() {
        if let Some(record) = record.as_object_mut() {
            record.entry("order").or_insert(json!(index + 1));
            let error = if record.get("isError") == Some(&Value::Bool(true)) {
                record
                    .get("result")
                    .filter(|v| !v.is_null())
                    .cloned()
                    .unwrap_or(json!("Tool call did not complete."))
            } else {
                Value::Null
            };
            record.entry("error").or_insert(error);
        }
    }
    Ok(records)
}
