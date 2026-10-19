//! Pure read/search calculations over supplied bytes, paths and file sizes.
use globset::GlobMatcher;

use super::{input, output, relative};

pub(super) enum ReadEnd {
    Complete,
    Bounded,
}
pub(super) enum ReadStep {
    More(usize),
    Full(usize),
    Bounded,
}

pub(super) struct Read {
    offset: usize,
    limit: usize,
    lines: Vec<output::Line>,
    line: Vec<u8>,
    current: usize,
    returned_bytes: usize,
    partial_line: bool,
}
impl Read {
    pub fn new(offset: usize, limit: usize) -> Self {
        Self {
            offset,
            limit,
            lines: Vec::new(),
            line: Vec::new(),
            current: 1,
            returned_bytes: 0,
            partial_line: false,
        }
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<ReadStep, String> {
        let end = bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|index| index + 1);
        let fragment = &bytes[..end.unwrap_or(bytes.len())];
        if self.current >= self.offset {
            if self.returned_bytes + self.line.len() + fragment.len() > super::READ_BYTES {
                if self.lines.is_empty() {
                    return Err(format!(
                        "Artifact line {} exceeds the {}-byte read limit.",
                        self.current,
                        super::READ_BYTES
                    ));
                }
                return Ok(ReadStep::Bounded);
            }
            if fragment.contains(&0) {
                return Err("Binary text (NUL) cannot be read.".into());
            }
            self.line.extend_from_slice(fragment);
        }
        let consumed = fragment.len();
        self.partial_line = end.is_none();
        if end.is_some() {
            if self.current >= self.offset {
                self.lines.push(text_line(self.current, &self.line)?);
                self.returned_bytes += self.line.len();
                self.line.clear();
            }
            self.current = self
                .current
                .checked_add(1)
                .ok_or("Artifact has too many lines.")?;
            if self.lines.len() == self.limit || self.returned_bytes == super::READ_BYTES {
                return Ok(ReadStep::Full(consumed));
            }
        }
        Ok(ReadStep::More(consumed))
    }

    pub fn finish(mut self, end: ReadEnd) -> Result<output::ReadRows, String> {
        if matches!(end, ReadEnd::Complete) && !self.line.is_empty() {
            self.lines.push(text_line(self.current, &self.line)?);
        }
        let page = match end {
            ReadEnd::Complete => output::ReadPage::Complete {
                total_lines: self.current - 1 + usize::from(self.partial_line),
            },
            ReadEnd::Bounded => output::ReadPage::Bounded {
                next_offset: self.current,
            },
        };
        Ok(output::ReadRows {
            start_line: self.offset,
            end_line: self.lines.last().map(|line| line.number),
            line_count: self.lines.len(),
            lines: self.lines,
            page,
        })
    }
}

fn text_line(number: usize, line: &[u8]) -> Result<output::Line, String> {
    let text = std::str::from_utf8(line)
        .map_err(|_| "Artifact contains invalid UTF-8 in the requested lines.")?;
    Ok(output::Line {
        number,
        text: text.to_owned(),
    })
}

pub(super) enum Selection {
    Skip,
    Added,
    Full,
}
pub(super) struct Glob<'a> {
    path: &'a str,
    matcher: &'a GlobMatcher,
    files: Vec<String>,
    bytes: usize,
    truncated: bool,
}
impl<'a> Glob<'a> {
    pub fn new(path: &'a str, matcher: &'a GlobMatcher, truncated: bool) -> Self {
        Self {
            path,
            matcher,
            files: Vec::new(),
            bytes: super::RESULT_ENVELOPE_BYTES
                + serde_json::to_vec(path).expect("path is JSON").len(),
            truncated,
        }
    }
    pub fn push(&mut self, file: &str) -> Selection {
        if !self.matcher.is_match(relative(self.path, file)) {
            return Selection::Skip;
        }
        self.bytes += serde_json::to_vec(file).expect("path is JSON").len() + 1;
        if self.files.len() == super::MAX_RESULTS || self.bytes > super::RESULT_BYTES {
            self.truncated = true;
            return Selection::Full;
        }
        self.files.push(file.to_owned());
        Selection::Added
    }
    pub fn finish(self) -> output::Glob {
        output::Glob {
            path: self.path.to_owned(),
            files: self.files,
            truncated: self.truncated,
        }
    }
}

pub(super) enum FileRead {
    Read(u64),
    Skip,
    Full,
}
pub(super) enum Received {
    Text,
    Skip,
    Full,
}
pub(super) struct Grep<'a> {
    path: &'a str,
    input: &'a input::Grep,
    matches: Vec<output::GrepMatch>,
    searched: usize,
    bytes: usize,
    truncated: bool,
}
impl<'a> Grep<'a> {
    pub fn new(path: &'a str, input: &'a input::Grep, truncated: bool) -> Self {
        Self {
            path,
            input,
            matches: Vec::new(),
            searched: 0,
            bytes: super::RESULT_ENVELOPE_BYTES
                + serde_json::to_vec(path).expect("path is JSON").len(),
            truncated,
        }
    }
    pub fn allows(&self, file: &str) -> bool {
        self.input
            .glob
            .as_ref()
            .is_none_or(|filter| filter.is_match(relative(self.path, file)))
    }
    pub fn file(&mut self, bytes: u64) -> FileRead {
        if bytes > super::SEARCH_FILE_BYTES as u64 {
            self.truncated = true;
            return FileRead::Skip;
        }
        if self.searched + bytes as usize > super::SEARCH_BYTES {
            self.truncated = true;
            return FileRead::Full;
        }
        // One extra byte detects growth beyond either advertised search bound.
        FileRead::Read(
            (super::SEARCH_FILE_BYTES.min(super::SEARCH_BYTES - self.searched) + 1) as u64,
        )
    }
    pub fn received(&mut self, bytes: usize) -> Received {
        self.searched += bytes;
        if self.searched > super::SEARCH_BYTES {
            self.truncated = true;
            Received::Full
        } else if bytes > super::SEARCH_FILE_BYTES {
            self.truncated = true;
            Received::Skip
        } else {
            Received::Text
        }
    }
    pub fn line(&mut self, path: &str, index: usize, line: &str) -> bool {
        let line = line.strip_suffix('\n').unwrap_or(line);
        let line = line.strip_suffix('\r').unwrap_or(line);
        if !self.input.pattern.is_match(line) {
            return true;
        }
        let entry = output::GrepMatch {
            path: path.to_owned(),
            line: index + 1,
            text: line.to_owned(),
        };
        self.bytes += serde_json::to_vec(&entry).expect("match is JSON").len() + 1;
        if self.matches.len() == self.input.max_results || self.bytes > super::RESULT_BYTES {
            self.truncated = true;
            return false;
        }
        self.matches.push(entry);
        true
    }
    pub fn finish(self) -> output::Grep {
        output::Grep {
            path: self.path.to_owned(),
            matches: self.matches,
            truncated: self.truncated,
        }
    }
}

pub(super) fn text(bytes: &[u8]) -> Option<&str> {
    (!bytes.contains(&0))
        .then(|| std::str::from_utf8(bytes).ok())
        .flatten()
}

/// Pagination and serialized-size accounting are independent of directory acquisition.
pub(super) fn list(
    artifact_id: crate::scope::ArtifactId,
    path: &str,
    entries: Vec<output::Entry>,
    offset: usize,
    limit: usize,
) -> Result<output::List, String> {
    let total = entries.len();
    let mut bytes =
        super::RESULT_ENVELOPE_BYTES + serde_json::to_vec(path).expect("path is JSON").len();
    let entries: Vec<_> = entries
        .into_iter()
        .skip(offset)
        .take(limit)
        .take_while(|entry| {
            bytes += serde_json::to_vec(entry).expect("entry is JSON").len() + 1;
            bytes <= super::RESULT_BYTES
        })
        .collect();
    if entries.is_empty() && offset < total {
        return Err("Listing entry exceeds 512 KiB; narrow the path.".into());
    }
    let next = offset + entries.len();
    Ok(output::List {
        artifact_id,
        path: path.to_owned(),
        entries,
        total_entries: total,
        page: if next < total {
            output::Page::Bounded { next_offset: next }
        } else {
            output::Page::Complete
        },
    })
}
