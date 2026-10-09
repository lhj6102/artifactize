//! `session show --summary`: what a saved conversation spent and did, computed from its
//! session file alone.

use std::collections::BTreeMap;

use rig_core::message::{AssistantContent, Message, UserContent};
use serde::Serialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::Conversation;

/// A saved conversation's backend, time span, turns, tokens and tool calls.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub backend: Option<String>,
    pub model: Option<String>,
    pub reasoning: Option<String>,
    /// The first and last event's time.
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub duration_ms: Option<u64>,
    pub follow_ups: usize,
    /// Every provider turn in order: the review's, then each follow-up's.
    pub turns: Vec<Turn>,
    /// The reported token counters of every attempt, summed.
    pub tokens: BTreeMap<String, u64>,
    /// By tool name.
    pub tool_calls: BTreeMap<String, ToolCalls>,
}

/// One provider request and its attempts.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    /// The follow-up the turn answered; absent for the review's turns.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub follow_up: Option<u64>,
    pub turn: u64,
    pub attempts: u64,
    /// The reported token counters of its attempts, summed.
    pub tokens: BTreeMap<String, u64>,
}

#[derive(Debug, Default, Serialize)]
pub struct ToolCalls {
    pub calls: u64,
    /// Calls whose result was an error, and calls never answered because the review or
    /// follow-up stopped first.
    pub failed: u64,
}

impl Summary {
    pub fn new(conversation: &Conversation) -> Self {
        let header = conversation.header();

        let (started_at, ended_at) = (
            conversation
                .events
                .first()
                .and_then(|event| event.at.as_deref()),
            conversation
                .events
                .last()
                .and_then(|event| event.at.as_deref()),
        );
        let instant = |text: &str| OffsetDateTime::parse(text, &Rfc3339).ok();
        let duration_ms = started_at
            .and_then(instant)
            .zip(ended_at.and_then(instant))
            .map(|(start, end)| (end - start).whole_milliseconds().max(0) as u64);
        let mut summary = Self {
            backend: header.backend.map(|backend| {
                serde_json::to_value(backend)
                    .expect("backend is JSON")
                    .as_str()
                    .expect("backend is text")
                    .to_owned()
            }),
            model: header.model.clone(),
            reasoning: header.reasoning.clone(),
            started_at: started_at.map(str::to_owned),
            ended_at: ended_at.map(str::to_owned),
            duration_ms,
            follow_ups: conversation.sends(),
            turns: Vec::new(),
            tokens: BTreeMap::new(),
            tool_calls: BTreeMap::new(),
        };
        // Calls by tool, then the results that answered them and those that failed.
        let mut answered = BTreeMap::<String, (u64, u64)>::new();
        for event in &conversation.events {
            match &event.kind {
                super::Kind::Attempt(attempt) => summary.attempt(event.send, attempt),
                super::Kind::Message(event) => match &event.message {
                    Message::Assistant { content, .. } => {
                        for part in content {
                            if let AssistantContent::ToolCall(call) = part {
                                let name = call.function.name.as_str().to_owned();
                                summary.tool_calls.entry(name).or_default().calls += 1;
                            }
                        }
                    }
                    Message::User { content } => {
                        let results = content.iter().filter_map(|part| match part {
                            UserContent::ToolResult(result) => Some(result),
                            _ => None,
                        });
                        for (index, result) in results.enumerate() {
                            let (count, failed) =
                                answered.entry(result.name.as_str().to_owned()).or_default();
                            *count += 1;
                            *failed += u64::from(event.is_error.get(index) == Some(&true));
                        }
                    }
                    Message::System { .. } => {}
                },
                _ => {}
            }
        }
        for (name, calls) in &mut summary.tool_calls {
            let (count, failed) = answered.get(name).copied().unwrap_or_default();
            calls.failed = failed + calls.calls.saturating_sub(count);
        }
        summary
    }

    /// Count an attempt in its turn and in the totals.
    fn attempt(&mut self, send: Option<usize>, attempt: &crate::llm::Attempt) {
        let follow_up = send.map(|send| send as u64);
        let turn = attempt.turn as u64;
        if self
            .turns
            .last()
            .is_none_or(|last| (last.follow_up, last.turn) != (follow_up, turn))
        {
            self.turns.push(Turn {
                follow_up,
                turn,
                attempts: 0,
                tokens: BTreeMap::new(),
            });
        }
        let current = self.turns.last_mut().expect("the attempt's turn");
        current.attempts += 1;
        for (name, value) in &attempt.usage {
            if let Some(value) = value.as_u64() {
                for tokens in [&mut current.tokens, &mut self.tokens] {
                    let total = tokens.entry(name.clone()).or_default();
                    *total = total.saturating_add(value);
                }
            }
        }
    }
}

/// Token counters as text, in a fixed order with readable names; `None` without any.
pub fn tokens_text(tokens: &BTreeMap<String, u64>) -> Option<String> {
    let parts: Vec<_> = [
        ("inputTokens", "input"),
        ("outputTokens", "output"),
        ("cacheReadTokens", "cache read"),
        ("cacheWriteTokens", "cache write"),
        ("reasoningTokens", "reasoning"),
    ]
    .into_iter()
    .filter_map(|(name, label)| Some(format!("{label} {}", tokens.get(name)?)))
    .collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

#[cfg(test)]
mod tests {
    use rig_core::message::{ToolCall, ToolFunction, ToolName, ToolResultContent};
    use serde_json::json;

    use super::*;

    fn call(id: &str, name: &str) -> ToolCall {
        ToolCall::from_wire(
            id,
            ToolFunction::new(ToolName::new(name).unwrap(), json!({})),
        )
    }

    #[test]
    fn a_summary_counts_turns_tokens_and_failed_calls() {
        let answer = Message::Assistant {
            id: None,
            content: vec![
                AssistantContent::ToolCall(call("c1", "read")),
                AssistantContent::ToolCall(call("c2", "list")),
            ],
        };
        let results = Message::tool_results(
            [call("c1", "read"), call("c2", "list")]
                .into_iter()
                .map(|call| call.result(vec![ToolResultContent::text("ok")]))
                .collect(),
        );
        let unanswered = Message::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(call("c3", "read"))],
        };
        let events = vec![
            json!({"kind":"review","backend":"openai","model":"m","at":"2026-10-06T01:00:00Z"}),
            json!({"kind":"attempt","turn":1,"attempt":1,"usage":{},"error":"busy"}),
            json!({
                "kind":"attempt",
                "turn":1,
                "attempt":2,
                "usage":{"inputTokens":10,"outputTokens":4,"cacheReadTokens":2},
            }),
            json!({"kind":"message","turn":1,"message":answer}),
            json!({"kind":"message","turn":2,"message":results,"isError":[true,false]}),
            json!({
                "kind":"attempt",
                "turn":2,
                "attempt":1,
                "usage":{"inputTokens":20,"outputTokens":6},
            }),
            json!({"kind":"end","result":{"verdict":"GREEN"}}),
            json!({"kind":"send","send":1}),
            json!({
                "kind":"attempt",
                "send":1,
                "turn":1,
                "attempt":1,
                "usage":{"inputTokens":30,"outputTokens":1,"reasoningTokens":1},
            }),
            json!({"kind":"message","send":1,"turn":1,"message":unanswered}),
            json!({"kind":"answer","send":1,"error":"stopped","at":"2026-10-06T01:00:02.5Z"}),
        ];
        let summary = Summary::new(&Conversation {
            path: "session.jsonl".into(),
            wire_events: Vec::new(),
            events: events
                .into_iter()
                .map(|event| serde_json::from_value(event).unwrap())
                .collect(),
        });
        assert_eq!(summary.backend.as_deref(), Some("openai"));
        assert_eq!(summary.duration_ms, Some(2500));
        assert_eq!(summary.follow_ups, 1);
        let turns: Vec<_> = summary
            .turns
            .iter()
            .map(|turn| (turn.follow_up, turn.turn, turn.attempts))
            .collect();
        assert_eq!(turns, [(None, 1, 2), (None, 2, 1), (Some(1), 1, 1)]);
        assert_eq!(
            json!(summary.tokens),
            json!({"inputTokens":60,"outputTokens":11,"cacheReadTokens":2,"reasoningTokens":1})
        );
        assert_eq!(
            json!(summary.tool_calls),
            json!({"list":{"calls":1,"failed":0},"read":{"calls":2,"failed":2}})
        );
        assert_eq!(
            tokens_text(&summary.tokens).unwrap(),
            "input 60 · output 11 · cache read 2 · reasoning 1"
        );
    }
}
