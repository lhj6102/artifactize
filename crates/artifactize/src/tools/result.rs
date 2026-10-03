use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{process::Output, runtime};

const TEXT_LIMIT: usize = 64 * 1024;
const JSON_LIMIT: usize = 512 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum Content {
    Text { text: String },
    Json { data: Value },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolResult {
    pub content: Vec<Content>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_error: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}

impl ToolResult {
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            content: vec![Content::Text {
                text: message.into(),
            }],
            is_error: true,
        }
    }
}

pub(super) fn parse(stdout: &[u8]) -> Result<ToolResult, ()> {
    let result: ToolResult = serde_json::from_slice(stdout).map_err(|_| ())?;
    if !(1..=32).contains(&result.content.len()) {
        return Err(());
    }
    for block in &result.content {
        let valid = match block {
            Content::Text { text } => text.len() <= TEXT_LIMIT,
            Content::Json { data } => serde_json::to_vec(data).map_err(|_| ())?.len() <= JSON_LIMIT,
        };
        if !valid {
            return Err(());
        }
    }
    if serde_json::to_vec(&result).map_err(|_| ())?.len() > 8 * 1024 * 1024 {
        return Err(());
    }
    if result.is_error
        && !matches!(result.content.as_slice(), [Content::Text { text }] if !text.trim().is_empty())
    {
        return Err(());
    }
    Ok(result)
}

pub(super) fn plain(output: &Output) -> ToolResult {
    let clean = runtime::clean_output(&output.stdout);
    let text = String::from_utf8(clean).expect("clean output is UTF-8");
    let failed = !output.status.success();
    let mut text = if failed {
        format!("Agent tool exited unsuccessfully.\n{text}")
    } else {
        text
    };
    if text.len() > TEXT_LIMIT || output.truncated {
        let suffix = "\n[output truncated]";
        let mut end = (TEXT_LIMIT - suffix.len()).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str(suffix);
    }
    ToolResult {
        content: vec![Content::Text { text }],
        is_error: failed,
    }
}
