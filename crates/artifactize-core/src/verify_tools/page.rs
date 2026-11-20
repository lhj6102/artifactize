//! Lossless bounded pages of CLI JSON, without transport-owned result snapshots.
use crate::{platform, runs, types::Sha256Digest};
use serde::{Serialize, Serializer};
use std::{fmt, num::NonZeroUsize, str::FromStr};
use time::OffsetDateTime;

// Structured MCP content is also mirrored in text for legacy clients. 8 KiB leaves
// room for both representations, escaped quotes and protocol framing under 64 KiB.
const PAGE_BYTES: usize = 8 * 1024;
/// Each page carries one contiguous text chunk so the client can append it at its offset.
const PAGE_ENTRIES: usize = 1;
/// Stateless cursors encode exactly the revision, byte offset and observation clock.
const CURSOR_FIELDS: usize = 3;

#[derive(Debug)]
pub enum CursorError {
    Shape,
    Revision,
    Offset,
    Clock,
    Expired,
}

impl fmt::Display for CursorError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str(match self {
            Self::Shape => "Invalid detail cursor.",
            Self::Revision => "Invalid detail cursor revision.",
            Self::Offset => "Invalid detail cursor offset.",
            Self::Clock => "Invalid detail cursor clock.",
            Self::Expired => "Expired or foreign detail cursor; restart this inspection.",
        })
    }
}

/// Keep elapsed fields stable between pages without storing a result snapshot.
/// Every continuation still hashes freshly read authoritative records.
pub(super) struct Cursor {
    revision: Sha256Digest,
    offset: NonZeroUsize,
    as_of: OffsetDateTime,
}

impl FromStr for Cursor {
    type Err = CursorError;
    fn from_str(value: &str) -> Result<Self, CursorError> {
        let mut fields = value.splitn(CURSOR_FIELDS, ':');
        let revision = fields
            .next()
            .ok_or(CursorError::Revision)?
            .parse()
            .map_err(|_| CursorError::Revision)?;
        let offset = fields
            .next()
            .ok_or(CursorError::Shape)?
            .parse()
            .map_err(|_| CursorError::Offset)?;
        let nanos = fields
            .next()
            .ok_or(CursorError::Shape)?
            .parse()
            .map_err(|_| CursorError::Clock)?;
        let as_of =
            OffsetDateTime::from_unix_timestamp_nanos(nanos).map_err(|_| CursorError::Clock)?;
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

impl Serialize for Cursor {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// Identify a tool and its exact arguments, excluding the separately bound project and cursor.
pub(super) struct QueryKey(Sha256Digest);

impl QueryKey {
    pub(super) fn new(bytes: &[u8]) -> Self {
        Self(Sha256Digest::of(bytes))
    }
}

enum Position {
    Initial(OffsetDateTime),
    Continuation(Cursor),
}

pub(super) struct Query {
    key: QueryKey,
    position: Position,
}

impl Query {
    pub(super) fn new(key: QueryKey, cursor: Option<Cursor>, as_of: OffsetDateTime) -> Self {
        Self {
            key,
            position: cursor.map_or(Position::Initial(as_of), Position::Continuation),
        }
    }

    pub(super) fn as_of(&self) -> OffsetDateTime {
        match &self.position {
            Position::Initial(as_of) => *as_of,
            Position::Continuation(cursor) => cursor.as_of,
        }
    }

    pub(super) fn project(
        self,
        context: &runs::Context,
        text: &str,
    ) -> Result<Page, super::error::Error> {
        let revision_inputs = (
            platform::path_text(context.repo()),
            platform::path_text(context.state()),
            &self.key.0,
            text,
        );
        let revision = Sha256Digest::of(&serde_json::to_vec(&revision_inputs)?);
        let offset = match &self.position {
            Position::Initial(_) => 0,
            Position::Continuation(cursor) => {
                if cursor.revision != revision {
                    return Err(super::error::Error::Cursor(CursorError::Expired));
                }
                let offset = cursor.offset.get();
                if offset >= text.len() || !text.is_char_boundary(offset) {
                    return Err(super::error::Error::Cursor(CursorError::Offset));
                }
                offset
            }
        };
        let end = text.floor_char_boundary(offset.saturating_add(PAGE_BYTES));
        Ok(Page {
            encoding: Encoding::Json,
            entries: [Entry {
                offset,
                text: text[offset..end].to_owned(),
            }],
            next_cursor: (end < text.len()).then(|| Cursor {
                revision,
                offset: NonZeroUsize::new(end).expect("a page with more text advances the cursor"),
                as_of: self.as_of(),
            }),
        })
    }
}

#[derive(Serialize)]
pub struct Page {
    encoding: Encoding,
    entries: [Entry; PAGE_ENTRIES],
    next_cursor: Option<Cursor>,
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum Encoding {
    Json,
}

#[derive(Serialize)]
struct Entry {
    offset: usize,
    text: String,
}
