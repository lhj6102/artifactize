//! Saved Agent conversations: one append-only JSON Lines file per review session, which
//! `session show` reads and `session send` continues.
//!
//! `$STATE/agent-sessions/` (owner-only) holds `<sessionId>.jsonl`, an owner-only file of
//! one JSON event per line, and `<sessionId>.lock`, which serializes the sends to it:
//!
//! - `review`, first: the session, Run, request and eval ids, the producer and state id,
//!   the backend, model, reasoning, provider parameters, budgets and tool definitions;
//! - `message`: one rig [`Message`] exactly as the provider received or sent it, with the
//!   `turn` (the provider request) that carried it. The system prompt and instructions come
//!   first, then each answer with its reasoning (encrypted reasoning items and thinking
//!   signatures included), tool calls and their results. A message of tool results has
//!   `isError`, whether each result failed, in order. The repair prompt has `repair`, and
//!   the answer it replaced is the message before it;
//! - `attempt`: one provider attempt of a `turn`, its `usage` counters and any error;
//! - `end`: the review's `result`, or its `errorCode` and `error`;
//! - `send` and `answer` around each follow-up, whose events carry its `send` number.
//!
//! Every event has its time in `at`. Writing never fails a review: an error is reported on
//! stderr and the review goes on without its conversation.

pub mod document;
mod events;
mod gc;
pub mod live;
mod outcome;
mod pages;
#[cfg(test)]
mod pages_tests;
pub mod transcript;
#[cfg(test)]
mod transcript_tests;
pub use events::{
    AcpPermissionEvent, AcpPlanEvent, AcpToolCallEvent, AcpToolUpdateEvent, Answer, Budgets,
    Delivery, DeliveryKind, DeliveryState, End, Event, HandoffEvent, Header, Kind, MessageEvent,
    Send,
};
#[cfg(test)]
mod recorder_tests;
mod summary;

pub use gc::{Collection, Usage, collect, usage};
pub use summary::{Summary, tokens_text};

use std::{
    fmt,
    fs::File,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
};

use rig_core::message::{AssistantContent, Message, ToolResultContent, UserContent};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use serde_json::Value;
#[cfg(test)]
use serde_json::json;

use crate::{platform, store::Request};

/// The session store below the state directory.
pub const DIRECTORY: &str = "agent-sessions";
/// The version of the session file's events.
const VERSION: u32 = 1;

/// Where an Agent review's conversation lives: the producing `user@host`, the state (by its
/// stable id), and the Run, request and session ids. Its text form is
/// `user@host/<state>/<runId>/<requestId>/<sessionId>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRef {
    pub producer: crate::types::ProducerId,
    pub state: crate::types::StateId,
    pub run_id: crate::types::RunId,
    pub request_id: crate::types::RequestId,
    pub session_id: crate::types::SessionId,
}

impl fmt::Display for SessionRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}/{}/{}/{}/{}",
            self.producer, self.state, self.run_id, self.request_id, self.session_id
        )
    }
}

impl SessionRef {
    /// The text form; `None` for anything else, such as a bare request or session id.
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.rsplitn(5, '/');
        let (session_id, request_id, run_id, state, producer) = (
            parts.next()?,
            parts.next()?,
            parts.next()?,
            parts.next()?,
            parts.next()?,
        );
        let reference = Self {
            producer: producer.parse().ok()?,
            state: state.parse().ok()?,
            run_id: run_id.parse().ok()?,
            request_id: request_id.parse().ok()?,
            session_id: session_id.parse().ok()?,
        };
        reference.valid().then_some(reference)
    }

    /// Bounded, printable fields; the ids are single path segments. A reference from a remote
    /// record is checked before it is kept.
    pub fn valid(&self) -> bool {
        let printable = |value: &str| {
            (1..=crate::types::MAX_ID_BYTES).contains(&value.len())
                && !value.chars().any(char::is_control)
        };
        [
            self.state.as_str(),
            self.run_id.as_str(),
            self.request_id.as_str(),
            self.session_id.as_str(),
        ]
        .into_iter()
        .all(|id| printable(id) && valid_id(id))
    }
}

/// A session or state id: letters, digits, `-`, `_` and `.`, never only dots.
fn valid_id(id: &str) -> bool {
    id.parse::<crate::types::SessionId>().is_ok()
}

/// The session store of a state.
pub fn directory(state: &Path) -> PathBuf {
    state.join(DIRECTORY)
}

/// The conversation file of a session id.
pub fn path(state: &Path, id: &crate::types::SessionId) -> PathBuf {
    directory(state).join(format!("{id}.jsonl"))
}

/// Where new conversations are saved, and as which producer and state; `None` when saving is
/// off (`agentSessions.enabled: false` in `limits.json`).
#[derive(Debug, Clone)]
pub struct Saving {
    pub state: PathBuf,
    pub state_id: crate::types::StateId,
    pub producer: crate::types::ProducerId,
}

fn warn(message: &str) {
    let _ = writeln!(std::io::stderr().lock(), "{message}");
}

/// Appends one session's events. It writes nothing when saving is off, and after a failed
/// write it reports the error once and writes nothing more.
pub struct Recorder {
    id: crate::types::SessionId,
    state: Recording,
    publisher: crate::changes::Publisher,
}

/// Only a successfully flushed review owns a saved reference. Follow-ups have a send number
/// instead; a later write failure retains the review's reference but drops its file.
enum Recording {
    Off,
    Pending(Box<Pending>),
    Review { file: File, reference: SessionRef },
    FollowUp { file: File, send: usize },
    Failed,
    SavedFailure(SessionRef),
}
struct Pending {
    path: PathBuf,
    reference: SessionRef,
    eval_id: Option<crate::types::EvalId>,
    target: Option<crate::types::ArtifactName>,
}

impl Recorder {
    /// A session that is not saved.
    pub fn off(id: &crate::types::SessionId) -> Self {
        Self {
            id: id.clone(),
            state: Recording::Off,
            publisher: crate::changes::Publisher::default(),
        }
    }

    /// The recorder of a request's new review session `id`. No file is created until start.
    pub fn new(saving: Option<&Saving>, request: &Request, id: &crate::types::SessionId) -> Self {
        let Some(saving) = saving else {
            return Self::off(id);
        };
        let path = path(&saving.state, id);
        Self {
            id: id.clone(),
            state: Recording::Pending(Box::new(Pending {
                path,
                reference: SessionRef {
                    producer: saving.producer.clone(),
                    state: saving.state_id.clone(),
                    run_id: request.run_id.clone(),
                    request_id: request.id.clone(),
                    session_id: id.clone(),
                },
                eval_id: Some(request.eval_id.clone()),
                target: Some(request.target.clone()),
            })),
            publisher: crate::changes::Publisher::new(&saving.state),
        }
    }

    /// A session saved below `state` for no Run.
    #[cfg(test)]
    pub(crate) fn test(state: &Path, id: &str) -> Self {
        let id: crate::types::SessionId = id.parse().unwrap();
        Self {
            state: Recording::Pending(Box::new(Pending {
                path: path(state, &id),
                reference: SessionRef {
                    producer: "tester@host".parse().unwrap(),
                    state: "state".parse().unwrap(),
                    run_id: "run".parse().unwrap(),
                    request_id: "request".parse().unwrap(),
                    session_id: id.clone(),
                },
                eval_id: None,
                target: None,
            })),
            ..Self::off(&id)
        }
    }

    /// Append follow-up `send` to the saved conversation at `path` without a new review header.
    pub(crate) fn append(
        state: &Path,
        path: &Path,
        id: &crate::types::SessionId,
        send: usize,
    ) -> Result<Self, String> {
        let file = platform::open_no_follow(File::options().append(true), path)
            .map_err(|error| format!("Cannot open Agent session {id}: {error}"))?;
        Ok(Self {
            id: id.clone(),
            state: Recording::FollowUp { file, send },
            publisher: crate::changes::Publisher::new(state),
        })
    }

    /// A Chat uses the same event stream and append framing as a review follow-up. Its
    /// independent directory is not eligible for review session GC.
    pub(crate) fn chat(
        state: &Path,
        path: &Path,
        id: &crate::types::SessionId,
        send: usize,
    ) -> Result<Self, String> {
        use std::io::{Read as _, Seek as _, SeekFrom};
        match platform::open_no_follow(std::fs::File::options().read(true).append(true), path) {
            Ok(mut file) => {
                let end = file
                    .seek(SeekFrom::End(0))
                    .map_err(|error| error.to_string())?;
                if end > 0 {
                    file.seek(SeekFrom::End(-1))
                        .map_err(|error| error.to_string())?;
                    let mut last = [0];
                    file.read_exact(&mut last)
                        .map_err(|error| error.to_string())?;
                    if last != *b"\n" {
                        file.seek(SeekFrom::Start(0))
                            .map_err(|error| error.to_string())?;
                        let mut bytes = Vec::new();
                        file.read_to_end(&mut bytes)
                            .map_err(|error| error.to_string())?;
                        let len = bytes
                            .iter()
                            .rposition(|&byte| byte == b'\n')
                            .map_or(0, |at| at + 1);
                        file.set_len(len as u64)
                            .map_err(|error| error.to_string())?;
                    }
                }
                Ok(Self {
                    id: id.clone(),
                    state: Recording::FollowUp { file, send },
                    publisher: crate::changes::Publisher::new(state),
                })
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let mut file = create(path)?;
                write_event(
                    &mut file,
                    None,
                    Kind::Review(Box::new(Header {
                        version: Some(VERSION),
                        session_id: Some(id.clone()),
                        ..Default::default()
                    })),
                )
                .map_err(|error| error.to_string())?;
                Ok(Self {
                    id: id.clone(),
                    state: Recording::FollowUp { file, send },
                    publisher: crate::changes::Publisher::new(state),
                })
            }
            Err(error) => Err(error.to_string()),
        }
    }

    pub(crate) fn check_saved(&self) -> Result<(), String> {
        if self.active() {
            Ok(())
        } else {
            Err("Could not save the Chat transcript.".into())
        }
    }

    pub fn id(&self) -> &crate::types::SessionId {
        &self.id
    }

    /// The saved review's reference, after its header flush, even if a later append fails.
    pub fn reference(&self) -> Option<&SessionRef> {
        match &self.state {
            Recording::Review { reference, .. } | Recording::SavedFailure(reference) => {
                Some(reference)
            }
            Recording::Off
            | Recording::Pending(_)
            | Recording::FollowUp { .. }
            | Recording::Failed => None,
        }
    }

    /// Create the session file with its `review` event: the identity, then `details`.
    pub fn start(&mut self, mut details: Header) {
        let state = std::mem::replace(&mut self.state, Recording::Off);
        let pending = match state {
            Recording::Pending(pending) => pending,
            other => {
                self.state = other;
                return;
            }
        };
        let reference = pending.reference;
        details.version = Some(VERSION);
        details.producer = Some(reference.producer.clone());
        details.state = Some(reference.state.clone());
        details.run_id = Some(reference.run_id.clone());
        details.request_id = Some(reference.request_id.clone());
        details.session_id = Some(reference.session_id.clone());
        details.eval_id = pending.eval_id;
        details.target = pending.target;
        match create(&pending.path) {
            Ok(file) => self.start_file(file, reference, details),
            Err(error) => self.fail(&error),
        }
    }

    /// Promote the reference only after the initial header has been flushed.
    fn start_file(&mut self, mut file: File, reference: SessionRef, details: Header) {
        match write_event(&mut file, None, Kind::Review(Box::new(details))) {
            Ok(()) => {
                self.state = Recording::Review { file, reference };
                self.notify();
            }
            Err(error) => self.fail(&error.to_string()),
        }
    }

    fn active(&self) -> bool {
        matches!(
            self.state,
            Recording::Review { .. } | Recording::FollowUp { .. }
        )
    }

    /// One message of `turn`; `repair` marks the prompt that asked for the verdict again.
    pub(crate) fn message(&mut self, turn: usize, message: &Message, repair: bool) {
        if !self.active() {
            return;
        }
        self.event(Kind::Message(MessageEvent::new(
            turn,
            message.clone(),
            repair,
            Vec::new(),
            None,
        )));
    }

    /// The message of tool results that `turn` sends, with whether each failed.
    pub(crate) fn tool_results(&mut self, turn: usize, message: &Message, failed: &[bool]) {
        if !self.active() {
            return;
        }
        self.event(Kind::Message(MessageEvent::new(
            turn,
            message.clone(),
            false,
            failed.to_vec(),
            None,
        )));
    }

    /// Append an event, stamped with its time and the follow-up it belongs to. End events do
    /// not close the writer; the caller owns its lifetime and session GC scheduling.
    pub fn event(&mut self, kind: Kind) {
        let result = match &mut self.state {
            Recording::Review { file, .. } => write_event(file, None, kind),
            Recording::FollowUp { file, send } => write_event(file, Some(*send), kind),
            Recording::Off
            | Recording::Pending(_)
            | Recording::Failed
            | Recording::SavedFailure(_) => return,
        };
        match result {
            Ok(()) => self.notify(),
            Err(error) => self.fail(&error.to_string()),
        }
    }

    pub(crate) fn notify(&self) {
        self.publisher
            .notify(crate::changes::Change::SessionInvalidated(self.id.clone()));
    }

    fn fail(&mut self, error: &str) {
        self.state = match std::mem::replace(&mut self.state, Recording::Failed) {
            Recording::Review { reference, .. } | Recording::SavedFailure(reference) => {
                Recording::SavedFailure(reference)
            }
            Recording::Off
            | Recording::Pending(_)
            | Recording::FollowUp { .. }
            | Recording::Failed => Recording::Failed,
        };
        warn(&format!(
            "Cannot save Agent session {}: {error}; the review goes on without saving it.",
            self.id
        ));
    }
}

/// A successful flush is the single write boundary for both headers and later events.
fn write_event(file: &mut File, send: Option<usize>, kind: Kind) -> std::io::Result<()> {
    let event = Event {
        kind,
        send,
        at: Some(crate::broker::now()),
    };
    let mut line = serde_json::to_vec(&event).expect("session events are JSON");
    line.push(b'\n');
    file.write_all(&line).and_then(|()| file.flush())
}

/// A new owner-only session file in an owner-only session store.
fn create(path: &Path) -> Result<File, String> {
    let directory = path.parent().expect("a session file has a directory");
    platform::create_private_dir_all(directory).map_err(|e| e.to_string())?;
    if !platform::is_private_dir(directory).map_err(|e| e.to_string())? {
        return Err(format!(
            "{} must have owner-only permissions.",
            crate::platform::path_text(directory)
        ));
    }
    let file = platform::open_no_follow(
        platform::private_options().append(true).create_new(true),
        path,
    )
    .map_err(|e| e.to_string())?;
    platform::restrict_file(&file).map_err(|e| e.to_string())?;
    Ok(file)
}

/// A saved conversation, read whole.
#[derive(Debug)]
pub struct Conversation {
    pub path: PathBuf,
    header: Box<Header>,
    header_at: Option<crate::types::Timestamp>,
    events: Vec<Event>,
    /// Original events for the lossless --json output; computations use events.
    original: Vec<Box<serde_json::value::RawValue>>,
}

impl Conversation {
    /// Read a session file; `None` when it does not exist. A last line cut short by a crash
    /// is left out.
    pub fn load(path: &Path) -> Result<Option<Self>, String> {
        let file = match platform::open_no_follow(File::options().read(true), path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(format!(
                    "Cannot read {}: {error}",
                    crate::platform::path_text(path)
                ));
            }
        };
        if !file.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err(format!(
                "{} is not a regular file.",
                crate::platform::path_text(path)
            ));
        }
        let mut reader = BufReader::new(file);
        let mut events = Vec::new();
        let mut original = Vec::new();
        for number in 1.. {
            let mut line = String::new();
            if reader
                .read_line(&mut line)
                .map_err(|error| format!("Cannot read {}: {error}", platform::path_text(path)))?
                == 0
            {
                break;
            }
            match Event::parse(line.as_bytes()) {
                Ok(event) => {
                    let json = serde_json::value::RawValue::from_string(line)
                        .expect("a decoded event is valid JSON");
                    original.push(json);
                    events.push(event);
                }
                Err(error) if error.is_eof() && !line.ends_with('\n') => break,
                Err(error) => {
                    return Err(format!(
                        "{} line {number} is not a session event: {error}",
                        platform::path_text(path)
                    ));
                }
            }
        }
        Self::from_events(path, events, original).map(Some)
    }

    fn from_events(
        path: &Path,
        events: Vec<Event>,
        original: Vec<Box<serde_json::value::RawValue>>,
    ) -> Result<Self, String> {
        let mut events = events.into_iter();
        let Some(Event {
            at: header_at,
            kind: Kind::Review(header),
            ..
        }) = events.next()
        else {
            return Err(format!(
                "{} has no review event.",
                crate::platform::path_text(path)
            ));
        };
        Ok(Self {
            path: path.into(),
            header,
            header_at,
            events: events.collect(),
            original,
        })
    }

    /// The required `review` header.
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// Events after the review header, in file order.
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// Original records, including the header, in file order.
    pub(crate) fn into_original(self) -> Vec<Box<serde_json::value::RawValue>> {
        self.original
    }

    #[cfg(test)]
    pub(crate) fn wire_events(&self) -> Vec<Value> {
        self.original
            .iter()
            .map(|event| serde_json::from_str(event.get()).unwrap())
            .collect()
    }

    /// How many follow-ups were sent.
    pub fn sends(&self) -> usize {
        self.events
            .iter()
            .filter(|event| matches!(event.kind, Kind::Send(_)))
            .count()
    }

    /// Every message in order, the review's and its follow-ups', ready to send again. Tool
    /// calls anywhere in the history whose turn stopped before answering get an error result
    /// right after that assistant message, preserving any already-recorded `ToolOutcome` results
    /// so switching from a stopped or interrupted turn to ACP and later back to Direct API never
    /// drops saved tool outcomes or sends unanswered `ToolCall`s to Anthropic/OpenAI.
    pub fn history(&self) -> Result<Vec<Message>, String> {
        enum HistoryEntry {
            Message(Message),
            ToolOutcome(Message),
        }

        let mut raw = Vec::new();
        let mut saw_assistant_text_in_turn = false;
        for event in &self.events {
            match &event.kind {
                Kind::Send(_) => {
                    saw_assistant_text_in_turn = false;
                }
                Kind::Message(event) => {
                    let msg = event.message().clone();
                    if let Message::Assistant { content, .. } = &msg
                        && content.iter().any(|part| {
                            matches!(part, AssistantContent::Text(text) if !text.text.trim().is_empty())
                        })
                    {
                        saw_assistant_text_in_turn = true;
                    }
                    raw.push(HistoryEntry::Message(msg));
                }
                Kind::ToolOutcome(event) => {
                    raw.push(HistoryEntry::ToolOutcome(event.message().clone()));
                }
                Kind::Answer(Answer::Completed(text))
                    if !saw_assistant_text_in_turn && !text.trim().is_empty() =>
                {
                    // Preserve a completed turn's final prose in Direct history even if earlier
                    // assistant messages held only tool calls or ACP hit the per-turn display cap.
                    saw_assistant_text_in_turn = true;
                    raw.push(HistoryEntry::Message(Message::assistant(text.clone())));
                }
                _ => {}
            }
        }
        let mut normalized = Vec::with_capacity(raw.len());
        let mut index = 0;
        while index < raw.len() {
            let entry = &raw[index];
            let message = match entry {
                HistoryEntry::Message(msg) => msg.clone(),
                HistoryEntry::ToolOutcome(_) => {
                    index += 1;
                    continue;
                }
            };
            match &message {
                Message::Assistant { content, .. } => {
                    let calls: Vec<_> = content
                        .iter()
                        .filter_map(|part| match part {
                            AssistantContent::ToolCall(call) => Some(call.clone()),
                            _ => None,
                        })
                        .collect();
                    normalized.push(message);
                    index += 1;
                    if calls.is_empty() {
                        continue;
                    }
                    // Collect any immediately following user tool-result messages and individual
                    // `ToolOutcome` events answering this assistant turn. Combined `Message`
                    // results take precedence; individual `ToolOutcome` results recover real tool
                    // outputs if the turn was interrupted before writing the combined message.
                    let mut answered = std::collections::BTreeSet::new();
                    let mut outcome_results: Vec<UserContent> = Vec::new();
                    let mut last_tool_result_pos = None;
                    while index < raw.len() {
                        match &raw[index] {
                            HistoryEntry::ToolOutcome(Message::User { content }) => {
                                for part in content {
                                    if matches!(part, UserContent::ToolResult(_)) {
                                        outcome_results.push(part.clone());
                                    }
                                }
                                index += 1;
                            }
                            HistoryEntry::Message(msg @ Message::User { content })
                                if content
                                    .iter()
                                    .any(|part| matches!(part, UserContent::ToolResult(_))) =>
                            {
                                for part in content {
                                    if let UserContent::ToolResult(result) = part {
                                        answered.insert(result.call.wire().into_owned());
                                    }
                                }
                                last_tool_result_pos = Some(normalized.len());
                                normalized.push(msg.clone());
                                index += 1;
                            }
                            _ => break,
                        }
                    }
                    let mut recovered: Vec<UserContent> = Vec::new();
                    for part in outcome_results {
                        if let UserContent::ToolResult(ref result) = part {
                            let wire_id = result.call.wire().into_owned();
                            if answered.insert(wire_id) {
                                recovered.push(part);
                            }
                        }
                    }
                    let missing: Vec<_> = calls
                        .iter()
                        .filter(|call| !answered.contains(call.id.wire().as_ref()))
                        .map(|call| {
                            UserContent::ToolResult(call.result(vec![ToolResultContent::text(
                                "Not run: the review stopped before this tool call completed.",
                            )]))
                        })
                        .collect();
                    recovered.extend(missing);
                    if !recovered.is_empty() {
                        if let Some(pos) = last_tool_result_pos
                            && let Some(Message::User { content }) = normalized.get_mut(pos)
                        {
                            content.extend(recovered);
                        } else {
                            normalized.push(Message::User { content: recovered });
                        }
                    }
                }
                _ => {
                    normalized.push(message);
                    index += 1;
                }
            }
        }
        Ok(normalized)
    }
}

/// Hold `<sessionId>.lock` until the returned file drops: sends to one session take turns.
pub async fn lock(state: &Path, id: &crate::types::SessionId) -> Result<File, String> {
    let path = path(state, id).with_extension("lock");
    let file = platform::open_no_follow(
        platform::private_options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false),
        &path,
    )
    .map_err(|e| format!("Cannot lock Agent session {id}: {e}"))?;
    loop {
        match crate::platform::paths::try_lock(&file) {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) => {
                tokio::time::sleep(platform::FILE_LOCK_RETRY_INTERVAL).await;
            }
            Err(std::fs::TryLockError::Error(error)) => {
                return Err(format!("Cannot lock Agent session {id}: {error}"));
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod live_tests;
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn reference() -> SessionRef {
        SessionRef {
            producer: "alice@laptop".parse().unwrap(),
            state: "6f1c0a52-8d1e-4c43-9a55-31f6a7f2d0e4".parse().unwrap(),
            run_id: "run-Hq2b9X".parse().unwrap(),
            request_id: "run-Hq2b9X-3".parse().unwrap(),
            session_id: "0e8c7c2e-2a49-4b8e-9f7a-5d7a0c3b1f20".parse().unwrap(),
        }
    }

    #[test]
    fn producer_identity_validates_at_the_session_wire_boundary() {
        let valid = serde_json::to_value(reference()).unwrap();
        for producer in [
            String::new(),
            "bad\nproducer".into(),
            "x".repeat(201),
            "é".repeat(101),
        ] {
            let mut wire = valid.clone();
            wire["producer"] = json!(producer);
            assert!(serde_json::from_value::<SessionRef>(wire).is_err());
        }
        for producer in [
            "name/with/slash@host".to_owned(),
            "x".repeat(200),
            "é".repeat(100),
        ] {
            let mut wire = valid.clone();
            wire["producer"] = json!(producer);
            let reference: SessionRef = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(serde_json::to_value(reference).unwrap(), wire);
        }
    }

    #[test]
    fn references_round_trip_through_their_text_form() {
        let reference = reference();
        let text = reference.to_string();
        assert_eq!(
            text,
            "alice@laptop/6f1c0a52-8d1e-4c43-9a55-31f6a7f2d0e4/run-Hq2b9X/run-Hq2b9X-3/0e8c7c2e-2a49-4b8e-9f7a-5d7a0c3b1f20"
        );
        assert_eq!(SessionRef::parse(&text), Some(reference));
        // A producer may hold a slash; the ids never do.
        let odd = SessionRef {
            producer: "a/b@host".parse().unwrap(),
            ..self::reference()
        };
        assert_eq!(SessionRef::parse(&odd.to_string()), Some(odd));
        for text in [
            "run-Hq2b9X-3",
            "0e8c7c2e-2a49-4b8e-9f7a-5d7a0c3b1f20",
            "a/b/c/d",
            "alice@laptop/state/run/../session",
            "alice@laptop/state/run/request/",
            "alice@laptop/st ate/run/request/session",
        ] {
            assert_eq!(SessionRef::parse(text), None, "{text}");
        }
        assert!("..".parse::<crate::types::SessionId>().is_err());
        assert!("a/b".parse::<crate::types::SessionId>().is_err());
    }

    #[test]
    fn calls_a_stopped_review_never_answered_are_answered_on_replay() {
        use rig_core::message::{ToolCall, ToolFunction, ToolName};
        let call = |id: &str| {
            ToolCall::from_wire(
                id,
                ToolFunction::new(ToolName::new("read").unwrap(), json!({})),
            )
        };
        let answer = Message::Assistant {
            id: None,
            content: vec![
                AssistantContent::ToolCall(call("c1")),
                AssistantContent::ToolCall(call("c2")),
            ],
        };
        let ran = Message::tool_results(vec![
            call("c1").result(vec![ToolResultContent::text("ran")]),
        ]);
        let root = crate::test_os::tempdir();
        let replay = |messages: &[&Message]| {
            let events: Vec<Value> = std::iter::once(json!({"kind":"review"}))
                .chain(
                    messages
                        .iter()
                        .map(|message| json!({"kind":"message","message":message})),
                )
                .collect();
            let path = root.path().join("session.jsonl");
            let text = events
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n");
            std::fs::write(&path, text).unwrap();
            Conversation::load(&path)
                .unwrap()
                .unwrap()
                .history()
                .unwrap()
        };
        let results = |message: &Message| match message {
            Message::User { content } => content
                .iter()
                .map(|part| match part {
                    UserContent::ToolResult(result) => {
                        (result.call.wire().into_owned(), json!(result.content))
                    }
                    _ => panic!("only tool results"),
                })
                .collect::<Vec<_>>(),
            _ => panic!("a user message"),
        };
        let not_run = json!([
            {"type":"text","text":"Not run: the review stopped before this tool call completed."},
        ]);
        // The budget stopped the second call: it is answered next to the first one's result.
        let history = replay(&[&answer, &ran]);
        assert_eq!(history.len(), 2);
        assert_eq!(
            results(&history[1]),
            [
                ("c1".to_owned(), json!([{"type":"text","text":"ran"}])),
                ("c2".to_owned(), not_run.clone())
            ]
        );
        // No call ran: every call is answered in a new message.
        let history = replay(&[&answer]);
        assert_eq!(history.len(), 2);
        assert_eq!(
            results(&history[1]),
            [
                ("c1".to_owned(), not_run.clone()),
                ("c2".to_owned(), not_run)
            ]
        );
    }

    #[test]
    fn a_session_that_cannot_be_written_is_skipped() {
        let root = crate::test_os::tempdir();
        // The store's place is taken by a file, so the session cannot be created.
        let state = root.path().join("state");
        std::fs::create_dir(&state).unwrap();
        std::fs::write(directory(&state), "not a directory").unwrap();
        let reference = reference();
        let request = crate::monitor::tests::request(
            "app/check",
            "RUNNING",
            json!({"profile":{"kind":"agent","backend":"openai","model":"fixture"}}),
        );
        let saving = Saving {
            state: state.clone(),
            state_id: reference.state,
            producer: reference.producer,
        };
        let mut recorder = Recorder::new(Some(&saving), &request.request, &reference.session_id);
        recorder.start(Header {
            backend: Some(crate::config::Provider::Openai.into()),
            ..Header::default()
        });
        recorder.message(1, &Message::user("still recorded nowhere"), false);
        recorder.event(Kind::End(End::default()));
        assert!(recorder.reference().is_none());
        assert!(std::fs::metadata(directory(&state)).unwrap().is_file());
    }
    #[test]
    fn jsonl_edges_keep_wire_events_and_reject_invalid_complete_records() {
        let root = crate::test_os::tempdir();
        let file = root.path().join("legacy.jsonl");
        let header = r#"{"kind":"review","version":1,"sessionId":"legacy.session","runId":"run-old","requestId":"run-old-1","backend":"openai","model":"fixture","reasoning":null,"parameters":{"vendor":true},"budgets":{"timeoutMs":1000,"maxToolCalls":null,"maxTokens":null},"tools":[],"unknownSavedField":7,"at":"2026-01-01T00:00:00Z"}"#;
        let message = r#"{"kind":"message","turn":1,"message":{"role":"user","content":[{"type":"text","text":"hello"}]},"at":"2026-01-01T00:00:01Z"}"#;
        std::fs::write(&file, format!("{header}\n{message}\n{{\"kind\":")).unwrap();
        let conversation = Conversation::load(&file).unwrap().unwrap();
        assert_eq!(conversation.events().len(), 1);
        assert_eq!(
            conversation.header().session_id.as_ref().unwrap().as_str(),
            "legacy.session"
        );
        assert_eq!(
            conversation.header().budgets.as_ref().unwrap().timeout_ms,
            Some(Duration::from_millis(1000))
        );
        assert_eq!(conversation.original[0].get(), header);
        assert_eq!(conversation.original[1].get(), message);
        assert_eq!(conversation.history().unwrap().len(), 1);
        for invalid in [
            r#"{"kind":"message","message":17}"#,
            r#"{"kind":"attempt","turn":"bad","attempt":1,"usage":{}}"#,
            r#"{"kind":"unknown"}"#,
        ] {
            std::fs::write(&file, format!("{header}\n{invalid}\n")).unwrap();
            assert!(Conversation::load(&file).is_err(), "{invalid}");
        }
        std::fs::write(&file, header.replace("legacy.session", "../escape")).unwrap();
        assert!(Conversation::load(&file).is_err());
    }
}
