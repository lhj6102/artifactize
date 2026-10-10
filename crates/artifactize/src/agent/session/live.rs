//! Incremental, state-scoped session reading. Call bounded `step` jobs off the UI thread.
//! Only byte offsets and row counts survive cache eviction; older history is paged in on demand.
use super::{
    Event, Header, Kind, SessionRef,
    document::{Anchor, Position},
};
use crate::{
    platform,
    store::{self, RequestView},
    types::SessionId,
};
use std::{
    collections::VecDeque,
    ffi::OsStr,
    fs::{File, Metadata},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::SystemTime,
};

/// Bound each indexing job, so even a continuously growing session yields to terminal input.
const BATCH_BYTES: usize = 256 * 1024;
const BATCH_EVENTS: usize = 128;
/// Small content guards detect truncate/regrow and in-place changes around already read data.
const GUARD_BYTES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub state: PathBuf,
    pub reference: SessionRef,
    pub saved: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    Local(Source),
    Unavailable(String),
}

pub async fn resolve(state: &Path, view: &RequestView) -> Result<Resolution, String> {
    let state_id: crate::types::StateId =
        store::read_state_id(state)?.ok_or("State has no identity.")?;
    let producer = store::Producer::current().name;
    let mut view = view.clone();
    // A local follower may have no session_id until completion. Resolve only its original
    // local producer, never a mirrored provenance id that happens to collide in this state.
    if view.request.origin.is_none()
        && view
            .request
            .producer
            .as_ref()
            .is_none_or(|source| source.name == producer)
        && view
            .execution
            .as_ref()
            .and_then(|execution| execution.producer.as_ref())
            .is_none_or(|source| source.name == producer)
        && view
            .execution
            .as_ref()
            .is_none_or(|execution| execution.origin.is_none())
    {
        let original = view
            .execution
            .as_ref()
            .map(|execution| &execution.provenance)
            .or(view.request.provenance.as_ref());
        if let Some(original) = original
            && original.request_id != view.request.id
            && view.request.session.is_none()
            && view
                .request
                .producer
                .as_ref()
                .and_then(|producer| producer.session.as_ref())
                .is_none()
            && view
                .execution
                .as_ref()
                .and_then(|execution| execution.producer.as_ref())
                .and_then(|producer| producer.session.as_ref())
                .is_none()
        {
            let source = store::read_request(state, &original.request_id).await?;
            if source.request.run_id != original.run_id {
                return Err("Original session request has a different Run identity.".into());
            }
            view = source;
        }
    }
    let request = &view.request;
    let reference = request
        .session
        .as_ref()
        .or_else(|| {
            request
                .producer
                .as_ref()
                .and_then(|producer| producer.session.as_ref())
        })
        .or_else(|| {
            view.execution
                .as_ref()
                .and_then(|execution| execution.producer.as_ref())
                .and_then(|producer| producer.session.as_ref())
        });
    if let Some(reference) = reference {
        if !reference.valid() {
            return Err("Invalid saved session reference.".into());
        }
        if reference.state != state_id || reference.producer != producer {
            return Ok(Resolution::Unavailable(format!(
                "This conversation lives in {}'s state {} (session {}). It is remote/unavailable here, not removed by local session GC.",
                reference.producer, reference.state, reference.session_id
            )));
        }
        return Ok(Resolution::Local(Source {
            state: state.into(),
            reference: reference.clone(),
            saved: true,
        }));
    }
    if request.origin.is_some()
        || view
            .execution
            .as_ref()
            .is_some_and(|execution| execution.origin.is_some())
        || request
            .producer
            .as_ref()
            .is_some_and(|source| source.name != producer)
        || view
            .execution
            .as_ref()
            .and_then(|execution| execution.producer.as_ref())
            .is_some_and(|source| source.name != producer)
    {
        return Ok(Resolution::Unavailable(
            "This remote conversation has no local session reference; it is unavailable here."
                .into(),
        ));
    }
    match &request.session_id {
        Some(id) => Ok(Resolution::Local(Source {
            state: state.into(),
            reference: SessionRef {
                producer,
                state: state_id,
                run_id: request.run_id.clone(),
                request_id: request.id.clone(),
                session_id: id.clone(),
            },
            saved: false,
        })),
        None => Ok(Resolution::Unavailable(
            "No saved Agent conversation was recorded for this request. It may have reused evidence with no session reference, predate session saving, or never have started an Agent review."
                .into(),
        )),
    }
}

fn identity(file: &File) -> Result<platform::FileIdentity, String> {
    platform::file_identity(file).map_err(|error| error.to_string())
}
fn private_directory(file: &File) -> Result<(), String> {
    if !file.metadata().map_err(|error| error.to_string())?.is_dir() {
        return Err("Session store is not a directory.".into());
    }
    if !platform::is_owner_only(file).map_err(|error| error.to_string())? {
        return Err("Session store must be owner-only and owned by this user.".into());
    }
    Ok(())
}
fn open(source: &Source) -> Result<Option<File>, String> {
    let root = platform::open_directory(&source.state).map_err(|error| error.to_string())?;
    let name =
        platform::EntryName::new(OsStr::new(super::DIRECTORY)).expect("constant session directory");
    let result = (|| {
        let directory = platform::open_entry(&root, &name)?;
        private_directory(&directory).map_err(std::io::Error::other)?;
        let name = platform::EntryName::new(OsStr::new(&format!(
            "{}.jsonl",
            source.reference.session_id
        )))
        .expect("validated session id");
        let file = platform::open_entry(&directory, &name)?;
        if !platform::is_private_file(&file)? {
            return Err(std::io::Error::other(
                "Session must be a private, single-link regular file.",
            ));
        }
        if !platform::is_owner_only(&file)? {
            return Err(std::io::Error::other("Session belongs to another user."));
        }
        Ok(file)
    })();
    match result {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

#[derive(Debug, thiserror::Error)]
enum ReadError {
    #[error("{0}")]
    Data(String),
    #[error("{0}")]
    Io(String),
}

impl From<String> for ReadError {
    fn from(error: String) -> Self {
        Self::Io(error)
    }
}

/// A selected reader owns its ephemeral text/row files. Dropping or resetting it removes them.
pub struct Reader {
    pub source: Source,
    file: Option<File>,
    identity: Option<platform::FileIdentity>,
    modified: Option<SystemTime>,
    length: u64,
    offset: u64,
    start: u64,
    pending: VecDeque<(u64, usize)>,
    chunk: Vec<u8>,
    chunk_offset: u64,
    guards: (Vec<u8>, Vec<u8>),
    records: usize,
    pages: Option<super::pages::Pages>,
    transcript: super::transcript::Transcript,
    error: Option<String>,
    pub bytes_read: u64,
    pub decoded_events: usize,
}
#[derive(Debug, Clone, Default)]
pub struct Window {
    pub rows: Vec<String>,
    pub top: usize,
    pub total: usize,
    pub anchor: Anchor,
    pub position: Position,
    pub loading: bool,
    pub reset: bool,
    pub status: Option<String>,
    pub groups: Vec<(usize, super::transcript::BlockId)>,
    pub thinking: Vec<usize>,
}
impl Reader {
    pub fn new(source: Source) -> Self {
        Self {
            source,
            file: None,
            identity: None,
            modified: None,
            length: 0,
            offset: 0,
            start: 0,
            pending: VecDeque::new(),
            chunk: Vec::new(),
            chunk_offset: 0,
            guards: Default::default(),
            records: 0,
            pages: None,
            transcript: super::transcript::Transcript::default(),
            error: None,
            bytes_read: 0,
            decoded_events: 0,
        }
    }
    #[cfg(test)]
    pub(crate) fn records_len(&self) -> usize {
        self.records
    }
    pub fn commit(&mut self, width: usize) {
        if let Some(pages) = &mut self.pages {
            pages.commit(width);
        }
    }
    pub fn id(&self) -> &SessionId {
        &self.source.reference.session_id
    }
    fn reset(&mut self) {
        self.file = None;
        self.identity = None;
        self.length = 0;
        self.offset = 0;
        self.start = 0;
        self.pending.clear();
        self.chunk.clear();
        self.guards = Default::default();
        self.records = 0;
        self.pages = None;
        self.transcript = super::transcript::Transcript::default();
        self.error = None;
    }
    fn bytes(&mut self, offset: u64, size: usize) -> Result<Vec<u8>, String> {
        let file = self.file.as_mut().ok_or("Session file is unavailable.")?;
        file.seek(SeekFrom::Start(offset))
            .map_err(|error| error.to_string())?;
        let mut bytes = vec![0; size];
        file.read_exact(&mut bytes)
            .map_err(|error| error.to_string())?;
        self.bytes_read += size as u64;
        Ok(bytes)
    }
    fn guards(&mut self) -> Result<(Vec<u8>, Vec<u8>), String> {
        let size = GUARD_BYTES.min(self.offset as usize);
        Ok((
            self.bytes(0, size)?,
            self.bytes(self.offset - size as u64, size)?,
        ))
    }
    fn probe(&mut self) -> Result<(bool, bool), String> {
        let Some(file) = open(&self.source)? else {
            let reset = self.identity.is_some();
            self.reset();
            return Ok((reset, false));
        };
        let metadata: Metadata = file.metadata().map_err(|error| error.to_string())?;
        let identity = identity(&file)?;
        let modified = metadata.modified().ok();
        let mut reset = self.identity.is_some_and(|old| old != identity)
            || metadata.len() < self.offset
            || metadata.len() < self.length
            || self.identity.is_some()
                && metadata.len() == self.length
                && self.modified != modified;
        self.file = Some(file);
        if !reset && self.offset != 0 && self.guards()? != self.guards {
            reset = true;
        }
        if reset {
            let file = self.file.take();
            self.reset();
            self.file = file;
        }
        self.identity = Some(identity);
        self.modified = modified;
        self.length = metadata.len();
        Ok((reset, true))
    }
    fn validate(&self, event: &Event, index: usize) -> Result<(), String> {
        let Kind::Review(header) = &event.kind else {
            return if index == 0 {
                Err("Session has no review header.".into())
            } else {
                Ok(())
            };
        };
        if index != 0 {
            return Err("Session contains a second review header.".into());
        }
        let Header {
            session_id,
            run_id,
            request_id,
            producer,
            state,
            version,
            ..
        } = header.as_ref();
        let expected = &self.source.reference;
        let mismatch = session_id
            .as_ref()
            .is_some_and(|id| id != &expected.session_id)
            || run_id.as_ref().is_some_and(|id| id != &expected.run_id)
            || request_id
                .as_ref()
                .is_some_and(|id| id != &expected.request_id)
            || producer.as_ref().is_some_and(|id| id != &expected.producer)
            || state.as_ref().is_some_and(|id| id != &expected.state);
        let incomplete = version.is_some()
            && (session_id.is_none()
                || run_id.is_none()
                || request_id.is_none()
                || producer.is_none()
                || state.is_none());
        if mismatch || incomplete {
            return Err(
                "Session header does not match its state, producer, Run, request and session identity."
                    .into(),
            );
        }
        if version.is_some_and(|version| version != super::VERSION) {
            return Err("Unsupported saved session version.".into());
        }
        Ok(())
    }
    fn index(&mut self) -> Result<bool, ReadError> {
        if self.pending.is_empty() && self.offset < self.length {
            let available = BATCH_BYTES.min(self.length.saturating_sub(self.offset) as usize);
            let bytes = self.bytes(self.offset, available)?;
            for (byte, _) in bytes.iter().enumerate().filter(|(_, byte)| **byte == b'\n') {
                let end = self.offset + byte as u64;
                self.pending
                    .push_back((self.start, (end - self.start) as usize));
                self.start = end + 1;
            }
            self.chunk_offset = self.offset;
            self.chunk = bytes;
            self.offset += available as u64;
        }
        let mut budget = 0;
        let mut count = 0;
        while count < BATCH_EVENTS && budget < BATCH_BYTES {
            let Some((offset, size)) = self.pending.pop_front() else {
                break;
            };
            // One complete typed Event and its pretty text require temporary memory proportional
            // to that event. No arbitrary producer-incompatible record cap, permanent clone or
            // per-refresh decode: both are dropped after the ephemeral text file is written.
            let owned;
            let bytes = if offset >= self.chunk_offset
                && offset + size as u64 <= self.chunk_offset + self.chunk.len() as u64
            {
                &self.chunk[(offset - self.chunk_offset) as usize
                    ..(offset - self.chunk_offset) as usize + size]
            } else {
                owned = self.bytes(offset, size)?;
                &owned
            };
            let event = Event::parse(bytes).map_err(|error| {
                ReadError::Data(format!(
                    "Invalid saved session event at line {}: {error}",
                    self.records + 1
                ))
            })?;
            self.validate(&event, self.records)
                .map_err(ReadError::Data)?;
            let patches = self.transcript.apply(&event);
            for patch in patches {
                self.pages
                    .as_mut()
                    .expect("pages initialized")
                    .set(patch.id.0, &patch.text)?;
            }
            self.records += 1;
            self.decoded_events += 1;
            budget += size;
            count += 1;
        }
        self.guards = self.guards()?;
        Ok(self.offset < self.length || !self.pending.is_empty())
    }
    /// Chunk scan and disk layout are bounded; the one-time typed decode of a completed
    /// record necessarily allocates its Event and formatted text, then releases both.
    pub fn step(&mut self, width: usize, height: usize, position: Position) -> Window {
        self.step_expanded(width, height, position, &std::collections::BTreeSet::new())
    }
    pub fn step_expanded(
        &mut self,
        width: usize,
        height: usize,
        position: Position,
        expanded: &std::collections::BTreeSet<super::transcript::BlockId>,
    ) -> Window {
        let result = (|| {
            let (reset, exists) = self.probe()?;
            if !exists {
                return Ok(Window {
                    reset,
                    status: Some(
                        if self.source.saved {
                            "This Agent session was removed by session GC (or the saved file was removed)."
                        } else {
                            "This Agent session was never recorded as saved, or has not been created yet."
                        }
                        .into(),
                    ),
                    ..Window::default()
                });
            }
            if width == 0 {
                return Ok(Window {
                    reset,
                    ..Window::default()
                });
            }
            if self.pages.is_none() {
                self.pages = Some(super::pages::Pages::new(width)?);
            }
            let position = if reset {
                Position::Bottom
            } else {
                self.pages
                    .as_mut()
                    .expect("pages initialized")
                    .position(position)?
            };
            for patch in self.transcript.expansion(expanded) {
                self.pages
                    .as_mut()
                    .expect("pages initialized")
                    .set(patch.id.0, &patch.text)?;
            }
            self.pages
                .as_mut()
                .expect("pages initialized")
                .width(width)?;
            let indexing = if self.error.is_none() {
                self.index()?
            } else {
                false
            };
            let pages = self.pages.as_mut().expect("pages initialized");
            let layout = pages.layout()?;
            let mut window = if layout {
                Window::default()
            } else {
                let (top, total, anchor, rows) = pages.window(position, height)?;
                let groups = pages
                    .anchors(top, height)?
                    .into_iter()
                    .enumerate()
                    .filter_map(|(row, anchor)| {
                        let id = super::transcript::BlockId(anchor.event);
                        self.transcript.is_group(id).then_some((row, id))
                    })
                    .collect();
                let thinking = pages
                    .anchors(top, height)?
                    .into_iter()
                    .enumerate()
                    .filter_map(|(row, anchor)| {
                        self.transcript
                            .is_thinking(super::transcript::BlockId(anchor.event))
                            .then_some(row)
                    })
                    .collect();
                Window {
                    top,
                    total,
                    anchor,
                    rows,
                    groups,
                    thinking,
                    ..Window::default()
                }
            };
            window.position = position;
            window.reset = reset;
            window.loading = self.error.is_none() && (indexing || layout);
            window.status = self.error.clone().or_else(|| {
                (self.records == 0).then(|| "Waiting for a complete review header…".into())
            });
            Ok::<_, ReadError>(window)
        })();
        match result {
            Ok(window) => window,
            Err(error) => {
                let reset = matches!(error, ReadError::Io(_));
                match &error {
                    ReadError::Data(message) => {
                        if self.file.is_some()
                            && let Ok(guards) = self.guards()
                        {
                            self.guards = guards;
                        }
                        self.error = Some(message.clone());
                    }
                    ReadError::Io(_) => self.reset(),
                }
                Window {
                    reset,
                    status: Some(format!("Conversation unavailable: {error}")),
                    ..Window::default()
                }
            }
        }
    }
}
