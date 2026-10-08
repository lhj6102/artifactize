//! Semantic, minimal session presentation. Stored events remain lossless; this view keeps only
//! prose and bounded activity headers, never tool output, arguments, reasoning or media payloads.
use super::{Delivery, DeliveryKind, DeliveryState, Event, Kind};
use rig_core::message::{
    AssistantContent, Message, ToolCall, ToolResult, ToolResultContent, UserContent,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// Activity labels must stay small even when a custom tool or error has very large text.
const HEADER_CHARS: usize = 180;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BlockId(pub usize);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Running,
    Succeeded,
    Failed(String),
    CompletedUnknown,
    Unfinished,
}
impl State {
    fn label(&self) -> &str {
        match self {
            Self::Running => "running",
            Self::Succeeded => "done",
            Self::Failed(_) => "failed",
            Self::CompletedUnknown => "completed (status not recorded)",
            Self::Unfinished => "not completed",
        }
    }
}
#[derive(Debug, Clone)]
pub struct Activity {
    pub name: String,
    pub target: String,
    pub state: State,
    pub orphan: bool,
}
#[derive(Debug, Default)]
pub struct Group {
    pub activities: Vec<Activity>,
}
impl Group {
    pub fn text(&self, expanded: bool) -> String {
        let active = self
            .activities
            .iter()
            .find(|activity| activity.state == State::Running);
        let failed = self
            .activities
            .iter()
            .filter(|activity| matches!(activity.state, State::Failed(_)))
            .collect::<Vec<_>>();
        let mut text = match active {
            Some(activity) => format!(
                "{} {} {} — running{}",
                if expanded { "▾" } else { "▸" },
                activity_label(&activity.name),
                activity.target,
                if self.activities.len() > 1 {
                    format!(" · {} activities", self.activities.len())
                } else {
                    String::new()
                }
            ),
            None => {
                let label = self
                    .activities
                    .first()
                    .map(|activity| activity_label(&activity.name))
                    .unwrap_or("Tools");
                let target = self
                    .activities
                    .first()
                    .map(|activity| activity.target.as_str())
                    .unwrap_or_default();
                format!(
                    "{} {label} {target} — {}{}",
                    if expanded { "▾" } else { "▸" },
                    if failed.is_empty() {
                        "completed"
                    } else {
                        "failed"
                    },
                    if self.activities.len() > 1 {
                        format!(" · {} activities", self.activities.len())
                    } else {
                        String::new()
                    }
                )
            }
        };
        // Failures remain visible even when collapsed. Only recorded failure flags qualify.
        for activity in failed {
            let State::Failed(reason) = &activity.state else {
                unreachable!("failed activities filtered");
            };
            text.push_str(&format!(
                "{}{} {} — failed: {}",
                if expanded { "\n  " } else { " · " },
                activity.name,
                activity.target,
                reason
            ));
        }
        if expanded {
            for activity in &self.activities {
                if matches!(activity.state, State::Failed(_)) {
                    continue;
                }
                text.push_str(&format!(
                    "\n  {} {} — {}{}",
                    activity.name,
                    activity.target,
                    activity.state.label(),
                    if activity.orphan {
                        " (unmatched recorded result)"
                    } else {
                        ""
                    }
                ));
            }
        } else if self.activities.iter().any(|activity| {
            activity.orphan || matches!(activity.state, State::CompletedUnknown | State::Unfinished)
        }) {
            text.push_str(" · incomplete/status unreported");
        }
        text.push_str("\n\n");
        text
    }
}

/// Disk-document edits use stable slots. A group update replaces only that group's text;
/// prose strings leave this layer immediately and are retained by the existing private pages.
pub struct Patch {
    pub id: BlockId,
    pub text: String,
}

struct LiveBlock {
    id: BlockId,
    text: String,
}

#[derive(Default)]
pub struct Transcript {
    blocks: usize,
    groups: BTreeMap<BlockId, Group>,
    calls: BTreeMap<(Option<usize>, String), (BlockId, usize)>,
    active: Option<BlockId>,
    expanded: BTreeSet<BlockId>,
    assistant: Option<[u8; 32]>,
    question: Option<(Option<usize>, [u8; 32])>,
    /// A JSON-looking assistant message is unchanged until an identical authoritative End arrives.
    result_candidate: Option<(BlockId, [u8; 32])>,
    deliveries: BTreeMap<(Option<usize>, usize, usize, DeliveryKind, String), LiveBlock>,
    delivered_turns: BTreeSet<(Option<usize>, usize)>,
    thinking: BTreeSet<BlockId>,
}
impl Transcript {
    fn slot(&mut self) -> BlockId {
        let id = BlockId(self.blocks);
        self.blocks += 1;
        id
    }
    fn prose(&mut self, text: &str, user: bool, patches: &mut Vec<Patch>) {
        if text.trim().is_empty() {
            return;
        }
        self.active = None;
        let candidate = if !user {
            serde_json::from_str::<serde_json::Value>(text)
                .ok()
                .filter(|value| {
                    value
                        .get("verdict")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|verdict| matches!(verdict, "GREEN" | "RED"))
                })
                .map(|value| Sha256::digest(value.to_string().as_bytes()).into())
        } else {
            None
        };
        let text = markdown(text);
        if !user {
            self.assistant = Some(Sha256::digest(text.trim().as_bytes()).into());
        }
        let id = self.slot();
        self.result_candidate = candidate.map(|digest| (id, digest));
        patches.push(Patch {
            id,
            text: format!("{}{text}\n\n", if user { "You\n" } else { "" }),
        });
    }
    fn group(&mut self) -> BlockId {
        if let Some(id) = self.active {
            return id;
        }
        let id = self.slot();
        self.groups.insert(id, Group::default());
        self.active = Some(id);
        id
    }
    fn call(&mut self, send: Option<usize>, call: &ToolCall, dirty: &mut BTreeSet<BlockId>) {
        let key = (send, call.id.wire().into_owned());
        // Repeated recorded IDs do not invent another execution or attach to a later call.
        if self.calls.contains_key(&key) {
            return;
        }
        let id = self.group();
        let group = self.groups.get_mut(&id).expect("active group exists");
        let index = group.activities.len();
        group.activities.push(Activity {
            name: header(call.function.name.as_str()),
            target: target(call),
            state: State::Running,
            orphan: false,
        });
        self.calls.insert(key, (id, index));
        dirty.insert(id);
    }
    fn result(
        &mut self,
        send: Option<usize>,
        result: &ToolResult,
        failed: Option<bool>,
        dirty: &mut BTreeSet<BlockId>,
    ) {
        let key = (send, result.call.wire().into_owned());
        let state = match failed {
            Some(true) => State::Failed(failure_reason(result)),
            Some(false) => State::Succeeded,
            None => State::CompletedUnknown,
        };
        let (id, index) = match self.calls.get(&key).copied() {
            Some(link) => link,
            None => {
                let id = self.group();
                let group = self.groups.get_mut(&id).expect("active group exists");
                let index = group.activities.len();
                group.activities.push(Activity {
                    name: header(result.name.as_str()),
                    target: "target not recorded".into(),
                    state: state.clone(),
                    orphan: true,
                });
                self.calls.insert(key, (id, index));
                (id, index)
            }
        };
        self.groups
            .get_mut(&id)
            .expect("linked group exists")
            .activities[index]
            .state = state;
        dirty.insert(id);
    }
    fn stopped(&mut self, dirty: &mut BTreeSet<BlockId>) {
        for (id, group) in &mut self.groups {
            for activity in &mut group.activities {
                if activity.state == State::Running {
                    activity.state = State::Unfinished;
                    dirty.insert(*id);
                }
            }
        }
        self.active = None;
    }
    fn delivery(&mut self, send: Option<usize>, delivery: &Delivery) -> Vec<Patch> {
        let mut patches = Vec::new();
        if delivery.block == "attempt" && delivery.state == DeliveryState::Interrupted {
            for ((follow, turn, attempt, kind, _), block) in &self.deliveries {
                if (*follow, *turn, *attempt) == (send, delivery.turn, delivery.attempt)
                    && !block.text.is_empty()
                {
                    let label = if *kind == DeliveryKind::Summary {
                        "Thinking · partial"
                    } else {
                        "Partial response"
                    };
                    patches.push(Patch {
                        id: block.id,
                        text: format!("{label} (interrupted)\n{}\n\n", block.text),
                    });
                }
            }
            return patches;
        }
        self.active = None;
        let key = (
            send,
            delivery.turn,
            delivery.attempt,
            delivery.kind,
            delivery.block.clone(),
        );
        let id = if let Some(block) = self.deliveries.get(&key) {
            block.id
        } else {
            let id = self.slot();
            self.deliveries.insert(
                key.clone(),
                LiveBlock {
                    id,
                    text: String::new(),
                },
            );
            id
        };
        let block = self
            .deliveries
            .get_mut(&key)
            .expect("delivery block exists");
        match delivery.state {
            DeliveryState::Delta => block.text.push_str(&delivery.text),
            DeliveryState::Complete | DeliveryState::Interrupted => {
                block.text = delivery.text.clone()
            }
        }
        let text = block.text.clone();
        if delivery.kind == DeliveryKind::Summary {
            self.thinking.insert(id);
        }
        if delivery.kind == DeliveryKind::Text {
            self.delivered_turns.insert((send, delivery.turn));
        }
        if delivery.kind == DeliveryKind::Text && delivery.state == DeliveryState::Complete {
            let value = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .filter(|value| {
                    value
                        .get("verdict")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|verdict| matches!(verdict, "GREEN" | "RED"))
                });
            self.result_candidate =
                value.map(|value| (id, Sha256::digest(value.to_string().as_bytes()).into()));
            self.assistant = Some(Sha256::digest(markdown(&text).trim().as_bytes()).into());
        }
        if delivery.state == DeliveryState::Complete {
            self.deliveries
                .get_mut(&key)
                .expect("delivery exists")
                .text
                .clear();
        }
        patches.push(Patch {
            id,
            text: format!(
                "{}{}{}\n\n",
                if delivery.kind == DeliveryKind::Summary {
                    "Thinking\n"
                } else {
                    ""
                },
                markdown(&text),
                if delivery.state == DeliveryState::Interrupted {
                    "\n[partial, interrupted]"
                } else {
                    ""
                }
            ),
        });
        patches
    }
    pub fn is_thinking(&self, id: BlockId) -> bool {
        self.thinking.contains(&id)
    }
    pub fn apply(&mut self, event: &Event) -> Vec<Patch> {
        let mut patches = Vec::new();
        let mut dirty = BTreeSet::new();
        match &event.kind {
            Kind::Delivery(delivery) => return self.delivery(event.send, delivery),
            Kind::Review(_) | Kind::Attempt(_) => {}
            Kind::Send(send) => {
                self.active = None;
                self.assistant = None;
                if let Some(text) = &send.text {
                    self.prose(text, true, &mut patches);
                    self.question =
                        Some((event.send, Sha256::digest(text.trim().as_bytes()).into()));
                }
            }
            Kind::Message(message) => {
                if let Some(question) = &message.question {
                    let identity = (
                        event.send,
                        Sha256::digest(question.trim().as_bytes()).into(),
                    );
                    if self.question.as_ref() != Some(&identity) {
                        self.prose(question, true, &mut patches);
                    }
                    self.question = None;
                } else {
                    match &message.message {
                        Message::System { .. } => {}
                        Message::Assistant { content, .. } => {
                            for part in content {
                                match part {
                                    AssistantContent::Text(text) => {
                                        if !self
                                            .delivered_turns
                                            .contains(&(event.send, message.turn))
                                        {
                                            self.prose(&text.text, false, &mut patches);
                                        }
                                    }
                                    AssistantContent::ToolCall(call) => {
                                        self.call(event.send, call, &mut dirty)
                                    }
                                    AssistantContent::Reasoning(_) | AssistantContent::Image(_) => {
                                    }
                                }
                            }
                        }
                        Message::User { content } => {
                            let mut result_index = 0;
                            for part in content {
                                match part {
                                    UserContent::Text(text) => {
                                        self.prose(&text.text, true, &mut patches)
                                    }
                                    UserContent::ToolResult(result) => {
                                        self.result(
                                            event.send,
                                            result,
                                            message.is_error.get(result_index).copied(),
                                            &mut dirty,
                                        );
                                        result_index += 1;
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
            Kind::End(end) => {
                self.stopped(&mut dirty);
                if let Some(result) = &end.result {
                    let digest: [u8; 32] = Sha256::digest(result.to_string().as_bytes()).into();
                    let text = result_text(result);
                    if let Some((id, candidate)) = self.result_candidate.take()
                        && candidate == digest
                    {
                        patches.push(Patch {
                            id,
                            text: format!("{text}\n\n"),
                        });
                    } else {
                        self.prose(&text, false, &mut patches);
                    }
                }
                if let Some(error) = &end.error {
                    self.prose(
                        &format!(
                            "Review failed: {}\n{error}",
                            end.error_code.as_deref().unwrap_or_default()
                        ),
                        false,
                        &mut patches,
                    );
                }
            }
            Kind::Answer(answer) => {
                if let Some(text) = &answer.text {
                    let digest: [u8; 32] = Sha256::digest(markdown(text).trim().as_bytes()).into();
                    if self.assistant != Some(digest) {
                        self.prose(text, false, &mut patches);
                    }
                }
                if let Some(error) = &answer.error {
                    self.stopped(&mut dirty);
                    self.prose(
                        &format!(
                            "Follow-up failed: {}\n{error}",
                            answer.error_code.as_deref().unwrap_or_default()
                        ),
                        false,
                        &mut patches,
                    );
                }
            }
        }
        for id in dirty {
            patches.push(Patch {
                id,
                text: self.groups[&id].text(self.expanded.contains(&id)),
            });
        }
        patches.sort_by_key(|patch| patch.id);
        patches
    }
    pub fn expansion(&mut self, expanded: &BTreeSet<BlockId>) -> Vec<Patch> {
        let changed = self
            .expanded
            .symmetric_difference(expanded)
            .copied()
            .collect::<Vec<_>>();
        self.expanded = expanded.clone();
        changed
            .into_iter()
            .filter_map(|id| {
                self.groups.get(&id).map(|group| Patch {
                    id,
                    text: group.text(self.expanded.contains(&id)),
                })
            })
            .collect()
    }
    pub fn is_group(&self, id: BlockId) -> bool {
        self.groups.contains_key(&id)
    }
}

fn header(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .take(HEADER_CHARS)
        .collect()
}
fn activity_label(name: &str) -> &str {
    if name == "read" || name.starts_with("read_") {
        "Read"
    } else if name == "grep"
        || name == "glob"
        || name.starts_with("grep_")
        || name.starts_with("glob_")
    {
        "Search"
    } else if name == "list" || name.starts_with("list_") {
        "List"
    } else if name == "view_image" || name.starts_with("view_image_") {
        "View image"
    } else {
        name
    }
}
fn target(call: &ToolCall) -> String {
    let name = call.function.name.as_str();
    let builtin = ["read", "list", "glob", "grep", "view_image"]
        .iter()
        .any(|base| {
            name == *base
                || name
                    .strip_prefix(base)
                    .is_some_and(|rest| rest.starts_with('_'))
        });
    if !builtin {
        return "target not recorded".into();
    }
    let args = &call.function.arguments;
    let path = args
        .get("path")
        .and_then(serde_json::Value::as_str)
        .map(header)
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| ".".into());
    match args.get("pattern").and_then(serde_json::Value::as_str) {
        Some(pattern) => format!("{path} · {}", header(pattern)),
        None => path,
    }
}
fn failure_reason(result: &ToolResult) -> String {
    result
        .content
        .iter()
        .find_map(|part| match part {
            ToolResultContent::Text(text) => Some(header(&text.text)),
            _ => None,
        })
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "failure reason not recorded".into())
}

/// Owner result fields are meaningful data, not provider transport. Render them recursively
/// as labels and text instead of JSON syntax; unknown owner fields remain visible too.
fn result_text(result: &serde_json::Value) -> String {
    fn field(label: &str, value: &serde_json::Value, indent: usize, lines: &mut Vec<String>) {
        let prefix = "  ".repeat(indent);
        match value {
            serde_json::Value::Object(fields) => {
                if !label.is_empty() {
                    lines.push(format!("{prefix}{label}"));
                }
                for (name, value) in fields {
                    field(name, value, indent + usize::from(!label.is_empty()), lines);
                }
            }
            serde_json::Value::Array(items) => {
                if !label.is_empty() {
                    lines.push(format!("{prefix}{label}"));
                }
                for value in items {
                    field("-", value, indent + 1, lines);
                }
            }
            serde_json::Value::String(text) => lines.push(format!(
                "{prefix}{}{}",
                if label.is_empty() {
                    String::new()
                } else {
                    format!("{label}: ")
                },
                markdown(text)
            )),
            value => lines.push(format!("{prefix}{label}: {value}")),
        }
    }
    let mut lines = Vec::new();
    if let Some(verdict) = result.get("verdict").and_then(serde_json::Value::as_str) {
        lines.push(format!("Verdict: {}", header(verdict)));
    }
    match result {
        serde_json::Value::Object(fields) => {
            for (name, value) in fields {
                if name != "verdict" {
                    field(name, value, 0, &mut lines);
                }
            }
        }
        other => field("Result", other, 0, &mut lines),
    }
    lines.join("\n")
}

/// Lightweight terminal Markdown: keep literal paragraphs/lists and code indentation, remove
/// heading/fence syntax only. No HTML, links, images or private payloads are interpreted.
pub fn markdown(text: &str) -> String {
    let mut code = false;
    text.lines()
        .map(|line| {
            if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
                code = !code;
                return String::new();
            }
            if code {
                return format!("    {line}");
            }
            let heading = line.trim_start_matches('#');
            if heading.len() != line.len() && heading.starts_with(' ') {
                heading.trim_start().to_owned()
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
