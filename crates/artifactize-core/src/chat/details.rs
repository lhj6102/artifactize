//! Bounded display details without changing the tool reply replayed to a model.
use std::borrow::Cow;

use rig_core::message::ToolResultContent;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Keep a tool's disclosure small enough to open even in a long conversation.
const EXCERPT_BYTES: usize = 8 * 1024;
/// Includes the gap marker between the head and tail.
const EXCERPT_LINES: usize = 80;
const GAP: &str = "\n…\n";
/// Keep the activity label short even for a long path or command argument.
const SUMMARY_CHARS: usize = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolKind {
    Read,
    Edit,
    Command,
    Verify,
    Other,
}

impl ToolKind {
    pub(crate) fn for_name(name: &str) -> Self {
        match name {
            "read" | "list" | "glob" | "grep" => Self::Read,
            "write" | "edit" => Self::Edit,
            "run_command" => Self::Command,
            name if crate::verify_tools::chat_tool(name) => Self::Verify,
            _ => Self::Other,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Edit => "edit",
            Self::Command => "command",
            Self::Verify => "verify",
            Self::Other => "other",
        }
    }

    pub(crate) fn from_db(value: &str) -> Self {
        match value {
            "read" => Self::Read,
            "edit" => Self::Edit,
            "command" => Self::Command,
            "verify" => Self::Verify,
            _ => Self::Other,
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Arguments<'a> {
    path: Option<&'a str>,
    pattern: Option<&'a str>,
    old_text: Option<&'a str>,
    new_text: Option<&'a str>,
    program: Option<&'a str>,
    #[serde(borrow)]
    args: Vec<&'a str>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WriteArgs {
    path: String,
    text: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EditArgs {
    path: String,
    old_text: String,
    new_text: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CommandArgs {
    program: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cwd: Option<String>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
struct ReadDisplayArgs {
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pattern: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_line: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_line: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_results: Option<u64>,
}

/// Format only validated, typed tool arguments for public `Block::Tool.arguments`; unparsed or
/// invalid provider payloads are dropped rather than forwarded verbatim (`C-ACP-EDGE-ISOLATION`).
pub(super) fn display_arguments(name: &str, arguments: &Value) -> String {
    let encoded = match name {
        "read" | "list" | "glob" | "grep" => {
            let builtin = match name {
                "read" => artifactize_tools::Builtin::Read,
                "list" => artifactize_tools::Builtin::List,
                "glob" => artifactize_tools::Builtin::Glob,
                "grep" => artifactize_tools::Builtin::Grep,
                _ => unreachable!(),
            };
            if artifactize_tools::builtin::Input::parse(builtin, arguments.clone()).is_err() {
                return String::new();
            }
            let parsed: ReadDisplayArgs = match serde_json::from_value(arguments.clone()) {
                Ok(parsed) => parsed,
                Err(_) => return String::new(),
            };
            serde_json::to_string(&parsed).unwrap_or_default()
        }
        "write" => {
            let Ok(mut parsed) = serde_json::from_value::<WriteArgs>(arguments.clone()) else {
                return String::new();
            };
            parsed.text = excerpt(parsed.text).0;
            serde_json::to_string(&parsed).unwrap_or_default()
        }
        "edit" => {
            let Ok(mut parsed) = serde_json::from_value::<EditArgs>(arguments.clone()) else {
                return String::new();
            };
            parsed.old_text = excerpt(parsed.old_text).0;
            parsed.new_text = excerpt(parsed.new_text).0;
            serde_json::to_string(&parsed).unwrap_or_default()
        }
        "run_command" => {
            let Ok(parsed) = serde_json::from_value::<CommandArgs>(arguments.clone()) else {
                return String::new();
            };
            serde_json::to_string(&parsed).unwrap_or_default()
        }
        name if crate::verify_tools::chat_tool(name) => {
            let Some(map) = arguments.as_object().cloned() else {
                return String::new();
            };
            let Ok(call) =
                crate::verify_tools::Call::parse(name, map, time::OffsetDateTime::now_utc())
            else {
                return String::new();
            };
            call.display_arguments()
        }
        _ => return String::new(),
    };
    if encoded.len() <= EXCERPT_BYTES {
        encoded
    } else {
        excerpt(encoded).0
    }
}

pub(super) fn read_summary(
    name: &str,
    path: Option<&std::path::Path>,
    pattern: Option<&str>,
) -> String {
    let path = path.map(crate::platform::path_text);
    let argument = match name {
        "list" => path.as_deref().unwrap_or("."),
        "glob" | "grep" => pattern.unwrap_or(""),
        _ => path.as_deref().unwrap_or(""),
    };
    short([name, argument].into_iter().flat_map(str::split_whitespace))
}

pub(super) fn summary(name: &str, arguments: &Value) -> String {
    let args = Arguments::deserialize(arguments).unwrap_or_default();
    let words = match name {
        "read" | "write" => vec![name, args.path.unwrap_or("")],
        "edit" => {
            let target = short(
                [name, args.path.unwrap_or("")]
                    .into_iter()
                    .flat_map(str::split_whitespace),
            );
            return match (args.old_text, args.new_text) {
                (Some(old), Some(new)) => {
                    format!("{target} +{} −{}", new.lines().count(), old.lines().count())
                }
                _ => target,
            };
        }
        "list" => vec![name, args.path.unwrap_or(".")],
        "glob" | "grep" => vec![name, args.pattern.unwrap_or("")],
        "run_command" => {
            let mut words = vec!["ran", args.program.unwrap_or("")];
            words.extend(args.args);
            words
        }
        _ => return short(name.split('_')),
    };
    short(words.into_iter().flat_map(str::split_whitespace))
}

fn short<'a>(words: impl IntoIterator<Item = &'a str>) -> String {
    let mut chars = words.into_iter().flat_map(|word| word.chars().chain([' ']));
    let label: String = chars.by_ref().take(SUMMARY_CHARS).collect();
    if chars.next().is_some() {
        format!("{}…", label.trim_end())
    } else {
        label.trim_end().to_owned()
    }
}

pub(super) struct ResultView {
    pub output: String,
    pub exit_code: Option<i32>,
    pub omitted_lines: usize,
    pub truncated: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CommandOutput<'a> {
    pub exit_code: i32,
    #[serde(borrow)]
    pub stdout: Cow<'a, str>,
    #[serde(borrow)]
    pub stderr: Cow<'a, str>,
    #[serde(default)]
    pub truncated: bool,
}

impl<'a> CommandOutput<'a> {
    fn from_part(part: &'a ToolResultContent) -> Option<Self> {
        match part {
            ToolResultContent::Json { value } => Self::deserialize(value).ok(),
            // The Claude Code MCP transport carries structured output as JSON text.
            ToolResultContent::Text(text) => serde_json::from_str(&text.text).ok(),
            _ => None,
        }
    }
}

fn legacy_exit(text: &str) -> Option<i32> {
    text.lines()
        .next()?
        .strip_prefix("Command did not succeed: Exited(")?
        .strip_suffix(')')?
        .parse()
        .ok()
}

impl ResultView {
    pub(super) fn new<'a>(
        kind: ToolKind,
        content: impl IntoIterator<Item = &'a ToolResultContent>,
    ) -> Self {
        let mut exit_code = None;
        let mut capture_truncated = false;
        let text = content
            .into_iter()
            .map(|part| {
                if kind == ToolKind::Command
                    && let Some(command) = CommandOutput::from_part(part)
                {
                    exit_code = Some(command.exit_code);
                    capture_truncated = command.truncated;
                    return [command.stdout.as_ref(), command.stderr.as_ref()]
                        .into_iter()
                        .filter(|text| !text.is_empty())
                        .collect::<Vec<_>>()
                        .join("\n");
                }
                match part {
                    ToolResultContent::Text(text) => {
                        if kind == ToolKind::Command {
                            exit_code = legacy_exit(&text.text);
                        }
                        text.text.clone()
                    }
                    ToolResultContent::Json { value } => value.to_string(),
                    _ => "[non-text output]".into(),
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let (output, omitted_lines, clipped) = excerpt(text);
        Self {
            output,
            exit_code,
            omitted_lines,
            truncated: clipped || capture_truncated,
        }
    }
}

fn excerpt(text: String) -> (String, usize, bool) {
    let lines = text.lines().count();
    if text.len() <= EXCERPT_BYTES && lines <= EXCERPT_LINES {
        return (text, 0, false);
    }
    let bytes_per_end = (EXCERPT_BYTES - GAP.len()) / 2;
    let lines_per_end = (EXCERPT_LINES - 1) / 2;
    let head_end = text
        .split_inclusive('\n')
        .take(lines_per_end)
        .map(str::len)
        .sum::<usize>();
    let tail_len = text
        .split_inclusive('\n')
        .rev()
        .take(lines_per_end)
        .map(str::len)
        .sum::<usize>();
    let head = &text[..text.floor_char_boundary(head_end.min(bytes_per_end))];
    let tail = &text[text.ceil_char_boundary(text.len() - tail_len.min(bytes_per_end))..];
    let omitted = lines.saturating_sub(head.lines().count() + tail.lines().count());
    let head = head.strip_suffix('\n').unwrap_or(head);
    (format!("{head}{GAP}{tail}"), omitted, true)
}
