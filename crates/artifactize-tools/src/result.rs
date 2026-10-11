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

/// A nonempty, bounded sequence of result blocks. Its contents cannot be mutated
/// into an empty or oversized result after construction.
#[derive(Debug, Clone, PartialEq)]
pub struct ContentBlocks<T>(Vec<T>);

impl<T> ContentBlocks<T> {
    pub fn new(content: Vec<T>) -> Result<Self, InvalidResult> {
        if !(1..=MAX_CONTENT_BLOCKS).contains(&content.len()) {
            return Err(InvalidResult(
                "tool result must contain 1..=32 content blocks",
            ));
        }
        Ok(Self(content))
    }

    pub fn as_slice(&self) -> &[T] {
        &self.0
    }

    pub fn into_vec(self) -> Vec<T> {
        self.0
    }
}

/// A single nonblank text block. Keeping the block itself lets callers borrow
/// the same content slice for both success and error results.
#[derive(Debug, Clone, PartialEq)]
pub struct ErrorMessage(Content);

impl ErrorMessage {
    pub fn new(message: impl Into<String>) -> Result<Self, InvalidResult> {
        let text = message.into();
        if text.trim().is_empty() {
            return Err(InvalidResult("tool error message must not be blank"));
        }
        Ok(Self(Content::Text { text }))
    }

    /// Replace a blank diagnostic with "Tool failed." without changing nonblank text.
    pub fn diagnostic(message: impl Into<String>) -> Self {
        Self::new(message)
            .unwrap_or_else(|_| Self::new("Tool failed.").expect("fallback is nonblank"))
    }

    pub fn as_str(&self) -> &str {
        let Content::Text { text } = &self.0 else {
            unreachable!("error messages are constructed only from text")
        };
        text
    }
}

/// A saved or constructed result contradicts the result type's invariants.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct InvalidResult(&'static str);

/// Successful multimodal content or one nonblank textual diagnostic.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolResult {
    Success(ContentBlocks<Content>),
    Error(ErrorMessage),
}

impl ToolResult {
    /// A single successful block is nonempty by construction.
    pub fn success(content: Content) -> Self {
        Self::Success(ContentBlocks(vec![content]))
    }

    pub fn try_success(content: Vec<Content>) -> Result<Self, InvalidResult> {
        ContentBlocks::new(content).map(Self::Success)
    }

    /// Blank messages become "Tool failed." so even a missing diagnostic is a
    /// valid error result. Nonblank messages are preserved byte-for-byte.
    pub fn error(message: impl Into<String>) -> Self {
        Self::Error(ErrorMessage::diagnostic(message))
    }

    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error(_))
    }

    pub fn content(&self) -> &[Content] {
        match self {
            Self::Success(content) => content.as_slice(),
            Self::Error(message) => std::slice::from_ref(&message.0),
        }
    }

    pub fn into_content(self) -> Vec<Content> {
        match self {
            Self::Success(content) => content.into_vec(),
            Self::Error(message) => vec![message.0],
        }
    }

    fn from_wire(content: Vec<Content>, is_error: bool) -> Result<Self, InvalidResult> {
        if !is_error {
            return Self::try_success(content);
        }
        let [Content::Text { text }] = content.as_slice() else {
            return Err(InvalidResult(
                "tool error must contain exactly one text block",
            ));
        };
        ErrorMessage::new(text.clone()).map(Self::Error)
    }
}

impl Serialize for ToolResult {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut result =
            serializer.serialize_struct("ToolResult", if self.is_error() { 2 } else { 1 })?;
        result.serialize_field("content", self.content())?;
        if self.is_error() {
            result.serialize_field("isError", &true)?;
        }
        result.end()
    }
}

impl<'de> Deserialize<'de> for ToolResult {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Saved images are already inline and validated. Do not read files or
        // impose command-output byte limits when replaying an existing result.
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct SavedResult {
            content: Vec<Content>,
            #[serde(default)]
            is_error: bool,
        }
        let saved = SavedResult::deserialize(deserializer)?;
        Self::from_wire(saved.content, saved.is_error).map_err(serde::de::Error::custom)
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
    Text { text: String },
    Json { data: Value },
    Image(WireImage),
}

/// An image block carries its bytes inline or names a file in the output directory, never
/// both and never neither.
#[derive(Deserialize)]
#[serde(untagged)]
enum WireImage {
    Data(InlineImage),
    Path(ImageFile),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InlineImage {
    mime_type: String,
    data: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ImageFile {
    mime_type: String,
    path: String,
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
    let mut content = Vec::with_capacity(wire.content.len());
    let mut size = 0;
    for block in wire.content {
        let block = match block {
            WireContent::Text { text } if text.len() <= TEXT_LIMIT => Content::Text { text },
            WireContent::Json { data }
                if serde_json::to_vec(&data).map_err(|_| InvalidOutput)?.len() <= JSON_LIMIT =>
            {
                Content::Json { data }
            }
            WireContent::Image(WireImage::Data(InlineImage { mime_type, data })) => {
                image::from_base64(&data, &mime_type).map_err(|_| InvalidOutput)?
            }
            WireContent::Image(WireImage::Path(ImageFile { mime_type, path })) => {
                image::from_output(output_dir, &path, &mime_type).map_err(|_| InvalidOutput)?
            }
            _ => return Err(InvalidOutput),
        };
        size += serde_json::to_vec(&block).map_err(|_| InvalidOutput)?.len();
        if size > RESULT_LIMIT {
            return Err(InvalidOutput);
        }
        content.push(block);
    }
    let result = ToolResult::from_wire(content, wire.is_error).map_err(|_| InvalidOutput)?;
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
    if failed {
        ToolResult::error(text)
    } else {
        ToolResult::success(Content::Text { text })
    }
}

/// ECMA-48 control sequence bytes: after `ESC [`, parameter bytes, then intermediate bytes,
/// then one final byte end the sequence.
const CSI_PARAMETER: std::ops::RangeInclusive<u8> = 0x30..=0x3f;
const CSI_INTERMEDIATE: std::ops::RangeInclusive<u8> = 0x20..=0x2f;
const CSI_FINAL: std::ops::RangeInclusive<u8> = 0x40..=0x7e;

/// The two-byte prefix introducing an ECMA-48 control sequence.
const CSI_PREFIX: &[u8] = b"\x1b[";
/// C0 controls except tab, line feed and carriage return are not printable tool output.
const C0_CONTROLS: &[std::ops::RangeInclusive<u8>] = &[0..=8, 11..=12, 14..=31];

pub fn clean_output(bytes: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(bytes);
    let mut bytes = text.as_bytes();
    let mut clean = Vec::with_capacity(bytes.len());
    while let Some((&byte, tail)) = bytes.split_first() {
        if bytes.starts_with(CSI_PREFIX) {
            let mut end = CSI_PREFIX.len();
            while bytes
                .get(end)
                .is_some_and(|byte| CSI_PARAMETER.contains(byte))
            {
                end += 1;
            }
            while bytes
                .get(end)
                .is_some_and(|byte| CSI_INTERMEDIATE.contains(byte))
            {
                end += 1;
            }
            if bytes.get(end).is_some_and(|byte| CSI_FINAL.contains(byte)) {
                bytes = &bytes[end + 1..];
                continue;
            }
        }
        if !C0_CONTROLS.iter().any(|range| range.contains(&byte)) {
            clean.push(byte);
        }
        bytes = tail;
    }
    clean
}
