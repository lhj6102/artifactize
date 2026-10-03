use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value, json};

use super::{Attempt, counters, total};

const LINE_LIMIT: usize = 8 * 1024 * 1024;
const STREAM_LIMIT: usize = 32 * 1024 * 1024;

#[derive(Default)]
struct Turn {
    usage: Map<String, Value>,
    content: Vec<Value>,
    stop: Option<String>,
}

pub(super) struct Stream {
    model: String,
    tools: BTreeSet<String>,
    initialized: bool,
    pending: Vec<u8>,
    bytes: usize,
    turns: Vec<Turn>,
    ids: BTreeMap<String, usize>,
    calls: BTreeMap<String, (usize, Value)>,
    current: Option<usize>,
    result: Option<Value>,
    terminal_usage: Map<String, Value>,
    model_usage: Map<String, Value>,
    limit: Option<u64>,
    prior_tokens: u64,
    pub error: Option<String>,
    pub transcript: Vec<Value>,
}

impl Stream {
    pub fn new(
        model: &str,
        tools: BTreeSet<String>,
        limit: Option<u64>,
        prior_tokens: u64,
    ) -> Self {
        Self {
            model: model.into(),
            tools,
            initialized: false,
            pending: Vec::new(),
            bytes: 0,
            turns: Vec::new(),
            ids: BTreeMap::new(),
            calls: BTreeMap::new(),
            current: None,
            result: None,
            terminal_usage: Map::new(),
            model_usage: Map::new(),
            limit,
            prior_tokens,
            error: None,
            transcript: Vec::new(),
        }
    }

    pub fn push(&mut self, bytes: &[u8]) {
        if self.error.is_some() {
            return;
        }
        self.bytes = self.bytes.saturating_add(bytes.len());
        if self.bytes > STREAM_LIMIT {
            self.error = Some("Claude stream exceeds 32 MiB.".into());
            return;
        }
        for part in bytes.split_inclusive(|byte| *byte == b'\n') {
            self.pending.extend_from_slice(part);
            if self.pending.len() > LINE_LIMIT {
                self.error = Some("Claude stream line exceeds 8 MiB.".into());
                return;
            }
            if part.last() == Some(&b'\n') {
                let line = std::mem::take(&mut self.pending);
                let result = serde_json::from_slice(&line)
                    .map_err(|e| format!("Invalid Claude stream JSON: {e}."))
                    .and_then(|event| self.event(event));
                if let Err(error) = result {
                    self.error = Some(error);
                    return;
                }
            }
        }
    }

    pub fn end(&mut self) {
        if self.error.is_none() && !self.pending.is_empty() {
            let line = std::mem::take(&mut self.pending);
            if let Err(error) = serde_json::from_slice(&line)
                .map_err(|e| format!("Incomplete Claude stream JSON: {e}."))
                .and_then(|event| self.event(event))
            {
                self.error = Some(error);
            }
        }
    }

    fn model(&self, actual: Option<&str>) -> Result<(), String> {
        if actual != Some(self.model.as_str()) {
            return Err(format!(
                "Claude model mismatch: requested {:?}, received {actual:?}.",
                self.model
            ));
        }
        Ok(())
    }

    fn message(&mut self, message: &Value) -> Result<usize, String> {
        let id = message["id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or("Claude assistant message has no ID.")?;
        let index = if let Some(index) = self.ids.get(id) {
            *index
        } else {
            let index = self.turns.len();
            self.turns.push(Turn::default());
            self.ids.insert(id.into(), index);
            index
        };
        let usage = counters(&message["usage"], false)?;
        merge(&mut self.turns[index].usage, usage);
        if let Some(model) = message["model"].as_str() {
            self.turns[index].usage.insert("model".into(), json!(model));
        }
        self.model(message["model"].as_str())?;
        if let Some(stop) = message["stop_reason"].as_str() {
            self.turns[index].stop = Some(stop.into());
        }
        Ok(index)
    }

    fn event(&mut self, event: Value) -> Result<(), String> {
        let kind = event["type"]
            .as_str()
            .ok_or("Claude stream event has no type.")?;
        if self.result.is_some() {
            return Err("Claude emitted data after its terminal result.".into());
        }
        if event
            .get("parent_tool_use_id")
            .is_some_and(|value| !value.is_null())
        {
            return Err("Claude spawned an undeclared subagent.".into());
        }
        match kind {
            "system" if event["subtype"] == "init" => {
                if self.initialized {
                    return Err("Duplicate Claude system/init.".into());
                }
                let tools = event["tools"]
                    .as_array()
                    .ok_or("Claude system/init has no tools list.")?;
                let mut actual: BTreeSet<String> = tools
                    .iter()
                    .map(|tool| {
                        tool.as_str()
                            .map(str::to_owned)
                            .ok_or("Invalid Claude init tool.")
                    })
                    .collect::<Result<_, _>>()?;
                if actual.len() != tools.len() {
                    return Err("Duplicate Claude init tool.".into());
                }
                // The CLI retains this control-only tool whenever any MCP tool is available.
                if !self.tools.is_empty() {
                    actual.remove("EndConversation");
                }
                if actual != self.tools {
                    return Err(format!(
                        "Claude init tools mismatch: expected {:?}, received {actual:?}.",
                        self.tools
                    ));
                }
                self.model(event["model"].as_str())?;
                for key in ["mcp_server_errors", "plugin_errors"] {
                    if event[key]
                        .as_array()
                        .is_some_and(|errors| !errors.is_empty())
                    {
                        return Err(format!("Claude initialization failed: {}.", event[key]));
                    }
                }
                self.initialized = true;
            }
            "system" if event["subtype"] == "api_retry" => {
                return Err(format!("Unexpected Claude retry: {}.", event["error"]));
            }
            "system" if event["subtype"] == "compact_boundary" => {
                return Err("Claude compacted the review context.".into());
            }
            "stream_event" | "assistant" | "user" | "result" if !self.initialized => {
                return Err("Claude output preceded system/init.".into());
            }
            "stream_event" => {
                let value = &event["event"];
                match value["type"].as_str() {
                    Some("message_start") => self.current = Some(self.message(&value["message"])?),
                    Some("message_delta") => {
                        let index = self
                            .current
                            .ok_or("Claude message_delta has no assistant turn.")?;
                        merge(
                            &mut self.turns[index].usage,
                            counters(&value["usage"], false)?,
                        );
                        if let Some(stop) = value["delta"]["stop_reason"].as_str() {
                            self.turns[index].stop = Some(stop.into());
                        }
                    }
                    Some("message_stop") => {
                        self.current = None;
                    }
                    Some("error") => {
                        return Err(format!("Claude stream error: {}.", value["error"]));
                    }
                    _ => {}
                }
            }
            "assistant" => {
                if let Some(error) = event.get("error") {
                    return Err(format!("Claude assistant error: {error}."));
                }
                let index = self.message(&event["message"])?;
                let content = event["message"]["content"]
                    .as_array()
                    .ok_or("Claude assistant has no content.")?;
                let mut added = false;
                let mut block_ids = BTreeSet::new();
                for block in content {
                    if block["type"] == "tool_use" {
                        let id = block["id"]
                            .as_str()
                            .filter(|id| !id.is_empty())
                            .ok_or("Claude tool call has no ID.")?;
                        if !block_ids.insert(id)
                            || self
                                .calls
                                .get(id)
                                .is_some_and(|(turn, saved)| *turn != index || saved != block)
                        {
                            return Err("Claude repeated a tool-call ID.".into());
                        }
                        self.calls.insert(id.into(), (index, block.clone()));
                        let name = block["name"]
                            .as_str()
                            .ok_or("Claude tool call has no name.")?;
                        if !self.tools.contains(name)
                            && !(name == "EndConversation" && !self.tools.is_empty())
                        {
                            return Err(format!("Claude attempted undeclared tool {name:?}."));
                        }
                    }
                    if !self.turns[index].content.contains(block) {
                        self.turns[index].content.push(block.clone());
                        added = true;
                    }
                }
                if added {
                    self.transcript.push(event.clone());
                }
            }
            "user" => self.transcript.push(event.clone()),
            "result" => {
                self.result = Some(event.clone());
                self.terminal_usage = counters(&event["usage"], false)?;
                if let Some(models) = event["modelUsage"].as_object() {
                    for (model, usage) in models {
                        self.model(Some(model))?;
                        self.model_usage = counters(usage, true)?;
                        // modelUsage names cache counters differently from per-message usage.
                        for (source, target) in [
                            ("cacheReadInputTokens", "cacheReadTokens"),
                            ("cacheCreationInputTokens", "cacheWriteTokens"),
                        ] {
                            if let Some(value) = usage.get(source) {
                                let count = value
                                    .as_u64()
                                    .ok_or("Invalid Claude model usage counter.")?;
                                self.model_usage.insert(target.into(), json!(count));
                            }
                        }
                    }
                }
                if event["subtype"] != "success" || event["is_error"] != false {
                    return Err(format!(
                        "Claude terminal error: {} {}.",
                        event["subtype"],
                        event
                            .get("errors")
                            .or_else(|| event.get("result"))
                            .unwrap_or(&Value::Null)
                    ));
                }
            }
            _ => {}
        }
        if self
            .limit
            .is_some_and(|limit| self.prior_tokens.saturating_add(self.tokens()) > limit)
        {
            return Err("PROVIDER_BUDGET_EXCEEDED: review exceeded its maxTokens budget.".into());
        }
        Ok(())
    }

    pub fn tokens(&self) -> u64 {
        self.turns
            .iter()
            .fold(0_u64, |sum, turn| sum.saturating_add(total(&turn.usage)))
            .max(total(&self.terminal_usage))
            .max(total(&self.model_usage))
    }

    pub fn finish(&self) -> Result<String, String> {
        let result = self
            .result
            .as_ref()
            .ok_or("Claude did not emit a successful terminal result.")?;
        let last = self
            .turns
            .last()
            .ok_or("Claude emitted no assistant response.")?;
        if last.stop.as_deref() != Some("end_turn") {
            return Err(format!("Claude response was incomplete: {:?}.", last.stop));
        }
        let text: String = last
            .content
            .iter()
            .filter(|block| block["type"] == "text")
            .filter_map(|block| block["text"].as_str())
            .collect();
        if result["result"].as_str() != Some(text.as_str()) {
            return Err("Claude terminal result does not match final assistant text.".into());
        }
        Ok(text)
    }

    pub fn record(&self, attempts: &mut Vec<Attempt>, invocation: usize, error: Option<&String>) {
        let offset = attempts.len();
        for (index, turn) in self.turns.iter().enumerate() {
            let mut usage = turn.usage.clone();
            if usage.values().any(Value::is_u64) {
                usage.insert("totalTokens".into(), json!(total(&usage)));
            }
            usage.insert("invocation".into(), json!(invocation));
            attempts.push(Attempt {
                turn: offset + index + 1,
                attempt: 1,
                usage,
                error: None,
            });
        }
        if self.turns.is_empty() {
            attempts.push(Attempt {
                turn: offset + 1,
                attempt: 1,
                usage: Map::new(),
                error: None,
            });
        }
        let last = attempts.last_mut().unwrap();
        last.error = error.cloned();
        if let Some(result) = &self.result {
            last.usage.insert("invocationTotals".into(), json!({"usage":result["usage"], "modelUsage":result["modelUsage"], "totalCostUsd":result["total_cost_usd"]}));
        }
    }
}

fn merge(target: &mut Map<String, Value>, usage: Map<String, Value>) {
    for (name, value) in usage {
        let previous = target.get(&name).and_then(Value::as_u64).unwrap_or(0);
        target.insert(name, json!(previous.max(value.as_u64().unwrap())));
    }
}
