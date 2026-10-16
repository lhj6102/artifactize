//! Lossless bounded pages of CLI JSON, without transport-owned result snapshots.
use artifactize_core::types::Sha256Digest;
use serde_json::{Value, json};
use std::{fmt, str::FromStr};
use time::OffsetDateTime;

// Structured MCP content is also mirrored in text for legacy clients. 8 KiB leaves
// room for both representations, escaped quotes and protocol framing under 64 KiB.
const PAGE_BYTES: usize = 8 * 1024;

/// Keep elapsed fields stable between pages without storing a result snapshot.
/// Every continuation still hashes freshly read authoritative records.
pub(super) struct Cursor {
    revision: Sha256Digest,
    offset: usize,
    pub as_of: OffsetDateTime,
}
impl FromStr for Cursor {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, String> {
        let mut fields = value.splitn(3, ':');
        let revision = fields
            .next()
            .ok_or("Invalid detail cursor revision.")?
            .parse::<Sha256Digest>()
            .map_err(|_| "Invalid detail cursor revision.")?;
        let offset = fields
            .next()
            .ok_or("Invalid detail cursor.")?
            .parse()
            .map_err(|_| "Invalid detail cursor offset.")?;
        let nanos = fields
            .next()
            .ok_or("Invalid detail cursor.")?
            .parse()
            .map_err(|_| "Invalid detail cursor clock.")?;
        let as_of = OffsetDateTime::from_unix_timestamp_nanos(nanos)
            .map_err(|_| "Invalid detail cursor clock.")?;
        Ok(Self {
            revision,
            offset,
            as_of,
        })
    }
}
impl fmt::Display for Cursor {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            out,
            "{}:{}:{}",
            self.revision,
            self.offset,
            self.as_of.unix_timestamp_nanos()
        )
    }
}

pub(super) fn project(
    context: &artifactize_core::runs::Context,
    name: &str,
    arguments: &Value,
    value: Value,
    cursor: Option<Cursor>,
    as_of: OffsetDateTime,
) -> Result<Value, String> {
    let text = serde_json::to_string(&value).map_err(|e| e.to_string())?;
    let revision = Sha256Digest::of(
        &serde_json::to_vec(&(context.repo(), context.state(), name, arguments, &text))
            .map_err(|e| e.to_string())?,
    );
    let offset = match cursor {
        None => 0,
        Some(cursor) => {
            if cursor.revision != revision {
                return Err("Expired or foreign detail cursor; restart this inspection.".into());
            }
            cursor.offset
        }
    };
    if offset > text.len() || !text.is_char_boundary(offset) {
        return Err("Invalid detail cursor offset.".into());
    }
    let mut end = text.len().min(offset.saturating_add(PAGE_BYTES));
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    Ok(json!({
        "encoding": "json", "entries": [{"offset": offset, "text": &text[offset..end]}],
        "next_cursor": (end < text.len()).then(|| Cursor { revision, offset: end, as_of }.to_string()),
    }))
}
