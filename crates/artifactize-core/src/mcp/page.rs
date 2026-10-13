//! Lossless bounded pages of CLI JSON, without transport-owned result snapshots.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

// Structured MCP content is also mirrored in text for legacy clients. 8 KiB leaves
// room for both representations, escaped quotes and protocol framing under 64 KiB.
const PAGE_BYTES: usize = 8 * 1024;

pub(super) fn project(
    name: &str,
    arguments: &Value,
    value: Value,
    cursor: Option<&str>,
) -> Result<Value, String> {
    let text = serde_json::to_string(&value).map_err(|e| e.to_string())?;
    let revision =
        Sha256::digest(serde_json::to_vec(&(name, arguments, &text)).map_err(|e| e.to_string())?)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
    let offset = match cursor {
        None => 0,
        Some(cursor) => {
            let (hash, offset) = cursor.split_once(':').ok_or("Invalid detail cursor.")?;
            if hash != revision {
                return Err("Expired or foreign detail cursor; restart this inspection.".into());
            }
            offset
                .parse::<usize>()
                .map_err(|_| "Invalid detail cursor offset.")?
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
        "next_cursor": (end < text.len()).then(|| format!("{revision}:{end}")),
    }))
}
