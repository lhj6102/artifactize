use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{process::Output, runtime};

use super::image;

const TEXT_LIMIT: usize = 64 * 1024;
const JSON_LIMIT: usize = 512 * 1024;

/// Self-contained tool content for persistence and multimodal Agent turns.
/// Registry results contain validated images, never references to temporary files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum Content {
    Text {
        text: String,
    },
    Json {
        data: Value,
    },
    /// Canonical base64 of at most 4 MiB; mimeType is image/png, image/jpeg or image/webp.
    Image {
        data: String,
        #[serde(rename = "mimeType")]
        mime_type: String,
    },
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireResult {
    content: Vec<WireContent>,
    #[serde(default)]
    is_error: bool,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
enum WireContent {
    Text {
        text: String,
    },
    Json {
        data: Value,
    },
    Image {
        #[serde(rename = "mimeType")]
        mime_type: String,
        data: Option<String>,
        path: Option<String>,
    },
}

pub(super) fn parse(stdout: &[u8], output_dir: &Path) -> Result<ToolResult, ()> {
    let wire: WireResult = serde_json::from_slice(stdout).map_err(|_| ())?;
    if !(1..=32).contains(&wire.content.len())
        || wire.is_error
            && !matches!(wire.content.as_slice(), [WireContent::Text { text }] if !text.trim().is_empty())
    {
        return Err(());
    }
    let mut result = ToolResult {
        content: Vec::new(),
        is_error: wire.is_error,
    };
    let mut size = 0;
    for block in wire.content {
        let block = match block {
            WireContent::Text { text } if text.len() <= TEXT_LIMIT => Content::Text { text },
            WireContent::Json { data }
                if serde_json::to_vec(&data).map_err(|_| ())?.len() <= JSON_LIMIT =>
            {
                Content::Json { data }
            }
            WireContent::Image {
                data,
                path,
                mime_type,
            } => match (data, path) {
                (Some(data), None) => image::from_base64(&data, &mime_type),
                (None, Some(path)) => image::from_output(output_dir, &path, &mime_type),
                _ => return Err(()),
            }
            .map_err(|_| ())?,
            _ => return Err(()),
        };
        size += serde_json::to_vec(&block).map_err(|_| ())?.len();
        if size > 8 * 1024 * 1024 {
            return Err(());
        }
        result.content.push(block);
    }
    if serde_json::to_vec(&result).map_err(|_| ())?.len() > 8 * 1024 * 1024 {
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
