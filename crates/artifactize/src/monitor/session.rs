//! Selected-session jobs. Geometry/input are pure; the driver alone starts bounded reader jobs.
use crate::agent::session::{
    document::{Anchor, Mode, Move, Position, Scroll},
    live::{Reader, Source, Window},
};

pub(super) struct Live {
    pub serial: u64,
    pub source: Source,
    pub reader: Option<Reader>,
    pub scroll: Scroll,
    pub window: Window,
    width: usize,
    height: usize,
    anchor: Anchor,
    position: Position,
    revision: u64,
    rendered_width: usize,
    dirty: bool,
    expanded: std::collections::BTreeSet<crate::agent::session::transcript::BlockId>,
}

pub(super) struct Job {
    pub serial: u64,
    pub revision: u64,
    pub width: usize,
    pub height: usize,
    pub position: Position,
    pub reader: Reader,
    pub expanded: std::collections::BTreeSet<crate::agent::session::transcript::BlockId>,
}
impl Live {
    pub fn new(serial: u64, source: Source) -> Self {
        Self {
            serial,
            source: source.clone(),
            reader: Some(Reader::new(source)),
            scroll: Scroll::default(),
            window: Window::default(),
            width: 0,
            height: 0,
            anchor: Anchor::default(),
            position: Position::Bottom,
            revision: 0,
            rendered_width: 0,
            dirty: true,
            expanded: std::collections::BTreeSet::new(),
        }
    }
    pub fn geometry(&mut self, width: usize, height: usize) {
        if self.width != width || self.height != height {
            self.width = width;
            self.height = height;
            // Preserve pending Row/Top intent. The reader converts an old-layout row to
            // its logical anchor before changing width; committed positions already anchor.
            self.revision += 1;
            self.dirty = true;
        }
    }
    pub fn invalidate(&mut self) {
        self.dirty = true;
    }
    #[cfg(test)]
    pub fn expanded(&self, id: crate::agent::session::transcript::BlockId) -> bool {
        self.expanded.contains(&id)
    }
    pub fn toggle(&mut self, id: crate::agent::session::transcript::BlockId) {
        if !self.expanded.remove(&id) {
            self.expanded.insert(id);
        }
        self.revision += 1;
        self.dirty = true;
    }
    pub fn toggle_visible(&mut self) {
        if let Some((_, id)) = self.window.groups.first().copied() {
            self.toggle(id);
        }
    }
    pub fn movement(&mut self, movement: Move) {
        let before = self.scroll.top;
        self.scroll.apply(movement);
        let delta = self.scroll.top as isize - before as isize;
        self.position = match movement {
            Move::Bottom => Position::Bottom,
            Move::Top => Position::Anchor(Anchor::default()),
            Move::Up(_) | Move::Down(_) if self.scroll.mode == Mode::Following => Position::Bottom,
            Move::Up(_) | Move::Down(_) => match self.position {
                Position::Anchor(anchor) => Position::Relative {
                    anchor,
                    rows: delta,
                    width: self.rendered_width.max(1),
                },
                Position::Relative {
                    anchor,
                    rows,
                    width,
                } => Position::Relative {
                    anchor,
                    rows: rows.saturating_add(delta),
                    width,
                },
                _ => Position::Relative {
                    anchor: self.anchor,
                    rows: self.scroll.top as isize - self.window.top as isize,
                    width: self.rendered_width.max(1),
                },
            },
        };
        self.revision += 1;
        self.dirty = true;
    }
    pub fn job(&mut self) -> Option<Job> {
        if !self.dirty || self.width == 0 || self.height == 0 {
            return None;
        }
        let reader = self.reader.take()?;
        self.dirty = false;
        Some(Job {
            serial: self.serial,
            revision: self.revision,
            width: self.width,
            height: self.height,
            position: self.position,
            reader,
            expanded: self.expanded.clone(),
        })
    }
    pub fn finish(&mut self, job: Job, window: Window) {
        let mut reader = job.reader;
        reader.source = self.source.clone();
        self.reader = Some(reader);
        // A replacement creates a different document even if its visible result is stale.
        if window.reset {
            self.scroll = Scroll::default();
            self.position = Position::Bottom;
            self.dirty = true;
        }
        if self.revision != job.revision {
            self.dirty = true;
            return;
        }
        if window.loading && window.rows.is_empty() && window.status.is_none() {
            // Keep the old visible text during bounded relayout, with an honest loading label.
            self.window.loading = true;
            self.position = window.position;
        } else {
            self.scroll
                .update(window.total, self.height, Some(window.top));
            self.anchor = window.anchor;
            self.rendered_width = job.width;
            if let Some(reader) = &mut self.reader {
                reader.commit(job.width);
            }
            self.position = match self.scroll.mode {
                Mode::Following => Position::Bottom,
                Mode::Paused => Position::Anchor(self.anchor),
            };
            self.window = window;
        }
        self.dirty |= self.window.loading;
    }
    pub fn indicator(&self) -> String {
        let position = if self.scroll.total == 0 {
            "0/0".into()
        } else {
            format!("{}/{}", self.scroll.top + 1, self.scroll.total)
        };
        format!(
            "{position} {} {}{}",
            if self.scroll.at_bottom() {
                "bottom"
            } else {
                "history"
            },
            if self.scroll.mode == Mode::Following {
                "following"
            } else {
                "paused"
            },
            if self.window.loading {
                " · indexing…"
            } else {
                ""
            }
        )
    }
}
