//! Typed review results, saved as `{ verdict, kind, issues }`. Every evaluator reports what
//! must change as issues: an Agent with priorities, a person as reasons, a command as its exit
//! code and the end of what it printed. Records saved before 0.27 had no `kind`; they decode
//! once here, for history only.

use std::time::Duration;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value, json};

use crate::{config::Priority, runtime::Verdict};

/// An Agent's issue: how important it is and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentIssue {
    pub priority: Priority,
    pub reason: String,
}

/// A person's issue: what must change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanIssue {
    pub reason: String,
}

/// A command's issue: its nonzero exit code and the end of what it printed. Full output stays
/// in the attempt's logs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeIssue {
    pub exit_code: i32,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeResult {
    pub verdict: Verdict,
    pub issues: Vec<RuntimeIssue>,
    pub duration: Option<Duration>,
}

impl RuntimeResult {
    /// GREEN on exit 0; otherwise RED with one issue whose reason is `reason`.
    pub fn exited(exit_code: i32, reason: impl FnOnce() -> String, duration: Duration) -> Self {
        let (verdict, issues) = if exit_code == 0 {
            (Verdict::Green, Vec::new())
        } else {
            (
                Verdict::Red,
                vec![RuntimeIssue {
                    exit_code,
                    reason: reason(),
                }],
            )
        };
        Self {
            verdict,
            issues,
            duration: Some(duration),
        }
    }

    /// What a remote summary may carry: no captured output, only the engine's own words.
    pub fn summary(&self) -> Self {
        Self {
            verdict: self.verdict,
            issues: self
                .issues
                .iter()
                .map(|issue| RuntimeIssue {
                    exit_code: issue.exit_code,
                    reason: exited_with(issue.exit_code),
                })
                .collect(),
            duration: self.duration,
        }
    }
}

/// The reason a command gives when it printed nothing.
pub fn exited_with(exit_code: i32) -> String {
    format!("Process exited with code {exit_code}.")
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExecutionResult {
    Agent {
        verdict: Verdict,
        issues: Vec<AgentIssue>,
    },
    Human {
        verdict: Verdict,
        issues: Vec<HumanIssue>,
    },
    Runtime(RuntimeResult),
}

/// One issue whichever evaluator reported it, for listings and summaries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub reason: String,
}

impl ExecutionResult {
    pub fn verdict(&self) -> Verdict {
        match self {
            Self::Agent { verdict, .. } | Self::Human { verdict, .. } => *verdict,
            Self::Runtime(result) => result.verdict,
        }
    }

    pub fn kind(&self) -> crate::config::ProfileKind {
        match self {
            Self::Agent { .. } => crate::config::ProfileKind::Agent,
            Self::Human { .. } => crate::config::ProfileKind::Human,
            Self::Runtime(_) => crate::config::ProfileKind::Runtime,
        }
    }

    /// Every issue, most important first for an Agent, as reported otherwise.
    pub fn issues(&self) -> Vec<Issue> {
        match self {
            Self::Agent { issues, .. } => {
                let mut listed: Vec<_> = issues
                    .iter()
                    .map(|issue| Issue {
                        priority: Some(issue.priority),
                        exit_code: None,
                        reason: issue.reason.clone(),
                    })
                    .collect();
                listed.sort_by_key(|issue| issue.priority);
                listed
            }
            Self::Human { issues, .. } => issues
                .iter()
                .map(|issue| Issue {
                    priority: None,
                    exit_code: None,
                    reason: issue.reason.clone(),
                })
                .collect(),
            Self::Runtime(result) => result
                .issues
                .iter()
                .map(|issue| Issue {
                    priority: None,
                    exit_code: Some(issue.exit_code),
                    reason: issue.reason.clone(),
                })
                .collect(),
        }
    }

    /// A runtime result's exit code: its issue's, or 0 for GREEN.
    pub fn exit_code(&self) -> Option<i32> {
        match self {
            Self::Runtime(result) => Some(result.issues.first().map_or(0, |issue| issue.exit_code)),
            Self::Agent { .. } | Self::Human { .. } => None,
        }
    }

    /// How many issues there are and the most important one's reason, in one line: `2 issues:
    /// the title is wrong`. None without issues.
    pub fn summary(&self) -> Option<String> {
        let issues = self.issues();
        let first = issues.first()?;
        let count = match issues.len() {
            1 => "1 issue".to_owned(),
            count => format!("{count} issues"),
        };
        let lead = match (first.priority, first.exit_code) {
            (Some(priority), _) => format!("{priority} "),
            (None, Some(code)) => format!("exit {code}, "),
            (None, None) => String::new(),
        };
        let reason = first
            .reason
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("");
        Some(format!("{count}: {lead}{}", reason.trim()))
    }

    /// JSON is materialized only at presentation and serialization boundaries.
    pub fn to_json(&self) -> Value {
        let verdict = serde_json::to_value(self.verdict()).expect("verdict is JSON");
        match self {
            Self::Agent { issues, .. } => {
                json!({"verdict": verdict, "kind": "agent", "issues": issues})
            }
            Self::Human { issues, .. } => {
                json!({"verdict": verdict, "kind": "human", "issues": issues})
            }
            Self::Runtime(result) => {
                let mut value =
                    json!({"verdict": verdict, "kind": "runtime", "issues": result.issues});
                if let Some(duration) = result.duration {
                    value["durationMs"] = json!(
                        u64::try_from(duration.as_millis())
                            .expect("a saved runtime duration fits u64 milliseconds")
                    );
                }
                value
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
enum Tagged {
    Agent {
        verdict: Verdict,
        issues: Vec<AgentIssue>,
    },
    Human {
        verdict: Verdict,
        issues: Vec<HumanIssue>,
    },
    Runtime {
        verdict: Verdict,
        issues: Vec<RuntimeIssue>,
        #[serde(rename = "durationMs", default)]
        duration_ms: Option<u64>,
    },
}

impl TryFrom<Value> for ExecutionResult {
    type Error = serde_json::Error;
    fn try_from(value: Value) -> Result<Self, Self::Error> {
        if value.get("kind").is_some() {
            return Ok(match serde_json::from_value(value)? {
                Tagged::Agent { verdict, issues } => Self::Agent { verdict, issues },
                Tagged::Human { verdict, issues } => Self::Human { verdict, issues },
                Tagged::Runtime {
                    verdict,
                    issues,
                    duration_ms,
                } => Self::Runtime(RuntimeResult {
                    verdict,
                    issues,
                    duration: duration_ms.map(Duration::from_millis),
                }),
            });
        }
        legacy(value)
    }
}

/// Bound a reason drawn from an old record's output, as a new capture bounds its tail.
const LEGACY_REASON_CHARS: usize = 2000;

/// A record saved before results named their kind: Agent findings, runtime output fields, or
/// a person's owner fields, read as issues for history.
fn legacy(value: Value) -> Result<ExecutionResult, serde_json::Error> {
    use serde::de::Error;
    let mut fields: Map<String, Value> = serde_json::from_value(value)?;
    let verdict: Verdict = serde_json::from_value(fields.remove("verdict").unwrap_or(Value::Null))?;
    if let Some(findings) = fields.remove("findings") {
        let issues: Vec<AgentIssue> = serde_json::from_value(findings)?;
        return Ok(ExecutionResult::Agent { verdict, issues });
    }
    if ["exitCode", "durationMs", "stdout", "stderr"]
        .iter()
        .any(|key| fields.contains_key(*key))
    {
        let exit_code: Option<i32> =
            serde_json::from_value(fields.remove("exitCode").unwrap_or(Value::Null))?;
        let duration: Option<u64> =
            serde_json::from_value(fields.remove("durationMs").unwrap_or(Value::Null))?;
        let text = |key: &str| {
            fields
                .get(key)
                .and_then(Value::as_str)
                .filter(|text| !text.trim().is_empty())
                .map(str::to_owned)
        };
        let issues = match (verdict, exit_code) {
            (Verdict::Red, Some(code)) => vec![RuntimeIssue {
                exit_code: code,
                reason: text("stderr").or_else(|| text("stdout")).map_or_else(
                    || exited_with(code),
                    |output| tail(&output, LEGACY_REASON_CHARS),
                ),
            }],
            (Verdict::Red, None) => {
                return Err(serde_json::Error::custom(
                    "a RED runtime result without an exit code",
                ));
            }
            _ => Vec::new(),
        };
        return Ok(ExecutionResult::Runtime(RuntimeResult {
            verdict,
            issues,
            duration: duration.map(Duration::from_millis),
        }));
    }
    let issues = if verdict == Verdict::Red {
        let mut reasons: Vec<String> = fields
            .values()
            .flat_map(|value| match value {
                Value::String(text) => vec![text.clone()],
                Value::Array(items) => items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_owned))
                    .collect(),
                _ => Vec::new(),
            })
            .filter(|reason| !reason.trim().is_empty())
            .collect();
        if reasons.is_empty() {
            reasons.push("RED, with no reason saved.".into());
        }
        reasons
            .into_iter()
            .map(|reason| HumanIssue { reason })
            .collect()
    } else {
        Vec::new()
    };
    Ok(ExecutionResult::Human { verdict, issues })
}

/// The end of `text`, at most `chars` characters, starting at a line where it can.
pub fn tail(text: &str, chars: usize) -> String {
    let text = text.trim_end();
    let count = text.chars().count();
    if count <= chars {
        return text.to_owned();
    }
    let start = text
        .char_indices()
        .nth(count - chars)
        .map_or(0, |(index, _)| index);
    let cut = &text[start..];
    cut.find('\n')
        .map_or(cut, |line| &cut[line + 1..])
        .to_owned()
}

impl Serialize for ExecutionResult {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_json().serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for ExecutionResult {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(Value::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tagged_results_round_trip_and_reject_unknown_fields() {
        for value in [
            json!({"verdict":"RED","kind":"agent","issues":[{"priority":"P1","reason":"r"}]}),
            json!({"verdict":"GREEN","kind":"human","issues":[]}),
            json!({"verdict":"RED","kind":"human","issues":[{"reason":"fix the title"}]}),
            json!({"verdict":"RED","kind":"runtime","issues":[{"exitCode":2,"reason":"boom"}],"durationMs":12}),
        ] {
            let result = ExecutionResult::try_from(value.clone()).unwrap();
            assert_eq!(result.to_json(), value);
        }
        for invalid in [
            json!({"verdict":"GREEN","kind":"human","issues":[],"approved":true}),
            json!({"verdict":"GREEN","kind":"robot","issues":[]}),
            json!({"verdict":"RED","kind":"runtime","issues":[{"exitCode":"2","reason":"x"}]}),
        ] {
            assert!(ExecutionResult::try_from(invalid).is_err());
        }
    }

    #[test]
    fn records_from_before_kinds_read_as_issues() {
        let agent = ExecutionResult::try_from(
            json!({"verdict":"RED","findings":[{"priority":"P2","reason":"a"},{"priority":"P0","reason":"b"}]}),
        )
        .unwrap();
        assert_eq!(
            agent
                .issues()
                .iter()
                .map(|issue| issue.reason.as_str())
                .collect::<Vec<_>>(),
            ["b", "a"]
        );
        let runtime = ExecutionResult::try_from(
            json!({"verdict":"RED","exitCode":7,"durationMs":5,"stdout":"out","stderr":"","truncated":false}),
        )
        .unwrap();
        assert_eq!(
            runtime.to_json(),
            json!({"verdict":"RED","kind":"runtime","issues":[{"exitCode":7,"reason":"out"}],"durationMs":5})
        );
        let human =
            ExecutionResult::try_from(json!({"verdict":"RED","change":["one","two"]})).unwrap();
        assert_eq!(human.issues().len(), 2);
        let approved =
            ExecutionResult::try_from(json!({"verdict":"GREEN","approved":true})).unwrap();
        assert_eq!(
            approved,
            ExecutionResult::Human {
                verdict: Verdict::Green,
                issues: Vec::new()
            }
        );
        assert!(ExecutionResult::try_from(json!({"verdict":"RED","stdout":"x"})).is_err());
    }

    #[test]
    fn summaries_keep_exit_codes_and_drop_output() {
        let result =
            RuntimeResult::exited(3, || "secret path /home/x".into(), Duration::from_millis(9));
        assert_eq!(
            ExecutionResult::Runtime(result.summary()).to_json(),
            json!({"verdict":"RED","kind":"runtime","issues":[{"exitCode":3,"reason":"Process exited with code 3."}],"durationMs":9})
        );
        assert_eq!(
            RuntimeResult::exited(0, || unreachable!(), Duration::ZERO).issues,
            Vec::new()
        );
    }

    #[test]
    fn tails_keep_the_last_whole_lines() {
        assert_eq!(tail("one\ntwo\nthree\n", 100), "one\ntwo\nthree");
        assert_eq!(tail("alpha\nbeta\ngamma", 9), "gamma");
    }
}
