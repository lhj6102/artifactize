//! Bounded display details without changing the tool reply replayed to a model.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolKind {
    Read,
    Edit,
    Command,
    Verify,
    Other,
}

impl ToolKind {
    pub(super) fn for_name(name: &str) -> Self {
        match name {
            "read" | "list" | "glob" | "grep" => Self::Read,
            "write" | "edit" => Self::Edit,
            "run_command" => Self::Command,
            name if crate::verify_tools::chat_tool(name) => Self::Verify,
            _ => Self::Other,
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Arguments<'a> {
    path: Option<&'a str>,
    pattern: Option<&'a str>,
    program: Option<&'a str>,
    #[serde(borrow)]
    args: Vec<&'a str>,
}

pub(super) fn summary(name: &str, arguments: &Value) -> String {
    let args = Arguments::deserialize(arguments).unwrap_or_default();
    let words = match name {
        "read" | "write" | "edit" => vec![name, args.path.unwrap_or("")],
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommandOutput<'a> {
    exit_code: i32,
    stdout: &'a str,
    stderr: &'a str,
    #[serde(default)]
    truncated: bool,
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
            .map(|part| match part {
                ToolResultContent::Text(text) => {
                    if kind == ToolKind::Command {
                        // Nonzero exits have always been saved as text diagnostics. Parse only
                        // the generated first line, never stdout or another tool's output.
                        exit_code = text
                            .text
                            .lines()
                            .next()
                            .and_then(|line| line.strip_prefix("Command did not succeed: Exited("))
                            .and_then(|code| code.strip_suffix(')'))
                            .and_then(|code| code.parse().ok());
                    }
                    text.text.clone()
                }
                ToolResultContent::Json { value } => {
                    if kind == ToolKind::Command
                        && let Ok(output) = CommandOutput::deserialize(value)
                    {
                        exit_code = Some(output.exit_code);
                        capture_truncated = output.truncated;
                        return [output.stdout, output.stderr]
                            .into_iter()
                            .filter(|s| !s.is_empty())
                            .collect::<Vec<_>>()
                            .join("\n");
                    }
                    value.to_string()
                }
                _ => "[non-text output]".into(),
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
