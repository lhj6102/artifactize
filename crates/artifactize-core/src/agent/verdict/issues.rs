//! An Agent eval's one answer: its prioritized issues. The evaluator reports them; artifactize
//! derives the verdict from their priorities and the eval's `red_at`.

use serde::Deserialize;
use serde_json::{Value, json};

use super::{MAX_RESPONSE_BYTES, ValidatedResult, bounded, bounded_result};
use crate::{config::Priority, runtime::Verdict, store::AgentIssue};

/// The final answer's object. Providers' structured output needs an object root, so the list
/// is its one field.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Answer {
    issues: Vec<AgentIssue>,
}

/// RED when an issue is at `red_at` or more important; without `red_at`, when there is any
/// issue. Issues below the bar are kept but leave the eval GREEN.
pub fn verdict(issues: &[AgentIssue], red_at: Option<Priority>) -> Verdict {
    if issues
        .iter()
        .any(|issue| red_at.is_none_or(|bar| issue.priority <= bar))
    {
        Verdict::Red
    } else {
        Verdict::Green
    }
}

/// The result schema sent to the backend, and the eval's bar to judge the answer by.
pub(in crate::agent) struct Issues {
    pub schema: Value,
    red_at: Option<Priority>,
}

/// Print only the bare error in the saved failure. The detail goes to the repair turn.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub(in crate::agent) struct ModelError {
    message: &'static str,
    detail: Option<String>,
}

impl From<&'static str> for ModelError {
    fn from(message: &'static str) -> Self {
        Self {
            message,
            detail: None,
        }
    }
}

impl Issues {
    /// Every keyword here is in the structured-output subset OpenAI, Anthropic and Gemini share:
    /// closed objects, every property required, and a string enum.
    pub fn new(red_at: Option<Priority>) -> Self {
        let priorities: Vec<_> = Priority::ALL.iter().map(|p| p.as_str()).collect();
        Self {
            schema: json!({
                "type":"object",
                "properties":{
                    "issues":{
                        "type":"array",
                        "description":"Every issue the review found; empty when there is none.",
                        "items":{
                            "type":"object",
                            "properties":{
                                "priority":{
                                    "type":"string",
                                    "enum":priorities,
                                    "description":"P0 is the most important, P3 the least.",
                                },
                                "reason":{"type":"string"},
                            },
                            "required":["priority","reason"],
                            "additionalProperties":false,
                        },
                    },
                },
                "required":["issues"],
                "additionalProperties":false,
            }),
            red_at,
        }
    }

    pub fn parse_model(&self, text: &str) -> Result<ValidatedResult, ModelError> {
        if text.len() > MAX_RESPONSE_BYTES {
            return Err("over_size: final JSON exceeds 1 MiB".into());
        }
        if text.trim().is_empty() {
            return Err("empty: return one JSON object".into());
        }
        let value: Value = serde_json::from_str(text).map_err(
            |_| "not_json: return exactly one JSON object, without prose or code fences",
        )?;
        let answer = Answer::deserialize(&value).map_err(|error| ModelError {
            message: "schema_mismatch: result must be an object with an issues list, each issue a priority P0 to P3 and a reason",
            detail: Some(bounded(error.to_string())),
        })?;
        if answer
            .issues
            .iter()
            .any(|issue| issue.reason.trim().is_empty())
        {
            return Err("schema_mismatch: every issue needs a reason".into());
        }
        bounded_result(&value)?;
        Ok(ValidatedResult {
            verdict: verdict(&answer.issues, self.red_at),
            issues: answer.issues,
        })
    }

    /// The error with what failed in the answer, for the repair turn.
    pub fn repair_detail(error: ModelError) -> String {
        match error.detail {
            Some(detail) => format!("{}\n- {detail}", error.message),
            None => error.message.into(),
        }
    }
}

#[cfg(test)]
mod tests;
