use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

use super::{Receipts, receipts::Error};
use crate::tools::ToolResult;

impl Receipts {
    /// A session may precede the execution row (keyless reviews insert it at completion).
    pub async fn register_mcp(
        &self,
        execution: &str,
        binding: &Value,
        max_calls: Option<u64>,
    ) -> Result<(), String> {
        let execution = execution.to_owned();
        let binding = binding.to_string();
        let max_calls = max_calls.map(|n| n.min(i64::MAX as u64) as i64);
        self.connection.call(move |db| -> Result<(), Error> {
            let transaction = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            transaction.execute("INSERT INTO mcp_sessions(execution_id,binding,max_calls,started) VALUES (?,?,?,0) ON CONFLICT DO NOTHING", params![execution, binding, max_calls])?;
            let saved: (String, Option<i64>) = transaction.query_row("SELECT binding,max_calls FROM mcp_sessions WHERE execution_id=?", [&execution], |r| Ok((r.get(0)?,r.get(1)?)))?;
            if saved != (binding, max_calls) { return Err(Error::Invalid("MCP execution scope or budget changed.".into())); }
            transaction.commit()?;
            Ok(())
        }).await.map_err(|e| e.to_string())
    }

    /// Persist admission and an unfinished audit record atomically, before running any tool.
    pub async fn begin_tool_call(
        &self,
        execution: &str,
        name: &str,
        arguments: &Value,
    ) -> Result<(i64, Option<String>), String> {
        let execution = execution.to_owned();
        let name = name.to_owned();
        let arguments = arguments.clone();
        self.connection.call(move |db| -> Result<_, Error> {
            let transaction = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let (started, max): (i64, Option<i64>) = transaction.query_row("SELECT started,max_calls FROM mcp_sessions WHERE execution_id=?", [&execution], |r| Ok((r.get(0)?, r.get(1)?)))?;
            let status: Option<String> = transaction.query_row("SELECT status FROM executions WHERE id=?", [&execution], |r| r.get(0)).optional()?;
            let denied = if status.is_some_and(|s| s != "RUNNING") {
                Some("Execution is not active.".to_owned())
            } else if max.is_some_and(|n| started >= n) || started == i64::MAX {
                Some("Agent maxToolCalls budget exhausted.".to_owned())
            } else { None };
            let order: i64 = transaction.query_row("SELECT COALESCE(MAX(ordinal),0)+1 FROM tool_calls WHERE execution_id=?", [&execution], |r| r.get(0))?;
            let record = json!({"order":order,"name":name,"arguments":arguments,"result":null,"isError":true,"error":denied.as_deref().unwrap_or("Tool call did not complete.")});
            transaction.execute("INSERT INTO tool_calls(execution_id,ordinal,data) VALUES (?,?,?)", params![execution, order, record.to_string()])?;
            if denied.is_none() {
                transaction.execute("UPDATE mcp_sessions SET started=started+1 WHERE execution_id=?", [&execution])?;
            }
            transaction.commit()?;
            Ok((order, denied))
        }).await.map_err(|e| e.to_string())
    }

    pub async fn finish_tool_call(
        &self,
        execution: &str,
        order: i64,
        result: &ToolResult,
    ) -> Result<(), String> {
        let execution = execution.to_owned();
        let summary: String = serde_json::to_string(result)
            .map_err(|e| e.to_string())?
            .chars()
            .take(4096)
            .collect();
        let error = result.is_error.then(|| summary.clone());
        let is_error = result.is_error;
        self.connection.call(move |db| -> Result<(), Error> {
            if db.execute("UPDATE tool_calls SET data=json_set(data,'$.result',?,'$.isError',json(?),'$.error',?) WHERE execution_id=? AND ordinal=?", params![summary, is_error.to_string(), error, execution, order])? != 1 {
                return Err(Error::Invalid("Tool call audit record not found.".into()));
            }
            Ok(())
        }).await.map_err(|e| e.to_string())
    }

    pub async fn tool_calls(&self, execution: &str) -> Result<Vec<Value>, String> {
        let execution = execution.to_owned();
        self.connection
            .call(move |db| project(db, Some(&execution), &[]))
            .await
            .map_err(|e| e.to_string())
    }
}

/// One saved projection for both in-process Agent audit and durable MCP audit.
pub(super) fn project(
    db: &rusqlite::Connection,
    execution: Option<&str>,
    saved: &[Value],
) -> Result<Vec<Value>, Error> {
    let mut records = Vec::new();
    // Read-only queries must continue to work with databases created before MCP was added.
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
