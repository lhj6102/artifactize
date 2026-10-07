//! Pure session layout and scrolling, shared by terminal and future graphical viewers.
//! Rows are indexed by logical UTF-8 offsets, not terminal-sized global scroll coordinates.
use super::{Event, Kind};
use ratatui::buffer::CellWidth;
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Anchor {
    pub event: usize,
    pub byte: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Position {
    #[default]
    Bottom,
    Row(usize),
    /// Movement is relative to the displayed logical anchor and its wrapping width.
    Relative {
        anchor: Anchor,
        rows: isize,
        width: usize,
    },
    Anchor(Anchor),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    Up(usize),
    Down(usize),
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Following,
    Paused,
}

/// Input sources all use the same clamping and follow transition rules.
#[derive(Debug, Clone)]
pub struct Scroll {
    pub top: usize,
    pub total: usize,
    pub height: usize,
    pub mode: Mode,
}
impl Default for Scroll {
    fn default() -> Self {
        Self {
            top: 0,
            total: 0,
            height: 0,
            mode: Mode::Following,
        }
    }
}
impl Scroll {
    pub fn bottom(&self) -> usize {
        self.total.saturating_sub(self.height)
    }
    pub fn at_bottom(&self) -> bool {
        self.top == self.bottom()
    }
    pub fn update(&mut self, total: usize, height: usize, anchor_row: Option<usize>) {
        self.total = total;
        self.height = height;
        self.top = match self.mode {
            Mode::Following => self.bottom(),
            Mode::Paused => anchor_row.unwrap_or(self.top).min(self.bottom()),
        };
        // Resizing/short content reaching the bottom is also a return to bottom.
        if self.at_bottom() {
            self.mode = Mode::Following;
        }
    }
    pub fn apply(&mut self, movement: Move) -> Position {
        self.top = match movement {
            Move::Up(rows) => self.top.saturating_sub(rows),
            Move::Down(rows) => self.top.saturating_add(rows).min(self.bottom()),
            Move::Top => 0,
            Move::Bottom => self.bottom(),
        };
        self.mode = if self.at_bottom() {
            Mode::Following
        } else {
            Mode::Paused
        };
        match self.mode {
            Mode::Following => Position::Bottom,
            Mode::Paused => Position::Row(self.top),
        }
    }
}

pub fn text(event: &Event) -> Result<String, String> {
    match &event.kind {
        Kind::Review(header) => Ok(format!(
            "Session {}\n",
            header
                .session_id
                .as_ref()
                .map_or("unreported", |id| id.as_str())
        )),
        Kind::Message(message) => Ok(format!(
            "\nTurn {}\n{}\n",
            message.turn,
            serde_json::to_string_pretty(&message.message).map_err(|error| error.to_string())?
        )),
        Kind::End(end) => Ok(format!(
            "\nReview end\n{}\n",
            serde_json::to_string_pretty(end).map_err(|error| error.to_string())?
        )),
        Kind::Send(send) => Ok(format!(
            "\nFollow-up\n{}\n",
            serde_json::to_string_pretty(send).map_err(|error| error.to_string())?
        )),
        Kind::Answer(answer) => Ok(format!(
            "\nAnswer\n{}\n",
            serde_json::to_string_pretty(answer).map_err(|error| error.to_string())?
        )),
        Kind::Attempt(_) => Ok(String::new()),
    }
}

/// Character wrapping is explicit: these exact rows render without a second wrapping pass.
/// Grapheme boundaries keep CJK, combining characters and emoji intact. Control characters
/// are omitted just as ratatui's text renderer omits them; an over-wide grapheme is skipped.
pub fn rows(text: &str, width: usize) -> Vec<Range<usize>> {
    if text.is_empty() || width == 0 {
        return Vec::new();
    }
    let mut rows = Vec::new();
    let mut start = 0;
    let mut columns = 0;
    for (byte, grapheme) in text.grapheme_indices(true) {
        if grapheme.contains('\n') {
            rows.push(start..byte);
            start = byte + grapheme.len();
            columns = 0;
        } else if !grapheme.contains(char::is_control) {
            let size = usize::from(grapheme.cell_width());
            if size <= width {
                if columns + size > width {
                    rows.push(start..byte);
                    start = byte;
                    columns = 0;
                }
                columns += size;
            }
        }
    }
    if start < text.len() {
        rows.push(start..text.len());
    }
    rows
}

pub fn row(text: &str, range: &Range<usize>, width: usize) -> String {
    text[range.clone()]
        .graphemes(true)
        .filter(|grapheme| {
            !grapheme.contains(char::is_control) && usize::from(grapheme.cell_width()) <= width
        })
        .collect()
}
