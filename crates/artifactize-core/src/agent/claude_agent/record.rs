//! Claude Code's messages as the saved conversation records them, its usage as attempt
//! counters, and its failures as review failures: pure conversions of parsed lines.

use std::collections::BTreeMap;

use rig_core::message::{
    AssistantContent, CallId, ImageMediaType, Issuer, Message, Reasoning, ToolName,
    ToolResultContent, UserContent,
};

use crate::{
    agent::error::{Code, Failure},
    llm::claude_agent::{
        command, shown,
        wire::{
            AssistantError, Block, Ending, LimitStatus, RateLimit, ResultBlock, ResultContent,
            Usage,
        },
    },
    types::{ClaudeMessageId, ClaudeToolUseId},
};

/// Who issued the thinking a saved conversation keeps.
const ISSUER: Issuer = Issuer::from_static("claude-agent");

/// A tool's own name: Claude Code's name without its MCP prefix. Claude Code's own tools,
/// such as the final answer's `StructuredOutput`, keep their names.
pub fn tool_name(name: &str) -> &str {
    name.strip_prefix(&command::tool_name("")).unwrap_or(name)
}

/// One response's blocks as the assistant message the conversation saves.
pub fn assistant(id: &ClaudeMessageId, blocks: &[Block]) -> Option<Message> {
    let content: Vec<AssistantContent> = blocks
        .iter()
        .filter_map(|block| match block {
            Block::Text { text } => Some(AssistantContent::text(text.clone())),
            Block::Thinking {
                thinking,
                signature,
            } => Some(AssistantContent::Reasoning(
                Reasoning::new_with_signature(
                    thinking,
                    Some(signature.clone()).filter(|signature| !signature.is_empty()),
                )
                .sealed(ISSUER),
            )),
            Block::ToolUse { id, name, input } => Some(AssistantContent::tool_call(
                id.as_str(),
                ToolName::new(tool_name(name)).ok()?,
                input.clone(),
            )),
            Block::Other => None,
        })
        .collect();
    (!content.is_empty()).then(|| Message::Assistant {
        id: Some(id.to_string()),
        content,
    })
}

/// One tool result: the call it answers, by id and name, its content, and whether it failed.
pub fn tool_result(
    id: &ClaudeToolUseId,
    name: &str,
    content: ResultContent,
) -> Option<UserContent> {
    let content = match content {
        ResultContent::Text(text) => vec![ToolResultContent::text(text)],
        ResultContent::Blocks(blocks) => blocks
            .into_iter()
            .filter_map(|block| match block {
                ResultBlock::Text { text } => Some(ToolResultContent::text(text)),
                ResultBlock::Image { source } => Some(ToolResultContent::image_base64(
                    source.data,
                    Some(match source.media_type.as_str() {
                        "image/png" => ImageMediaType::PNG,
                        "image/jpeg" => ImageMediaType::JPEG,
                        "image/webp" => ImageMediaType::WEBP,
                        "image/gif" => ImageMediaType::GIF,
                        _ => return None,
                    }),
                    None,
                )),
                ResultBlock::Other => None,
            })
            .collect(),
    };
    Some(UserContent::tool_result(
        CallId::from_wire(id.as_str()),
        ToolName::new(tool_name(name)).ok()?,
        content,
    ))
}

/// One response's token counters. Claude Code reports a response's usage several times
/// while it streams; each counter keeps its highest report.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counters {
    input: Option<u64>,
    output: Option<u64>,
    cache_read: Option<u64>,
    cache_write: Option<u64>,
    cache_write_1h: Option<u64>,
    reasoning: Option<u64>,
}

impl Counters {
    pub fn merge(&mut self, usage: &Usage) {
        let highest = |kept: &mut Option<u64>, reported: Option<u64>| {
            *kept = (*kept).max(reported);
        };
        highest(&mut self.input, usage.input_tokens);
        highest(&mut self.output, usage.output_tokens);
        highest(&mut self.cache_read, usage.cache_read_input_tokens);
        highest(&mut self.cache_write, usage.cache_creation_input_tokens);
        highest(
            &mut self.cache_write_1h,
            usage
                .cache_creation
                .and_then(|cache| cache.ephemeral_1h_input_tokens),
        );
        highest(
            &mut self.reasoning,
            usage
                .output_tokens_details
                .and_then(|details| details.thinking_tokens),
        );
    }

    /// The input as other backends count it: uncached input with cache reads and writes.
    fn input_total(&self) -> Option<u64> {
        self.input.map(|uncached| {
            uncached
                .saturating_add(self.cache_read.unwrap_or(0))
                .saturating_add(self.cache_write.unwrap_or(0))
        })
    }

    /// Every token of the response, as `maxTokens` counts them.
    pub fn total(&self) -> u64 {
        self.input_total()
            .unwrap_or(0)
            .saturating_add(self.output.unwrap_or(0))
    }

    /// The counters an attempt records, named as for every backend.
    pub fn usage(&self) -> BTreeMap<String, u64> {
        [
            ("inputTokens", self.input_total()),
            ("outputTokens", self.output),
            ("totalTokens", self.input.map(|_| self.total())),
            ("cacheReadTokens", self.cache_read),
            ("cacheWriteTokens", self.cache_write),
            ("cacheWrite1hTokens", self.cache_write_1h),
            ("reasoningTokens", self.reasoning),
        ]
        .into_iter()
        .filter_map(|(name, value)| {
            value
                .filter(|value| *value <= crate::types::MAX_SAFE_JSON_INTEGER)
                .map(|value| (name.to_owned(), value))
        })
        .collect()
    }
}

/// Why Claude Code could not answer, with the text it wrote and the subscription limit it
/// last reported, if any.
pub fn assistant_failure(error: AssistantError, text: &str, limit: Option<&RateLimit>) -> Failure {
    match error {
        AssistantError::AuthenticationFailed => Failure::new(
            Code::Authentication,
            "Claude Code is not signed in, or its login expired; run `claude auth login`.",
        ),
        AssistantError::BillingError => Failure::new(
            Code::Quota,
            "Claude Code reports a billing problem; check the Claude plan of the account `claude auth status` shows.",
        ),
        AssistantError::RateLimit => limit.map_or_else(
            || Failure::new(Code::Quota, "Claude Code subscription limit reached."),
            limit_failure,
        ),
        AssistantError::ServerError => Failure::new(
            Code::Transient,
            format!("Claude Code's service failed: {}", shown(text)),
        ),
        AssistantError::InvalidRequest | AssistantError::Unknown => Failure::new(
            Code::ProviderError,
            format!("Claude Code could not answer: {}", shown(text)),
        ),
    }
}

/// A subscription limit Claude Code reported as reached, with its window and reset time.
pub fn limit_failure(limit: &RateLimit) -> Failure {
    let window = match limit.rate_limit_type.as_deref() {
        Some("five_hour") => "the 5-hour window".to_owned(),
        Some("seven_day") => "the weekly window".to_owned(),
        Some(other) => format!("the {} window", other.replace('_', " ")),
        None => "its window".to_owned(),
    };
    let reset = limit
        .resets_at
        .and_then(|at| {
            at.format(&time::format_description::well_known::Rfc3339)
                .ok()
        })
        .map_or_else(String::new, |at| format!("; it resets at {at}"));
    Failure::new(
        Code::Quota,
        format!("Claude Code subscription limit reached in {window}{reset}."),
    )
}

/// Whether a limit report stops the review.
pub fn reached(limit: &RateLimit) -> bool {
    limit.status == LimitStatus::Rejected
}

/// The end of a turn that did not succeed.
pub fn finish_failure(ending: &Ending, errors: &[String]) -> Failure {
    let detail = shown(&errors.join(" "));
    let subtype = match ending {
        Ending::NoStructuredOutput => {
            return Failure::new(
                Code::InvalidResult,
                format!("Claude Code gave no answer matching the verdict schema. {detail}"),
            );
        }
        Ending::Other(subtype) => subtype,
    };
    Failure::new(
        Code::ProviderError,
        format!("Claude Code ended the turn with {subtype}. {detail}")
            .trim_end()
            .to_owned(),
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn usage_keeps_each_counters_highest_report_and_counts_cache_as_input() {
        let mut counters = Counters::default();
        for usage in [
            json!({"input_tokens": 2, "output_tokens": 24, "cache_read_input_tokens": 1214, "cache_creation_input_tokens": 93}),
            json!({"input_tokens": 2, "output_tokens": 212, "cache_read_input_tokens": 1214, "cache_creation_input_tokens": 93,
                   "cache_creation": {"ephemeral_1h_input_tokens": 93}, "output_tokens_details": {"thinking_tokens": 38}}),
        ] {
            counters.merge(&serde_json::from_value(usage).unwrap());
        }
        assert_eq!(counters.total(), 2 + 1214 + 93 + 212);
        assert_eq!(
            serde_json::to_value(counters.usage()).unwrap(),
            json!({
                "inputTokens": 1309, "outputTokens": 212, "totalTokens": 1521,
                "cacheReadTokens": 1214, "cacheWriteTokens": 93, "cacheWrite1hTokens": 93,
                "reasoningTokens": 38,
            })
        );
    }

    #[test]
    fn a_reached_limit_names_its_window_and_reset() {
        let limit: RateLimit = serde_json::from_value(
            json!({"status": "rejected", "resetsAt": 1794489000, "rateLimitType": "five_hour"}),
        )
        .unwrap();
        assert!(reached(&limit));
        let failure = limit_failure(&limit);
        assert_eq!(failure.code, Code::Quota);
        assert_eq!(
            failure.message,
            "Claude Code subscription limit reached in the 5-hour window; it resets at 2026-11-12T13:10:00Z."
        );
    }

    #[test]
    fn tool_calls_are_saved_under_artifactize_tool_names() {
        let message = assistant(
            &"msg_1".parse().unwrap(),
            &[
                Block::Text {
                    text: "Reading.".into(),
                },
                Block::ToolUse {
                    id: "toolu_1".parse().unwrap(),
                    name: "mcp__artifactize__read".into(),
                    input: json!({"path": "README.md"}),
                },
            ],
        )
        .unwrap();
        let Message::Assistant { id, content } = message else {
            panic!("an assistant message")
        };
        assert_eq!(id.as_deref(), Some("msg_1"));
        let AssistantContent::ToolCall(call) = &content[1] else {
            panic!("a tool call")
        };
        assert_eq!(call.function.name.as_ref(), "read");
        assert_eq!(call.id.wire(), "toolu_1");
    }
}
