//! Fold stored session events and live delivery into stable transcript patches.

use super::super::{Delivery, DeliveryKind, DeliveryState, Event, Kind};
use super::{
    BlockId, Patch,
    candidate::{Candidate, candidate},
    render::{
        Activity, Group, State, Target, failure_reason, header, markdown, result_text, target,
    },
};
use rig_core::message::{AssistantContent, Message, ToolCall, ToolResult, UserContent};
use std::collections::{BTreeMap, BTreeSet};

struct LiveBlock {
    id: BlockId,
    text: String,
}

/// The id a tool call and its result share on the wire, which links the two: built only
/// from a recorded call identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Correlation(String);

impl Correlation {
    fn of(id: &rig_core::message::CallId) -> Self {
        Self(id.wire().into_owned())
    }
}

#[derive(Default)]
pub struct Transcript {
    blocks: usize,
    groups: BTreeMap<BlockId, Group>,
    calls: BTreeMap<(Option<usize>, Correlation), (BlockId, usize)>,
    active: Option<BlockId>,
    expanded: BTreeSet<BlockId>,
    assistant: Option<crate::types::Sha256Bytes>,
    question: Option<(Option<usize>, crate::types::Sha256Bytes)>,
    /// A JSON-looking assistant message is unchanged until its authoritative End arrives.
    result_candidate: Option<(BlockId, Candidate)>,
    deliveries: BTreeMap<
        (
            Option<usize>,
            usize,
            usize,
            DeliveryKind,
            crate::types::DeliveryBlock,
        ),
        LiveBlock,
    >,
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
        let candidate = if !user { candidate(text) } else { None };
        let text = markdown(text);
        if !user {
            self.assistant = Some(crate::types::Sha256Bytes::of(text.trim().as_bytes()));
        }
        let id = self.slot();
        self.result_candidate = candidate.map(|value| (id, value));
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
    fn call(
        &mut self,
        send: Option<usize>,
        call: &ToolCall,
        call_target: Target<'_>,
        dirty: &mut BTreeSet<BlockId>,
    ) {
        let key = (send, Correlation::of(&call.id));
        // Repeated recorded IDs do not invent another execution or attach to a later call.
        if self.calls.contains_key(&key) {
            return;
        }
        let id = self.group();
        let group = self.groups.get_mut(&id).expect("active group exists");
        let index = group.activities.len();
        group.activities.push(Activity {
            name: header(call.function.name.as_str()),
            target: target(call_target),
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
        let key = (send, Correlation::of(&result.call));
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
        if delivery.block.is_attempt() && delivery.state == DeliveryState::Interrupted {
            return self
                .deliveries
                .iter()
                .filter(|((follow, turn, attempt, _, _), block)| {
                    (*follow, *turn, *attempt) == (send, delivery.turn, delivery.attempt)
                        && !block.text.is_empty()
                })
                .map(|((_, _, _, kind, _), block)| {
                    let label = if *kind == DeliveryKind::Summary {
                        "Thinking · partial"
                    } else {
                        "Partial response"
                    };
                    Patch {
                        id: block.id,
                        text: format!("{label} (interrupted)\n{}\n\n", block.text),
                    }
                })
                .collect();
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
            self.result_candidate = candidate(&text).map(|value| (id, value));
            self.assistant = Some(crate::types::Sha256Bytes::of(
                markdown(&text).trim().as_bytes(),
            ));
        }
        if delivery.state == DeliveryState::Complete {
            self.deliveries
                .get_mut(&key)
                .expect("delivery exists")
                .text
                .clear();
        }
        vec![Patch {
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
        }]
    }
    pub fn is_thinking(&self, id: BlockId) -> bool {
        self.thinking.contains(&id)
    }
    pub fn apply(&mut self, event: &Event) -> Vec<Patch> {
        let mut patches = Vec::new();
        let mut dirty = BTreeSet::new();
        match &event.kind {
            Kind::Delivery(delivery) => return self.delivery(event.send, delivery),
            Kind::Review(_)
            | Kind::Attempt(_)
            | Kind::RunReference { .. }
            | Kind::ToolOutcome(_)
            | Kind::AcpToolCall(_)
            | Kind::AcpToolUpdate(_)
            | Kind::AcpPlan(_)
            | Kind::AcpPermission(_)
            | Kind::Handoff(_)
            | Kind::Notice { .. } => {}
            Kind::Send(send) => {
                self.active = None;
                self.assistant = None;
                if let Some(text) = &send.text {
                    self.prose(text, true, &mut patches);
                    self.question = Some((
                        event.send,
                        crate::types::Sha256Bytes::of(text.trim().as_bytes()),
                    ));
                }
            }
            Kind::Message(message) => {
                if let Some(question) = &message.question {
                    let identity = (
                        event.send,
                        crate::types::Sha256Bytes::of(question.trim().as_bytes()),
                    );
                    if self.question.as_ref() != Some(&identity) {
                        self.prose(question, true, &mut patches);
                    }
                    self.question = None;
                } else {
                    match message.message() {
                        Message::System { .. } => {}
                        Message::Assistant { content, .. } => {
                            for (index, part) in content.iter().enumerate() {
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
                                        let target = message.builtin_target(index).map_or(
                                            Target::Unrecorded,
                                            |target| Target::Builtin {
                                                path: target.path.as_deref(),
                                                pattern: target.pattern.as_deref(),
                                            },
                                        );
                                        self.call(event.send, call, target, &mut dirty);
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
                if let Some(result) = end.result() {
                    let text = result_text(result);
                    if let Some((id, candidate)) = self.result_candidate.take()
                        && candidate.matches(result)
                    {
                        patches.push(Patch {
                            id,
                            text: format!("{text}\n\n"),
                        });
                    } else {
                        self.prose(&text, false, &mut patches);
                    }
                }
                if let Some((code, error)) = end.failure() {
                    self.prose(
                        &format!(
                            "Review failed: {}\n{error}",
                            code.map(crate::agent::error::Code::as_str)
                                .unwrap_or_default()
                        ),
                        false,
                        &mut patches,
                    );
                }
            }
            Kind::Answer(answer) => {
                if let Some(text) = answer.text() {
                    let digest = crate::types::Sha256Bytes::of(markdown(text).trim().as_bytes());
                    if self.assistant != Some(digest) {
                        self.prose(text, false, &mut patches);
                    }
                }
                if let Some((code, error)) = answer.failure() {
                    self.stopped(&mut dirty);
                    self.prose(
                        &format!(
                            "Follow-up failed: {}\n{error}",
                            code.map(crate::agent::error::Code::as_str)
                                .unwrap_or_default()
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
