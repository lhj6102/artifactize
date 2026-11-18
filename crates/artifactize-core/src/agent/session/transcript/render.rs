//! Render prose, activities and verdicts without exposing private tool or provider payloads.

use rig_core::message::{ToolResult, ToolResultContent};
use std::path::Path;

/// Activity labels must stay small even when a custom tool or error has very large text.
const HEADER_CHARS: usize = 180;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Running,
    Succeeded,
    Failed(String),
    CompletedUnknown,
    Unfinished,
}
impl State {
    fn label(&self) -> &str {
        match self {
            Self::Running => "running",
            Self::Succeeded => "done",
            Self::Failed(_) => "failed",
            Self::CompletedUnknown => "completed (status not recorded)",
            Self::Unfinished => "not completed",
        }
    }
}
#[derive(Debug, Clone)]
pub struct Activity {
    pub name: String,
    pub target: String,
    pub state: State,
    pub orphan: bool,
}
#[derive(Debug, Default)]
pub struct Group {
    pub activities: Vec<Activity>,
}
impl Group {
    pub fn text(&self, expanded: bool) -> String {
        let active = self
            .activities
            .iter()
            .find(|activity| activity.state == State::Running);
        let failed = self
            .activities
            .iter()
            .filter_map(|activity| match &activity.state {
                State::Failed(reason) => Some((activity, reason)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let mut text = match active {
            Some(activity) => format!(
                "{} {} {} — running{}",
                if expanded { "▾" } else { "▸" },
                activity_label(&activity.name),
                activity.target,
                if self.activities.len() > 1 {
                    format!(" · {} activities", self.activities.len())
                } else {
                    String::new()
                }
            ),
            None => {
                let label = self
                    .activities
                    .first()
                    .map(|activity| activity_label(&activity.name))
                    .unwrap_or("Tools");
                let target = self
                    .activities
                    .first()
                    .map(|activity| activity.target.as_str())
                    .unwrap_or_default();
                format!(
                    "{} {label} {target} — {}{}",
                    if expanded { "▾" } else { "▸" },
                    if failed.is_empty() {
                        "completed"
                    } else {
                        "failed"
                    },
                    if self.activities.len() > 1 {
                        format!(" · {} activities", self.activities.len())
                    } else {
                        String::new()
                    }
                )
            }
        };
        // Failures remain visible even when collapsed. Only recorded failure flags qualify.
        for (activity, reason) in failed {
            text.push_str(&format!(
                "{}{} {} — failed: {}",
                if expanded { "\n  " } else { " · " },
                activity.name,
                activity.target,
                reason
            ));
        }
        if expanded {
            for activity in &self.activities {
                if matches!(activity.state, State::Failed(_)) {
                    continue;
                }
                text.push_str(&format!(
                    "\n  {} {} — {}{}",
                    activity.name,
                    activity.target,
                    activity.state.label(),
                    if activity.orphan {
                        " (unmatched recorded result)"
                    } else {
                        ""
                    }
                ));
            }
        } else if self.activities.iter().any(|activity| {
            activity.orphan || matches!(activity.state, State::CompletedUnknown | State::Unfinished)
        }) {
            text.push_str(" · incomplete/status unreported");
        }
        text.push_str("\n\n");
        text
    }
}

pub(super) fn header(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .take(HEADER_CHARS)
        .collect()
}
fn activity_label(name: &str) -> &str {
    if name == "read" || name.starts_with("read_") {
        "Read"
    } else if name == "grep"
        || name == "glob"
        || name.starts_with("grep_")
        || name.starts_with("glob_")
    {
        "Search"
    } else if name == "list" || name.starts_with("list_") {
        "List"
    } else if name == "view_image" || name.starts_with("view_image_") {
        "View image"
    } else {
        name
    }
}
pub(super) enum Target<'a> {
    Builtin {
        path: Option<&'a Path>,
        pattern: Option<&'a str>,
    },
    Unrecorded,
}

pub(super) fn target(target: Target<'_>) -> String {
    let Target::Builtin { path, pattern } = target else {
        return "target not recorded".into();
    };
    let path = path
        .map(|path| header(&crate::platform::path_text(path)))
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| ".".into());
    match pattern {
        Some(pattern) => format!("{path} · {}", header(pattern)),
        None => path,
    }
}
pub(super) fn failure_reason(result: &ToolResult) -> String {
    result
        .content
        .iter()
        .find_map(|part| match part {
            ToolResultContent::Text(text) => Some(header(&text.text)),
            _ => None,
        })
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "failure reason not recorded".into())
}

/// Owner result fields are meaningful data, not provider transport. Render them recursively
/// as labels and text instead of JSON syntax; unknown owner fields remain visible too.
pub(super) fn result_text(result: &crate::agent::verdict::ValidatedResult) -> String {
    fn field(label: &str, value: &serde_json::Value, indent: usize, lines: &mut Vec<String>) {
        let prefix = "  ".repeat(indent);
        match value {
            serde_json::Value::Object(fields) => {
                if !label.is_empty() {
                    lines.push(format!("{prefix}{label}"));
                }
                for (name, value) in fields {
                    field(name, value, indent + usize::from(!label.is_empty()), lines);
                }
            }
            serde_json::Value::Array(items) => {
                if !label.is_empty() {
                    lines.push(format!("{prefix}{label}"));
                }
                for value in items {
                    field("-", value, indent + 1, lines);
                }
            }
            serde_json::Value::String(text) => lines.push(format!(
                "{prefix}{}{}",
                if label.is_empty() {
                    String::new()
                } else {
                    format!("{label}: ")
                },
                markdown(text)
            )),
            value => lines.push(format!("{prefix}{label}: {value}")),
        }
    }
    let mut lines = vec![format!("Verdict: {}", header(result.verdict.as_str()))];
    for (name, value) in &result.fields {
        field(name, value, 0, &mut lines);
    }
    lines.join("\n")
}

/// Lightweight terminal Markdown: keep literal paragraphs/lists and code indentation, remove
/// heading/fence syntax only. No HTML, links, images or private payloads are interpreted.
pub fn markdown(text: &str) -> String {
    let mut code = false;
    text.lines()
        .map(|line| {
            if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
                code = !code;
                return String::new();
            }
            if code {
                return format!("    {line}");
            }
            let heading = line.trim_start_matches('#');
            if heading.len() != line.len() && heading.starts_with(' ') {
                heading.trim_start().to_owned()
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
