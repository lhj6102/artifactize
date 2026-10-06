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

mod gc;
mod summary;

pub use gc::{Collection, Usage, collect, usage};
pub use summary::{Summary, tokens_text};

use std::{
    fmt,
    fs::File,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use rig_core::message::{AssistantContent, Message, ToolResultContent, UserContent};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

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
    pub producer: String,
    pub state: String,
    pub run_id: String,
    pub request_id: String,
    pub session_id: String,
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
            producer: producer.into(),
            state: state.into(),
            run_id: run_id.into(),
            request_id: request_id.into(),
            session_id: session_id.into(),
        };
        reference.valid().then_some(reference)
    }

    /// Bounded, printable fields; the ids are single path segments. A reference from a remote
    /// record is checked before it is kept.
    pub fn valid(&self) -> bool {
        let printable =
            |value: &str| (1..=200).contains(&value.len()) && !value.chars().any(char::is_control);
        printable(&self.producer)
            && [
                &self.state,
                &self.run_id,
                &self.request_id,
                &self.session_id,
            ]
            .into_iter()
            .all(|id| printable(id) && valid_id(id))
    }
}

/// A session or state id: letters, digits, `-`, `_` and `.`, never only dots.
fn valid_id(id: &str) -> bool {
    (1..=200).contains(&id.len())
        && id.bytes().any(|byte| byte != b'.')
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// The session store of a state.
pub fn directory(state: &Path) -> PathBuf {
    state.join(DIRECTORY)
}

/// The conversation file of a session id.
pub fn path(state: &Path, id: &str) -> Result<PathBuf, String> {
    if !valid_id(id) {
        return Err(format!("Invalid Agent session id: {id:?}."));
    }
    Ok(directory(state).join(format!("{id}.jsonl")))
}

/// Where new conversations are saved, and as which producer and state; `None` when saving is
/// off (`agentSessions.enabled: false` in `limits.json`).
#[derive(Debug, Clone)]
pub struct Saving {
    pub state: PathBuf,
    pub state_id: String,
    pub producer: String,
}

fn warn(message: &str) {
    let _ = writeln!(std::io::stderr().lock(), "{message}");
}

/// Appends one session's events. It writes nothing when saving is off, and after a failed
/// write it reports the error once and writes nothing more.
pub struct Recorder {
    id: String,
    target: Option<(PathBuf, Value)>,
    reference: Option<SessionRef>,
    file: Option<File>,
    /// The follow-up whose events this appends.
    send: Option<usize>,
}

impl Recorder {
    /// A session that is not saved.
    pub fn off(id: &str) -> Self {
        Self {
            id: id.into(),
            target: None,
            reference: None,
            file: None,
            send: None,
        }
    }

    /// The recorder of a request's new review session `id`.
    pub fn new(saving: Option<&Saving>, request: &Request, id: &str) -> Self {
        let Some(saving) = saving else {
            return Self::off(id);
        };
        let file = match path(&saving.state, id) {
            Ok(path) => path,
            Err(error) => {
                warn(&error);
                return Self::off(id);
            }
        };
        let reference = SessionRef {
            producer: saving.producer.clone(),
            state: saving.state_id.clone(),
            run_id: request.run_id.clone(),
            request_id: request.id.clone(),
            session_id: id.into(),
        };
        let mut identity = json!(reference);
        identity["evalId"] = json!(request.eval_id);
        identity["target"] = json!(request.target);
        Self {
            target: Some((file, identity)),
            ..Self::off(id)
        }
    }

    /// A session saved below `state` for no Run.
    #[cfg(test)]
    pub(crate) fn test(state: &Path, id: &str) -> Self {
        let reference = SessionRef {
            producer: "tester@host".into(),
            state: "state".into(),
            run_id: "run".into(),
            request_id: "request".into(),
            session_id: id.into(),
        };
        Self {
            target: Some((path(state, id).unwrap(), json!(reference))),
            ..Self::off(id)
        }
    }

    /// Append follow-up `send` to the saved conversation at `path`.
    pub(crate) fn append(path: &Path, id: &str, send: usize) -> Result<Self, String> {
        let file = platform::open_no_follow(File::options().append(true), path)
            .map_err(|error| format!("Cannot open Agent session {id}: {error}"))?;
        Ok(Self {
            file: Some(file),
            send: Some(send),
            ..Self::off(id)
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// The saved conversation's reference, once its first event is written.
    pub fn reference(&self) -> Option<&SessionRef> {
        self.reference.as_ref()
    }

    /// Create the session file with its `review` event: the identity, then `details`.
    pub(crate) fn start(&mut self, details: Value) {
        let Some((path, identity)) = self.target.take() else {
            return;
        };
        let reference = serde_json::from_value(identity.clone()).expect("a session reference");
        match create(&path) {
            Ok(file) => {
                self.file = Some(file);
                let mut event = json!({"kind":"review","version":VERSION});
                for (name, value) in identity
                    .as_object()
                    .into_iter()
                    .chain(details.as_object())
                    .flatten()
                {
                    event[name] = value.clone();
                }
                self.event(event);
                if self.file.is_some() {
                    self.reference = Some(reference);
                }
            }
            Err(error) => self.fail(&error),
        }
    }

    /// One message of `turn`; `repair` marks the prompt that asked for the verdict again.
    pub(crate) fn message(&mut self, turn: usize, message: &Message, repair: bool) {
        if self.file.is_none() {
            return;
        }
        let mut event = json!({"kind":"message","turn":turn,"message":message});
        if repair {
            event["repair"] = json!(true);
        }
        self.event(event);
    }

    /// The message of tool results that `turn` sends, with whether each failed.
    pub(crate) fn tool_results(&mut self, turn: usize, message: &Message, failed: &[bool]) {
        if self.file.is_none() {
            return;
        }
        self.event(json!({"kind":"message","turn":turn,"message":message,"isError":failed}));
    }

    /// Append an event, stamped with its time and the follow-up it belongs to.
    pub(crate) fn event(&mut self, mut event: Value) {
        let Some(file) = &mut self.file else {
            return;
        };
        if let Some(send) = self.send {
            event["send"] = json!(send);
        }
        event["at"] = json!(crate::broker::now());
        let mut line = serde_json::to_vec(&event).expect("session events are JSON");
        line.push(b'\n');
        if let Err(error) = file.write_all(&line).and_then(|()| file.flush()) {
            self.fail(&error.to_string());
        }
    }

    fn fail(&mut self, error: &str) {
        self.file = None;
        warn(&format!(
            "Cannot save Agent session {}: {error}; the review goes on without saving it.",
            self.id
        ));
    }
}

/// A new owner-only session file in an owner-only session store.
fn create(path: &Path) -> Result<File, String> {
    let directory = path.parent().expect("a session file has a directory");
    platform::create_private_dir_all(directory).map_err(|e| e.to_string())?;
    if !platform::is_private_dir(directory).map_err(|e| e.to_string())? {
        return Err(format!(
            "{} must have owner-only permissions.",
            directory.display()
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
    pub events: Vec<Value>,
}

impl Conversation {
    /// Read a session file; `None` when it does not exist. A last line cut short by a crash
    /// is left out.
    pub fn load(path: &Path) -> Result<Option<Self>, String> {
        let file = match platform::open_no_follow(File::options().read(true), path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("Cannot read {}: {error}", path.display())),
        };
        if !file.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err(format!("{} is not a regular file.", path.display()));
        }
        let lines: Vec<String> = BufReader::new(file)
            .lines()
            .collect::<Result<_, _>>()
            .map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
        let count = lines.len();
        let mut events = Vec::new();
        for (index, line) in lines.into_iter().enumerate() {
            match serde_json::from_str::<Value>(&line) {
                Ok(event) if event.is_object() => events.push(event),
                _ if index + 1 == count => {}
                _ => {
                    return Err(format!(
                        "{} line {} is not a session event.",
                        path.display(),
                        index + 1
                    ));
                }
            }
        }
        if events.first().is_none_or(|event| event["kind"] != "review") {
            return Err(format!("{} has no review event.", path.display()));
        }
        Ok(Some(Self {
            path: path.into(),
            events,
        }))
    }

    /// The `review` event.
    pub fn header(&self) -> &Value {
        &self.events[0]
    }

    /// How many follow-ups were sent.
    pub fn sends(&self) -> usize {
        self.events
            .iter()
            .filter(|event| event["kind"] == "send")
            .count()
    }

    /// Every message in order, the review's and its follow-ups', ready to send again. Tool
    /// calls a stopped review never answered get an error result next to the results of
    /// those that ran, since a provider expects every call answered.
    pub fn history(&self) -> Result<Vec<Message>, String> {
        let mut messages = self
            .events
            .iter()
            .filter(|event| event["kind"] == "message")
            .map(|event| serde_json::from_value::<Message>(event["message"].clone()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("{}: {e}", self.path.display()))?;
        let Some(last) = messages
            .iter()
            .rposition(|message| matches!(message, Message::Assistant { .. }))
        else {
            return Ok(messages);
        };
        let answered: Vec<_> = messages[last + 1..]
            .iter()
            .flat_map(|message| match message {
                Message::User { content } => content.as_slice(),
                _ => &[],
            })
            .filter_map(|part| match part {
                UserContent::ToolResult(result) => Some(result.call.wire()),
                _ => None,
            })
            .collect();
        let Message::Assistant { content, .. } = &messages[last] else {
            unreachable!("the last answer")
        };
        let missing: Vec<_> = content
            .iter()
            .filter_map(|part| match part {
                AssistantContent::ToolCall(call) if !answered.contains(&call.id.wire()) => {
                    Some(UserContent::ToolResult(call.result(vec![
                        ToolResultContent::text(
                            "Not run: the review stopped before this tool call completed.",
                        ),
                    ])))
                }
                _ => None,
            })
            .collect();
        if missing.is_empty() {
            return Ok(messages);
        }
        let ran = last + 1 < messages.len();
        match messages.last_mut() {
            Some(Message::User { content }) if ran => content.extend(missing),
            _ => messages.push(Message::User { content: missing }),
        }
        Ok(messages)
    }
}

/// Hold `<sessionId>.lock` until the returned file drops: sends to one session take turns.
pub async fn lock(state: &Path, id: &str) -> Result<File, String> {
    let path = path(state, id)?.with_extension("lock");
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
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) => {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            Err(std::fs::TryLockError::Error(error)) => {
                return Err(format!("Cannot lock Agent session {id}: {error}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference() -> SessionRef {
        SessionRef {
            producer: "alice@laptop".into(),
            state: "6f1c0a52-8d1e-4c43-9a55-31f6a7f2d0e4".into(),
            run_id: "run-Hq2b9X".into(),
            request_id: "run-Hq2b9X-3".into(),
            session_id: "0e8c7c2e-2a49-4b8e-9f7a-5d7a0c3b1f20".into(),
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
            producer: "a/b@host".into(),
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
        assert!(path(Path::new("/state"), "..").is_err());
        assert!(path(Path::new("/state"), "a/b").is_err());
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
        let replay = |messages: &[&Message]| {
            let events = std::iter::once(json!({"kind":"review"}))
                .chain(
                    messages
                        .iter()
                        .map(|message| json!({"kind":"message","message":message})),
                )
                .collect();
            Conversation {
                path: "session.jsonl".into(),
                events,
            }
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
        let not_run = json!([{"type":"text","text":"Not run: the review stopped before this tool call completed."}]);
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
        let root = tempfile::tempdir().unwrap();
        // The store's place is taken by a file, so the session cannot be created.
        let state = root.path().join("state");
        std::fs::create_dir(&state).unwrap();
        std::fs::write(directory(&state), "not a directory").unwrap();
        let reference = reference();
        let mut recorder = Recorder {
            target: Some((
                path(&state, &reference.session_id).unwrap(),
                json!(reference),
            )),
            ..Recorder::off(&reference.session_id)
        };
        recorder.start(json!({"backend":"openai"}));
        recorder.message(1, &Message::user("still recorded nowhere"), false);
        recorder.event(json!({"kind":"end"}));
        assert!(recorder.reference().is_none());
        assert!(std::fs::metadata(directory(&state)).unwrap().is_file());
    }
}
