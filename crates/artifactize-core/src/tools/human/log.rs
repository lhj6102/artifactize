//! The Tool log: what Human tools print while a review or an Artifact's Detail shows them, line
//! by line as it arrives.
//!
//! A screen opens a [`channel`]. Each tool run it starts is a [`Run`] of one log [`Key`]: a
//! review's, or an Artifact's own for the tools run from its Detail. Its output lines and its
//! end reach the channel as [`Event`]s, a launched program's for as long as it runs. A run keeps
//! draining its program's output whether or not anyone still receives; what nobody shows is
//! discarded. Nothing is saved.

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
};

use time::OffsetDateTime;
use tokio::sync::mpsc;

use crate::types::{ArtifactName, RequestId};

/// Whose Tool log a run belongs to.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Key {
    /// A Human review's: the tools its reviewer ran.
    Review(RequestId),
    /// An Artifact's own, in the workspace at `repo`: the tools run from its Detail, for no
    /// review.
    Artifact {
        repo: PathBuf,
        artifact: ArtifactName,
    },
}

impl Key {
    /// The review this log belongs to, if it is a review's.
    pub fn request(&self) -> Option<&RequestId> {
        match self {
            Self::Review(request) => Some(request),
            Self::Artifact { .. } => None,
        }
    }
}

impl From<&RequestId> for Key {
    fn from(request: &RequestId) -> Self {
        Self::Review(request.clone())
    }
}

/// Output bytes one run shows at most: a launched program may print for hours, and the log
/// is held in memory for as long as the review is open.
const RUN_BYTES: usize = 1024 * 1024;
/// A line longer than this is shown in pieces, so output without newlines still appears.
const LINE_BYTES: usize = 4096;
/// Shown once, as an error line, when a run printed more than the log keeps.
const DROPPED: &str = "[more output than the Tool log keeps; the rest is not shown]";

/// Numbers every run of this process, so lines arriving later find their own entry.
static NEXT_RUN: AtomicU64 = AtomicU64::new(1);

/// One tool run, unique in this process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RunNo(u64);

impl RunNo {
    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

/// How a run ended, as its last log line says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum End {
    /// The program exited: its exit code, or how it ended without one.
    Exited {
        code: Option<i32>,
        text: String,
    },
    /// Handed to the desktop's own opener; its output is not the tool's.
    Launched,
    /// A builtin finished.
    Done,
    Cancelled,
    TimedOut,
    /// The run could not start or failed, and why.
    Failed(String),
}

impl End {
    pub(crate) fn exited(status: &std::process::ExitStatus) -> Self {
        Self::Exited {
            code: match crate::platform::process_end(status) {
                crate::platform::ProcessEnd::Exited(code) => Some(code),
                crate::platform::ProcessEnd::Signaled(_) => None,
            },
            text: crate::platform::exit_description(status),
        }
    }

    /// The end line: `■ exit 0`, `■ launched`, `■ error: …`.
    pub fn text(&self) -> String {
        match self {
            Self::Exited {
                code: Some(code), ..
            } => format!("■ exit {code}"),
            Self::Exited { text, .. } => format!("■ {text}"),
            Self::Launched => "■ launched".into(),
            Self::Done => "■ done".into(),
            Self::Cancelled => "■ cancelled".into(),
            Self::TimedOut => "■ timed out".into(),
            Self::Failed(error) => format!("■ error: {error}"),
        }
    }

    /// The run did not end well.
    pub fn error(&self) -> bool {
        !matches!(
            self,
            Self::Exited { code: Some(0), .. } | Self::Launched | Self::Done
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Start {
        at: OffsetDateTime,
        operation: String,
        /// The Artifact that declares the tool.
        artifact: String,
    },
    Line {
        stream: Stream,
        text: String,
    },
    End(End),
}

/// One thing a run logged, for the log of `key`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub key: Key,
    pub run: RunNo,
    pub kind: Kind,
}

/// Where runs send their events; cloned into each job a review starts.
#[derive(Debug, Clone)]
pub struct Sink(mpsc::UnboundedSender<Event>);

/// The review's end of the log. Every run bounds its output, so the channel is unbounded.
pub type Events = mpsc::UnboundedReceiver<Event>;

pub fn channel() -> (Sink, Events) {
    let (sender, receiver) = mpsc::unbounded_channel();
    (Sink(sender), receiver)
}

impl Sink {
    /// Start a run of `operation` of `artifact` in the log of `key`: its start line is logged
    /// now.
    pub fn start(&self, key: impl Into<Key>, operation: &str, artifact: &str) -> Run {
        let run = Run::new(Some((self.clone(), key.into())));
        run.shared.send(Kind::Start {
            at: OffsetDateTime::now_utc(),
            operation: operation.to_owned(),
            artifact: artifact.to_owned(),
        });
        run
    }
}

struct Shared {
    /// Where the run's events go and the log they belong to; `None` when nobody watches.
    watch: Option<(Sink, Key)>,
    run: RunNo,
    /// Output bytes this run may still show.
    left: AtomicUsize,
    dropped: AtomicBool,
}

impl Shared {
    fn send(&self, kind: Kind) {
        if let Some((sink, key)) = &self.watch {
            // A closed review discards what its runs still print.
            let _ = sink.0.send(Event {
                key: key.clone(),
                run: self.run,
                kind,
            });
        }
    }

    fn line(&self, stream: Stream, bytes: &[u8]) {
        if !self.watched() {
            return;
        }
        let text =
            String::from_utf8(crate::runtime::clean_output(bytes)).expect("clean output is UTF-8");
        let size = text.len().max(1);
        // A compare-and-swap loop rather than `fetch_update`, which newer toolchains
        // deprecate under another name.
        let mut left = self.left.load(Ordering::Relaxed);
        let kept = loop {
            let Some(next) = left.checked_sub(size) else {
                break false;
            };
            match self
                .left
                .compare_exchange_weak(left, next, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => break true,
                Err(current) => left = current,
            }
        };
        if kept {
            self.send(Kind::Line { stream, text });
        } else if !self.dropped.swap(true, Ordering::Relaxed) {
            self.send(Kind::Line {
                stream: Stream::Stderr,
                text: DROPPED.into(),
            });
        }
    }
}

/// One tool run's log: lines while it runs, then exactly one end. A run dropped without an
/// end was cancelled.
pub struct Run {
    shared: Arc<Shared>,
    ended: bool,
}

impl std::fmt::Debug for Run {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Run")
            .field("run", &self.shared.run)
            .finish()
    }
}

impl Run {
    fn new(watch: Option<(Sink, Key)>) -> Self {
        Self {
            shared: Arc::new(Shared {
                watch,
                run: RunNo(NEXT_RUN.fetch_add(1, Ordering::Relaxed)),
                left: AtomicUsize::new(RUN_BYTES),
                dropped: AtomicBool::new(false),
            }),
            ended: false,
        }
    }

    /// A run nobody shows, such as `tool check`'s: launched programs get no pipes at all.
    pub fn unwatched() -> Self {
        Self::new(None)
    }

    pub(crate) fn watched(&self) -> bool {
        self.shared.watched()
    }

    /// A splitter of one output stream into this run's lines.
    pub(crate) fn lines(&self, stream: Stream) -> Lines {
        Lines {
            shared: self.shared.clone(),
            stream,
            pending: Vec::new(),
        }
    }

    /// Log `text`, a finished output, line by line.
    pub fn text(&self, stream: Stream, text: &str) {
        let mut lines = self.lines(stream);
        lines.push(text.as_bytes());
        lines.finish();
    }

    pub fn end(mut self, end: End) {
        self.ended = true;
        self.shared.send(Kind::End(end));
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        if !self.ended {
            self.shared.send(Kind::End(End::Cancelled));
        }
    }
}

/// Bytes of one stream cut into lines: at each newline, and in pieces of [`LINE_BYTES`].
pub(crate) struct Lines {
    shared: Arc<Shared>,
    stream: Stream,
    pending: Vec<u8>,
}

impl Lines {
    pub(crate) fn push(&mut self, bytes: &[u8]) {
        if !self.shared.watched() {
            return;
        }
        self.pending.extend_from_slice(bytes);
        while let Some(newline) = self.pending.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=newline).collect();
            let line = &line[..newline];
            self.shared
                .line(self.stream, line.strip_suffix(b"\r").unwrap_or(line));
        }
        while self.pending.len() >= LINE_BYTES {
            let end = piece(&self.pending);
            let piece: Vec<u8> = self.pending.drain(..end).collect();
            self.shared.line(self.stream, &piece);
        }
    }

    /// The stream closed: its last line, if it had no newline.
    pub(crate) fn finish(&mut self) {
        if !self.pending.is_empty() {
            let line = std::mem::take(&mut self.pending);
            self.shared.line(self.stream, &line);
        }
    }
}

impl Shared {
    fn watched(&self) -> bool {
        self.watch.is_some()
    }
}

/// Where a long line's first piece ends: [`LINE_BYTES`] in, moved back so that a character
/// cut in two stays whole for the next piece.
fn piece(bytes: &[u8]) -> usize {
    let end = LINE_BYTES.min(bytes.len());
    match std::str::from_utf8(&bytes[..end]) {
        Err(error) if error.error_len().is_none() && error.valid_up_to() > 0 => error.valid_up_to(),
        _ => end,
    }
}

/// The Tool log of one review as a screen shows it: one entry per run, in the order they
/// started, each with its output and, once known, its end.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolLog {
    entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub run: RunNo,
    pub at: OffsetDateTime,
    pub operation: String,
    pub artifact: String,
    pub lines: Vec<(Stream, String)>,
    pub end: Option<End>,
}

impl Entry {
    /// The start line, `▶ HH:MM:SS · operation · Artifact`, at `offset` from UTC.
    pub fn start_text(&self, offset: time::UtcOffset) -> String {
        start_text(
            self.at.to_offset(offset).time(),
            &self.operation,
            &self.artifact,
        )
    }
}

/// `▶ HH:MM:SS · operation · Artifact`.
pub fn start_text(at: time::Time, operation: &str, artifact: &str) -> String {
    format!(
        "▶ {:02}:{:02}:{:02} · {operation} · {artifact}",
        at.hour(),
        at.minute(),
        at.second()
    )
}

impl ToolLog {
    /// Take `event` in: a start opens an entry; lines and the end join their run's entry.
    /// Lines of a run this log never saw start (it was cleared since) are dropped.
    pub fn apply(&mut self, event: Event) {
        match event.kind {
            Kind::Start {
                at,
                operation,
                artifact,
            } => self.entries.push(Entry {
                run: event.run,
                at,
                operation,
                artifact,
                lines: Vec::new(),
                end: None,
            }),
            kind => {
                let Some(entry) = self.entries.iter_mut().find(|entry| entry.run == event.run)
                else {
                    return;
                };
                match kind {
                    Kind::Line { stream, text } => entry.lines.push((stream, text)),
                    Kind::End(end) => entry.end = Some(end),
                    Kind::Start { .. } => unreachable!("handled above"),
                }
            }
        }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> RequestId {
        "req-1".parse().unwrap()
    }

    fn drain(events: &mut Events) -> Vec<Kind> {
        std::iter::from_fn(|| events.try_recv().ok())
            .map(|event| event.kind)
            .collect()
    }

    fn line(stream: Stream, text: &str) -> Kind {
        Kind::Line {
            stream,
            text: text.into(),
        }
    }

    #[test]
    fn output_arrives_as_lines_of_its_own_stream() {
        let (sink, mut events) = channel();
        let run = sink.start(&request(), "open", "notes");
        let mut out = run.lines(Stream::Stdout);
        let mut err = run.lines(Stream::Stderr);
        out.push(b"one\r\ntw");
        err.push(b"warn\n");
        out.push(b"o\nthree");
        out.finish();
        err.finish();
        run.end(End::Exited {
            code: Some(0),
            text: String::new(),
        });
        let kinds = drain(&mut events);
        assert!(matches!(&kinds[0], Kind::Start { operation, artifact, .. }
            if operation == "open" && artifact == "notes"));
        assert_eq!(
            kinds[1..],
            [
                line(Stream::Stdout, "one"),
                line(Stream::Stderr, "warn"),
                line(Stream::Stdout, "two"),
                line(Stream::Stdout, "three"),
                Kind::End(End::Exited {
                    code: Some(0),
                    text: String::new()
                }),
            ]
        );
    }

    #[test]
    fn a_line_without_newline_shows_in_pieces_that_keep_characters_whole() {
        let (sink, mut events) = channel();
        let run = sink.start(&request(), "dump", "app");
        let mut out = run.lines(Stream::Stdout);
        let mut text = "a".repeat(LINE_BYTES - 1);
        text.push('é');
        out.push(text.as_bytes());
        let kinds = drain(&mut events);
        assert_eq!(
            kinds[1..],
            [line(Stream::Stdout, &"a".repeat(LINE_BYTES - 1))]
        );
        out.finish();
        assert_eq!(drain(&mut events), [line(Stream::Stdout, "é")]);
    }

    #[test]
    fn a_run_shows_its_output_up_to_its_budget_then_says_the_rest_is_not_shown() {
        let (sink, mut events) = channel();
        let run = sink.start(&request(), "spew", "app");
        let chunk = format!("{}\n", "x".repeat(LINE_BYTES - 1));
        for _ in 0..(RUN_BYTES / (LINE_BYTES - 1) + 10) {
            run.lines(Stream::Stdout).push(chunk.as_bytes());
        }
        let kinds = drain(&mut events);
        let shown: usize = kinds
            .iter()
            .filter_map(|kind| match kind {
                Kind::Line {
                    stream: Stream::Stdout,
                    text,
                } => Some(text.len()),
                _ => None,
            })
            .sum();
        assert!(shown <= RUN_BYTES);
        assert_eq!(kinds.last(), Some(&line(Stream::Stderr, DROPPED)));
        assert_eq!(
            kinds
                .iter()
                .filter(|kind| **kind == line(Stream::Stderr, DROPPED))
                .count(),
            1
        );
    }

    #[test]
    fn a_run_dropped_without_an_end_was_cancelled_and_a_closed_review_discards_lines() {
        let (sink, mut events) = channel();
        drop(sink.start(&request(), "slow", "app"));
        assert_eq!(drain(&mut events).last(), Some(&Kind::End(End::Cancelled)));
        let run = sink.start(&request(), "late", "app");
        drop(events);
        run.text(Stream::Stdout, "nobody reads this\n");
        run.end(End::Done);
    }

    #[test]
    fn the_log_groups_lines_under_their_run_and_ignores_runs_it_never_saw_start() {
        let (sink, mut events) = channel();
        let first = sink.start(&request(), "launch", "app");
        let second = sink.start(&request(), "check", "lib");
        second.text(Stream::Stdout, "checked\n");
        second.end(End::Done);
        first.text(Stream::Stderr, "app says\n");
        let mut log = ToolLog::default();
        for event in std::iter::from_fn(|| events.try_recv().ok()) {
            log.apply(event);
        }
        let [launch, check] = log.entries() else {
            panic!("two entries: {log:?}");
        };
        assert_eq!(launch.lines, [(Stream::Stderr, "app says".to_owned())]);
        assert_eq!(launch.end, None);
        assert_eq!(check.lines, [(Stream::Stdout, "checked".to_owned())]);
        assert_eq!(check.end, Some(End::Done));
        log.clear();
        first.end(End::Done);
        log.apply(events.try_recv().unwrap());
        assert!(log.is_empty());
    }

    #[test]
    fn end_lines_say_how_a_run_ended() {
        let exit = |code| End::Exited {
            code: Some(code),
            text: String::new(),
        };
        assert_eq!(exit(0).text(), "■ exit 0");
        assert!(!exit(0).error());
        assert_eq!(exit(2).text(), "■ exit 2");
        assert!(exit(2).error());
        assert_eq!(End::Launched.text(), "■ launched");
        assert_eq!(End::TimedOut.text(), "■ timed out");
        assert_eq!(End::Failed("no".into()).text(), "■ error: no");
        assert!(End::Cancelled.error());
        assert_eq!(
            start_text(time::Time::from_hms(9, 5, 7).unwrap(), "open", "notes"),
            "▶ 09:05:07 · open · notes"
        );
    }
}
