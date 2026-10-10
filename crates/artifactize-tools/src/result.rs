use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::image;

/// Text stays readable in the next provider turn rather than filling its whole context.
const TEXT_LIMIT: usize = 64 * 1024;
/// Bound one dynamic JSON block before it reaches the provider.
const JSON_LIMIT: usize = 512 * 1024;
/// Cap the whole result, including base64 images and JSON envelope overhead.
const RESULT_LIMIT: usize = 8 * 1024 * 1024;
/// Bound multimodal block fan-out and per-block validation work even when each block is small.
const MAX_CONTENT_BLOCKS: usize = 32;

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

/// Malformed or oversized command output; callers expose their own execution diagnostic.
#[derive(Debug, thiserror::Error)]
#[error("Agent tool returned invalid output.")]
pub struct InvalidOutput;

pub fn parse(stdout: &[u8], output_dir: &Path) -> Result<ToolResult, InvalidOutput> {
    let wire: WireResult = serde_json::from_slice(stdout).map_err(|_| InvalidOutput)?;
    if !(1..=MAX_CONTENT_BLOCKS).contains(&wire.content.len())
        || wire.is_error
            && !matches!(
                wire.content.as_slice(),
                [WireContent::Text { text }] if !text.trim().is_empty()
            )
    {
        return Err(InvalidOutput);
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
                if serde_json::to_vec(&data).map_err(|_| InvalidOutput)?.len() <= JSON_LIMIT =>
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
                _ => return Err(InvalidOutput),
            }
            .map_err(|_| InvalidOutput)?,
            _ => return Err(InvalidOutput),
        };
        size += serde_json::to_vec(&block).map_err(|_| InvalidOutput)?.len();
        if size > RESULT_LIMIT {
            return Err(InvalidOutput);
        }
        result.content.push(block);
    }
    if serde_json::to_vec(&result)
        .map_err(|_| InvalidOutput)?
        .len()
        > RESULT_LIMIT
    {
        return Err(InvalidOutput);
    }
    Ok(result)
}

pub fn plain(stdout: &[u8], successful: bool, truncated: bool) -> ToolResult {
    let clean = clean_output(stdout);
    let text = String::from_utf8(clean).expect("clean output is UTF-8");
    let failed = !successful;
    let mut text = if failed {
        format!("Agent tool exited unsuccessfully.\n{text}")
    } else {
        text
    };
    if text.len() > TEXT_LIMIT || truncated {
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

pub fn clean_output(bytes: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(bytes);
    let mut bytes = text.as_bytes();
    let mut clean = Vec::with_capacity(bytes.len());
    while let Some((&byte, tail)) = bytes.split_first() {
        if bytes.starts_with(b"\x1b[") {
            let mut end = 2;
            while bytes
                .get(end)
                .is_some_and(|byte| (0x30..=0x3f).contains(byte))
            {
                end += 1;
            }
            while bytes
                .get(end)
                .is_some_and(|byte| (0x20..=0x2f).contains(byte))
            {
                end += 1;
            }
            if bytes
                .get(end)
                .is_some_and(|byte| (0x40..=0x7e).contains(byte))
            {
                bytes = &bytes[end + 1..];
                continue;
            }
        }
        if !matches!(byte, 0..=8 | 11..=12 | 14..=31) {
            clean.push(byte);
        }
        bytes = tail;
    }
    clean
}
