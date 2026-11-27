use std::sync::LazyLock;

use serde_json::Value;

use crate::store::AgentIssue;

mod issues;
pub(super) use issues::Issues;
pub use issues::verdict;

/// Bound the raw model response before parsing; prose/whitespace can exceed the semantic result.
pub(super) const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
/// Keep the normalized result bounded in saved receipts, in UTF-16 units for JSON clients.
const MAX_RESULT_CHARS: usize = 256_000;
static RESULT_TOO_LARGE: LazyLock<String> =
    LazyLock::new(|| format!("over_size: normalized result exceeds {MAX_RESULT_CHARS} characters"));
/// Bound each diagnostic so a model's text cannot dominate the repair prompt.
const MAX_DIAGNOSTIC_CHARS: usize = 200;

/// Audit and usage live in separate rows; this bounds the persisted semantic result.
fn bounded_result(value: &Value) -> Result<(), &'static str> {
    if serde_json::to_string(value)
        .expect("result is JSON")
        .encode_utf16()
        .count()
        > MAX_RESULT_CHARS
    {
        return Err(&RESULT_TOO_LARGE);
    }
    Ok(())
}

fn bounded(text: String) -> String {
    if text.chars().count() > MAX_DIAGNOSTIC_CHARS {
        text.chars().take(MAX_DIAGNOSTIC_CHARS).collect::<String>() + "… (truncated)"
    } else {
        text
    }
}

/// An Agent eval's checked answer: its issues and the verdict artifactize derived from them
/// and the eval's `red_at`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ValidatedResult {
    pub verdict: crate::runtime::Verdict,
    /// Session files from before 0.27 call them findings.
    #[serde(default, alias = "findings", skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<AgentIssue>,
    /// What session files from before findings answered instead, kept readable as history.
    #[serde(flatten)]
    pub earlier: serde_json::Map<String, Value>,
}

impl ValidatedResult {
    /// The answer as a session file saves it.
    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).expect("an answer is JSON")
    }

    /// The answer's fields as the model wrote them, for transcripts.
    pub fn fields(&self) -> serde_json::Map<String, Value> {
        let mut fields = self.earlier.clone();
        if !self.issues.is_empty() || fields.is_empty() {
            fields.insert(
                "issues".to_owned(),
                serde_json::to_value(&self.issues).expect("issues are JSON"),
            );
        }
        fields
    }
}

#[cfg(test)]
impl ValidatedResult {
    /// The answer as saved before it becomes a typed result: its verdict and issues.
    pub(crate) fn into_json(self) -> Value {
        serde_json::json!({"verdict": self.verdict, "issues": self.issues})
    }
}

impl From<ValidatedResult> for crate::store::ExecutionResult {
    fn from(result: ValidatedResult) -> Self {
        Self::Agent {
            verdict: result.verdict,
            issues: result.issues,
        }
    }
}
