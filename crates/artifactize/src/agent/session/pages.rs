//! Ephemeral formatted text and row offsets. Large events are decoded once, never retained.
//! Layout scans bounded UTF-8 chunks; rows/page-in use this same explicit grapheme model.
use super::document::{Anchor, Position};
use ratatui::buffer::CellWidth;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    ops::Range,
};
use unicode_segmentation::UnicodeSegmentation;

/// One layout/page-in job scans at most a quarter MiB of formatted text.
const CHUNK: usize = 256 * 1024;
/// Bound each layout job even for empty events, which spend no byte budget, so input stays
/// responsive.
const LAYOUT_EVENTS: usize = 128;
/// Each disk row stores three u64s: source byte offset, rendered byte offset and rendered length.
/// Keep these offsets on disk instead of retaining a heap Range for every terminal row.
const ENTRY: u64 = 24;

fn temporary() -> Result<File, String> {
    let file = tempfile::tempfile().map_err(|error| error.to_string())?;
    crate::platform::restrict_file(&file).map_err(|error| error.to_string())?;
    Ok(file)
}

struct Text {
    offset: u64,
    bytes: usize,
}
#[derive(Clone, Copy)]
struct Rows {
    first: usize,
    count: usize,
    rendered_end: u64,
}
fn rendered_row(file: &mut File, rendered: &mut File, row: usize) -> Result<String, String> {
    file.seek(SeekFrom::Start(row as u64 * ENTRY + 8))
        .map_err(|error| error.to_string())?;
    let mut entry = [0; 16];
    file.read_exact(&mut entry)
        .map_err(|error| error.to_string())?;
    let offset = u64::from_le_bytes(entry[..8].try_into().expect("rendered offset"));
    let size = u64::from_le_bytes(entry[8..].try_into().expect("rendered length")) as usize;
    rendered
        .seek(SeekFrom::Start(offset))
        .map_err(|error| error.to_string())?;
    let mut bytes = vec![0; size];
    rendered
        .read_exact(&mut bytes)
        .map_err(|error| error.to_string())?;
    String::from_utf8(bytes).map_err(|error| error.to_string())
}
struct Segment {
    first: usize,
    end: usize,
    file: File,
    rendered: File,
}
struct Index {
    generation: u64,
    width: usize,
    file: File,
    rows: Vec<Rows>,
    visible: Vec<usize>,
    total: usize,
    event: usize,
    cursor: usize,
    start: usize,
    columns: usize,
    rendered: File,
    output: String,
    output_offset: u64,
    prefix: Vec<Segment>,
}
impl Index {
    fn new(width: usize, generation: u64) -> Result<Self, String> {
        Ok(Self {
            generation,
            width,
            file: temporary()?,
            rows: Vec::new(),
            visible: Vec::new(),
            total: 0,
            event: 0,
            cursor: 0,
            start: 0,
            columns: 0,
            rendered: temporary()?,
            output: String::new(),
            output_offset: 0,
            prefix: Vec::new(),
        })
    }
    fn append(&mut self, range: Range<usize>) -> Result<(), String> {
        self.file
            .seek(SeekFrom::Start(self.total as u64 * ENTRY))
            .map_err(|error| error.to_string())?;
        self.rendered
            .seek(SeekFrom::Start(self.output_offset))
            .map_err(|error| error.to_string())?;
        self.rendered
            .write_all(self.output.as_bytes())
            .map_err(|error| error.to_string())?;
        self.file
            .write_all(&(range.start as u64).to_le_bytes())
            .and_then(|()| self.file.write_all(&self.output_offset.to_le_bytes()))
            .and_then(|()| {
                self.file
                    .write_all(&(self.output.len() as u64).to_le_bytes())
            })
            .map_err(|error| error.to_string())?;
        self.output_offset += self.output.len() as u64;
        self.output.clear();
        self.total += 1;
        self.rows[self.event].count += 1;
        self.rows[self.event].rendered_end = self.output_offset;
        Ok(())
    }
    fn range(&mut self, row: usize) -> Result<Range<usize>, String> {
        if let Some(segment) = self
            .prefix
            .iter_mut()
            .find(|segment| row >= segment.first && row < segment.end)
        {
            segment
                .file
                .seek(SeekFrom::Start(row as u64 * ENTRY))
                .map_err(|error| error.to_string())?;
            let mut bytes = [0; 8];
            segment
                .file
                .read_exact(&mut bytes)
                .map_err(|error| error.to_string())?;
            let start = u64::from_le_bytes(bytes) as usize;
            return Ok(start..start);
        }
        self.file
            .seek(SeekFrom::Start(row as u64 * ENTRY))
            .map_err(|error| error.to_string())?;
        let mut bytes = [0; ENTRY as usize];
        self.file
            .read_exact(&mut bytes)
            .map_err(|error| error.to_string())?;
        Ok(
            u64::from_le_bytes(bytes[..8].try_into().expect("first offset")) as usize
                ..u64::from_le_bytes(bytes[..8].try_into().expect("first offset")) as usize,
        )
    }
    fn rendered(&mut self, row: usize) -> Result<String, String> {
        if let Some(segment) = self
            .prefix
            .iter_mut()
            .find(|segment| row >= segment.first && row < segment.end)
        {
            return rendered_row(&mut segment.file, &mut segment.rendered, row);
        }
        self.file
            .seek(SeekFrom::Start(row as u64 * ENTRY + 8))
            .map_err(|error| error.to_string())?;
        let mut entry = [0; 16];
        self.file
            .read_exact(&mut entry)
            .map_err(|error| error.to_string())?;
        let offset = u64::from_le_bytes(entry[..8].try_into().expect("rendered offset"));
        let size = u64::from_le_bytes(entry[8..].try_into().expect("rendered length")) as usize;
        self.rendered
            .seek(SeekFrom::Start(offset))
            .map_err(|error| error.to_string())?;
        let mut bytes = vec![0; size];
        self.rendered
            .read_exact(&mut bytes)
            .map_err(|error| error.to_string())?;
        String::from_utf8(bytes).map_err(|error| error.to_string())
    }
    fn anchor(&mut self, row: usize) -> Result<Anchor, String> {
        let visible = self
            .visible
            .partition_point(|event| self.rows[*event].first + self.rows[*event].count <= row);
        match self.visible.get(visible) {
            Some(event) => Ok(Anchor {
                event: *event,
                byte: self.range(row)?.start,
            }),
            None => Ok(Anchor::default()),
        }
    }
    fn row(&mut self, anchor: Anchor) -> Result<usize, String> {
        let Some(rows) = self.rows.get(anchor.event).copied() else {
            return Ok(0);
        };
        let (mut low, mut high) = (0, rows.count);
        while low < high {
            let middle = low + (high - low) / 2;
            if self.range(rows.first + middle)?.start <= anchor.byte {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        Ok(rows.first + low.saturating_sub(1))
    }
}

pub(super) struct Pages {
    text: File,
    texts: Vec<Text>,
    length: u64,
    current: Index,
    previous: Option<Index>,
    committed: u64,
    generation: u64,
    pub bytes_read: u64,
}
impl Pages {
    pub fn new(width: usize) -> Result<Self, String> {
        Ok(Self {
            text: temporary()?,
            texts: Vec::new(),
            length: 0,
            current: Index::new(width, 0)?,
            previous: None,
            committed: 0,
            generation: 0,
            bytes_read: 0,
        })
    }
    #[cfg(test)]
    pub fn append(&mut self, text: &str) -> Result<(), String> {
        self.set(self.texts.len(), text)
    }
    /// Stable semantic block slots are replaced by appending a new private text extent.
    /// Only layout is rebuilt; original JSONL events are never decoded again.
    pub fn set(&mut self, id: usize, text: &str) -> Result<(), String> {
        assert!(
            id <= self.texts.len(),
            "semantic block slots are consecutive"
        );
        let replacing = id < self.texts.len();
        if replacing {
            self.relayout_from(id)?;
        }
        self.text
            .seek(SeekFrom::Start(self.length))
            .map_err(|error| error.to_string())?;
        self.text
            .write_all(text.as_bytes())
            .map_err(|error| error.to_string())?;
        let extent = Text {
            offset: self.length,
            bytes: text.len(),
        };
        if replacing {
            self.texts[id] = extent;
        } else {
            self.texts.push(extent);
        }
        self.length += text.len() as u64;
        Ok(())
    }
    fn bytes(&mut self, event: usize, range: Range<usize>) -> Result<Vec<u8>, String> {
        let text = &self.texts[event];
        self.text
            .seek(SeekFrom::Start(text.offset + range.start as u64))
            .map_err(|error| error.to_string())?;
        let mut bytes = vec![0; range.len()];
        self.text
            .read_exact(&mut bytes)
            .map_err(|error| error.to_string())?;
        self.bytes_read += bytes.len() as u64;
        Ok(bytes)
    }
    pub fn commit(&mut self, width: usize) {
        assert_eq!(
            self.current.width, width,
            "only the current rendered layout is committed"
        );
        self.committed = self.current.generation;
        self.previous = None;
    }
    /// Updating a growing tail reuses indexed history. Keep only the displayed generation
    /// and one in-progress generation, restarting at the changed stable block, not row zero.
    fn relayout_from(&mut self, id: usize) -> Result<(), String> {
        if id >= self.current.event && self.current.cursor == 0 {
            return Ok(());
        }
        if self.current.generation == self.committed {
            self.relayout_width(self.current.width)?;
            let previous = self.previous.as_ref().expect("displayed generation pinned");
            let count = id.min(previous.event);
            let total = previous
                .rows
                .get(count)
                .map_or(previous.total, |row| row.first);
            let mut prefix = Vec::new();
            for segment in &previous.prefix {
                let end = segment.end.min(total);
                if segment.first < end {
                    prefix.push(Segment {
                        first: segment.first,
                        end,
                        file: segment
                            .file
                            .try_clone()
                            .map_err(|error| error.to_string())?,
                        rendered: segment
                            .rendered
                            .try_clone()
                            .map_err(|error| error.to_string())?,
                    });
                }
            }
            let first = prefix.last().map_or(0, |segment| segment.end);
            if first < total {
                prefix.push(Segment {
                    first,
                    end: total,
                    file: previous
                        .file
                        .try_clone()
                        .map_err(|error| error.to_string())?,
                    rendered: previous
                        .rendered
                        .try_clone()
                        .map_err(|error| error.to_string())?,
                });
            }
            self.current.prefix = prefix;
            self.current.rows = previous.rows[..count].to_vec();
            self.current.visible = previous
                .visible
                .iter()
                .copied()
                .take_while(|event| *event < count)
                .collect();
            self.current.total = total;
            self.current.event = count;
            self.current.output_offset = 0;
        } else {
            let count = id.min(self.current.event);
            let total = self
                .current
                .rows
                .get(count)
                .map_or(self.current.total, |row| row.first);
            let rendered = count
                .checked_sub(1)
                .map_or(0, |index| self.current.rows[index].rendered_end);
            self.current.prefix.retain(|segment| segment.first < total);
            for segment in &mut self.current.prefix {
                segment.end = segment.end.min(total);
            }
            self.current.rows.truncate(count);
            self.current.visible.retain(|event| *event < count);
            self.current.total = total;
            self.current.event = count;
            self.current.cursor = 0;
            self.current.start = 0;
            self.current.columns = 0;
            self.current.output.clear();
            self.current.output_offset = rendered;
        }
        Ok(())
    }
    fn relayout_width(&mut self, width: usize) -> Result<(), String> {
        // Multiple semantic updates within one unfinished rebuild need no new generation.
        if self.current.event == 0
            && self.current.cursor == 0
            && self.current.total == 0
            && self.current.width == width
        {
            return Ok(());
        }
        self.generation += 1;
        let next = Index::new(width, self.generation)?;
        let old = std::mem::replace(&mut self.current, next);
        if old.generation == self.committed {
            self.previous = Some(old);
        }
        Ok(())
    }
    pub fn width(&mut self, width: usize) -> Result<(), String> {
        if self.current.width != width {
            self.relayout_width(width)?;
        }
        Ok(())
    }
    pub fn position(&mut self, position: Position) -> Result<Position, String> {
        match position {
            Position::Relative {
                anchor,
                rows,
                width,
            } => {
                let index = if self.current.generation == self.committed
                    && self.current.width == width
                    && anchor.event < self.current.event
                {
                    &mut self.current
                } else {
                    self.previous
                        .as_mut()
                        .filter(|index| index.width == width)
                        .ok_or("Displayed row layout is no longer available.")?
                };
                let row = index
                    .row(anchor)?
                    .saturating_add_signed(rows)
                    .min(index.total.saturating_sub(1));
                Ok(Position::Anchor(index.anchor(row)?))
            }
            other => Ok(other),
        }
    }
    pub fn layout(&mut self) -> Result<bool, String> {
        self.layout_chunk(CHUNK)
    }
    #[cfg(test)]
    pub(super) fn private(&self) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            self.text.metadata().unwrap().mode() & 0o777 == 0o600
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            crate::platform::is_owner_only(self.text.as_raw_handle()).unwrap()
        }
    }
    pub(super) fn layout_chunk(&mut self, chunk: usize) -> Result<bool, String> {
        let mut budget = chunk;
        let mut events = 0;
        while self.current.event < self.texts.len() && budget > 0 && events < LAYOUT_EVENTS {
            let event = self.current.event;
            if self.current.rows.len() == event {
                self.current.rows.push(Rows {
                    first: self.current.total,
                    count: 0,
                    rendered_end: self.current.output_offset,
                });
            }
            let length = self.texts[event].bytes;
            let cursor = self.current.cursor;
            let end = (cursor + budget).min(length);
            if end > cursor {
                let bytes = self.bytes(event, cursor..end)?;
                let text = match std::str::from_utf8(&bytes) {
                    Ok(text) => text,
                    Err(error) => std::str::from_utf8(&bytes[..error.valid_up_to()])
                        .map_err(|error| error.to_string())?,
                };
                let mut processed = 0;
                for (byte, grapheme) in text.grapheme_indices(true) {
                    // The last grapheme/UTF-8 scalar may continue in the next chunk.
                    if end < length && byte + grapheme.len() == text.len() {
                        break;
                    }
                    let absolute = cursor + byte;
                    if grapheme.contains('\n') {
                        self.current.append(self.current.start..absolute)?;
                        self.current.start = absolute + grapheme.len();
                        self.current.columns = 0;
                    } else if !grapheme.contains(char::is_control) {
                        let size = usize::from(grapheme.cell_width());
                        if size <= self.current.width {
                            if self.current.columns + size > self.current.width {
                                self.current.append(self.current.start..absolute)?;
                                self.current.start = absolute;
                                self.current.columns = 0;
                            }
                            self.current.columns += size;
                            self.current.output.push_str(grapheme);
                        }
                    }
                    processed = byte + grapheme.len();
                }
                if processed == 0 && budget < chunk {
                    return Ok(true);
                }
                if processed == 0 {
                    // A grapheme can contain arbitrarily many combining scalars. Decode this
                    // exceptional tail once rather than rejecting valid text at an arbitrary cap.
                    let bytes = self.bytes(event, cursor..length)?;
                    let text = std::str::from_utf8(&bytes).map_err(|error| error.to_string())?;
                    let grapheme = text.graphemes(true).next().ok_or("Empty text tail.")?;
                    if grapheme.contains('\n') {
                        self.current.append(self.current.start..cursor)?;
                        self.current.start = cursor + grapheme.len();
                        self.current.columns = 0;
                    } else if !grapheme.contains(char::is_control)
                        && usize::from(grapheme.cell_width()) <= self.current.width
                    {
                        let size = usize::from(grapheme.cell_width());
                        if self.current.columns + size > self.current.width {
                            self.current.append(self.current.start..cursor)?;
                            self.current.start = cursor;
                            self.current.columns = 0;
                        }
                        self.current.columns += size;
                        self.current.output.push_str(grapheme);
                    }
                    self.current.cursor += grapheme.len();
                    return Ok(true);
                }
                self.current.cursor += processed;
                budget = budget.saturating_sub(processed);
                if end < length {
                    return Ok(true);
                }
            }
            if self.current.cursor == length {
                if self.current.start < length {
                    self.current.append(self.current.start..length)?;
                }
                if self.current.rows[event].count != 0 {
                    self.current.visible.push(event);
                }
                self.current.event += 1;
                self.current.cursor = 0;
                self.current.start = 0;
                self.current.columns = 0;
                events += 1;
            }
        }
        Ok(self.current.event < self.texts.len())
    }
    pub fn anchors(&mut self, top: usize, height: usize) -> Result<Vec<Anchor>, String> {
        (top..top.saturating_add(height).min(self.current.total))
            .map(|row| self.current.anchor(row))
            .collect()
    }
    pub fn window(
        &mut self,
        position: Position,
        height: usize,
    ) -> Result<(usize, usize, Anchor, Vec<String>), String> {
        let bottom = self.current.total.saturating_sub(height);
        let top = match position {
            Position::Bottom => bottom,
            Position::Row(row) => row.min(bottom),
            Position::Anchor(anchor) => self.current.row(anchor)?.min(bottom),
            Position::Relative { .. } => return Err("Unresolved relative position.".into()),
        };
        let anchor = self.current.anchor(top)?;
        let mut rows = Vec::new();
        for row in top..(top + height).min(self.current.total) {
            let rendered = self.current.rendered(row)?;
            self.bytes_read += rendered.len() as u64;
            rows.push(rendered);
        }
        Ok((top, self.current.total, anchor, rows))
    }
}
