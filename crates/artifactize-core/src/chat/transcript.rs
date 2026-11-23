//! Chat presents the recorder's existing messages and streamed delivery, with typed Run links.
use std::collections::{BTreeMap, BTreeSet, HashMap};

use rig_core::message::{AssistantContent, Message, UserContent};
use serde::Serialize;

use crate::{
    agent::{
        error::Code,
        session::{Answer, Conversation, DeliveryKind, DeliveryState, Event, Kind},
    },
    runs, store,
    types::{DeliveryBlock, EvalId, RequestStatus, RunId},
};

use super::{
    Outcome,
    details::{self, ResultView, ToolKind},
};

#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Block {
    Text {
        text: String,
    },
    Tool {
        name: String,
        arguments: String,
        result: Option<ToolResult>,
        tool_kind: ToolKind,
        summary: String,
    },
    Run {
        card: RunCard,
    },
}

/// A completed tool result; a pending call has no result at all.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResult {
    pub output: String,
    pub failed: Option<bool>,
    pub exit_code: Option<i32>,
    pub omitted_lines: usize,
    pub output_truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunCard {
    pub id: RunId,
    pub state: Option<runs::State>,
    pub failed: Vec<EvalId>,
    pub waiting: Vec<EvalId>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TurnState {
    Running,
    Completed,
    Stopped,
    Interrupted,
    Failed { message: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    pub user: String,
    pub blocks: Vec<Block>,
    pub state: TurnState,
}

struct Fold {
    turn: Turn,
    delivered: BTreeSet<usize>,
    delivery_blocks: BTreeMap<(usize, usize, DeliveryBlock), usize>,
    calls: HashMap<String, usize>,
}

impl Fold {
    fn new(user: String) -> Self {
        Self {
            turn: Turn {
                user,
                blocks: Vec::new(),
                state: TurnState::Interrupted,
            },
            delivered: BTreeSet::new(),
            delivery_blocks: BTreeMap::new(),
            calls: HashMap::new(),
        }
    }

    fn event(&mut self, event: &Event) {
        let blocks = &mut self.turn.blocks;
        match &event.kind {
            Kind::Delivery(delivery)
                if delivery.kind == DeliveryKind::Text && !delivery.block.is_attempt() =>
            {
                let index = *self
                    .delivery_blocks
                    .entry((delivery.turn, delivery.attempt, delivery.block.clone()))
                    .or_insert_with(|| {
                        let index = blocks.len();
                        blocks.push(Block::Text {
                            text: String::new(),
                        });
                        index
                    });
                let Block::Text { text } = &mut blocks[index] else {
                    unreachable!("recorded text block")
                };
                if delivery.state == DeliveryState::Delta {
                    text.push_str(&delivery.text);
                } else {
                    text.clone_from(&delivery.text);
                }
                self.delivered.insert(delivery.turn);
            }
            Kind::Message(message) | Kind::ToolOutcome(message) => match message.message() {
                Message::Assistant { content, .. } => {
                    for (part_index, part) in content.iter().enumerate() {
                        match part {
                            AssistantContent::Text(text)
                                if !self.delivered.contains(&message.turn) =>
                            {
                                blocks.push(Block::Text {
                                    text: text.text.clone(),
                                });
                            }
                            AssistantContent::ToolCall(call) => {
                                let index = blocks.len();
                                self.calls.insert(call.id.wire().into_owned(), index);
                                let name = call.function.name.as_str();
                                let tool_kind = ToolKind::for_name(name);
                                let summary = if tool_kind == ToolKind::Read {
                                    let (path, pattern) = message.builtin_target_parts(part_index);
                                    details::read_summary(name, path, pattern)
                                } else {
                                    details::summary(name, &call.function.arguments)
                                };
                                blocks.push(Block::Tool {
                                    name: name.to_owned(),
                                    arguments: call.function.arguments.to_string(),
                                    result: None,
                                    tool_kind,
                                    summary,
                                });
                            }
                            _ => {}
                        }
                    }
                }
                Message::User { content } => {
                    for (index, result) in content
                        .iter()
                        .filter_map(|part| match part {
                            UserContent::ToolResult(result) => Some(result),
                            _ => None,
                        })
                        .enumerate()
                    {
                        let Some(Block::Tool {
                            result: saved,
                            tool_kind,
                            summary,
                            ..
                        }) = self
                            .calls
                            .get(result.call.wire().as_ref())
                            .and_then(|index| blocks.get_mut(*index))
                        else {
                            continue;
                        };
                        let failed = message.is_error.get(index).copied();
                        // The final replay message can repeat an already streamed outcome.
                        if let Some(saved) = saved {
                            saved.failed = failed;
                            continue;
                        }
                        let view = ResultView::new(*tool_kind, result.content.iter());
                        if let Some(code) = view.exit_code {
                            summary.push_str(&format!(" (exit {code})"));
                        }
                        *saved = Some(ToolResult {
                            output: view.output,
                            failed,
                            exit_code: view.exit_code,
                            omitted_lines: view.omitted_lines,
                            output_truncated: view.truncated,
                        });
                    }
                }
                Message::System { .. } => {}
            },
            Kind::RunReference { run } => blocks.push(Block::Run {
                card: RunCard {
                    id: run.clone(),
                    state: None,
                    failed: Vec::new(),
                    waiting: Vec::new(),
                },
            }),
            Kind::Answer(answer) => {
                self.turn.state = match outcome(answer) {
                    Outcome::Completed => {
                        let Answer::Completed(text) = answer else {
                            unreachable!("completed answer")
                        };
                        if !text.is_empty()
                            && !blocks
                                .iter()
                                .any(|block| matches!(block, Block::Text { .. }))
                        {
                            blocks.push(Block::Text { text: text.clone() });
                        }
                        TurnState::Completed
                    }
                    Outcome::Stopped => TurnState::Stopped,
                    Outcome::Failed => {
                        let Answer::Failed { message, .. } = answer else {
                            unreachable!("failed answer")
                        };
                        TurnState::Failed {
                            message: message
                                .clone()
                                .unwrap_or_else(|| "The Chat turn failed.".into()),
                        }
                    }
                    Outcome::Interrupted => TurnState::Interrupted,
                    Outcome::Running => unreachable!("an answer ends the turn"),
                };
            }
            _ => {}
        }
    }
}

pub(super) fn turns(conversation: Option<&Conversation>, running: bool) -> Vec<Turn> {
    let mut turns = BTreeMap::new();
    for event in conversation
        .iter()
        .flat_map(|conversation| conversation.events())
    {
        let Some(send) = event.send else { continue };
        if let Kind::Send(start) = &event.kind {
            turns.insert(send, Fold::new(start.text.clone().unwrap_or_default()));
        } else if let Some(fold) = turns.get_mut(&send) {
            fold.event(event);
        }
    }
    if running
        && let Some(mut last) = turns.last_entry()
        && matches!(last.get().turn.state, TurnState::Interrupted)
    {
        last.get_mut().turn.state = TurnState::Running;
    }
    turns.into_values().map(|fold| fold.turn).collect()
}

pub(super) async fn update_runs(context: &runs::Context, turns: &mut [Turn]) {
    let mut cards: BTreeMap<RunId, RunCard> = BTreeMap::new();
    for card in turns
        .iter_mut()
        .flat_map(|turn| turn.blocks.iter_mut())
        .filter_map(|block| match block {
            Block::Run { card } => Some(card),
            _ => None,
        })
    {
        use std::collections::btree_map::Entry;
        match cards.entry(card.id.clone()) {
            Entry::Occupied(current) => *card = current.get().clone(),
            Entry::Vacant(slot) => {
                let (status, requests) = tokio::join!(
                    runs::status(context, &card.id),
                    store::read_requests_for_run(
                        context.state(),
                        Some(card.id.clone()),
                        Some(context.repo())
                    ),
                );
                card.state = status.ok().map(|observation| observation.state);
                if let Ok(requests) = requests {
                    for view in requests {
                        let request = &view.request;
                        match request.status() {
                            RequestStatus::Red | RequestStatus::Error => {
                                card.failed.push(request.eval_id.clone())
                            }
                            status if status.is_human_wait() => {
                                card.waiting.push(request.eval_id.clone())
                            }
                            _ => {}
                        }
                    }
                }
                slot.insert(card.clone());
            }
        }
    }
}

pub(super) fn outcome(answer: &Answer) -> Outcome {
    match answer {
        Answer::Completed(_) => Outcome::Completed,
        Answer::Failed {
            code: Some(Code::Cancelled),
            ..
        } => Outcome::Stopped,
        Answer::Failed { .. } => Outcome::Failed,
        Answer::Empty => Outcome::Interrupted,
    }
}

pub(super) fn last_outcome(conversation: &Conversation) -> Option<Outcome> {
    conversation
        .events()
        .iter()
        .rev()
        .find_map(|event| match &event.kind {
            Kind::Answer(answer) => Some(outcome(answer)),
            Kind::Send(_) => Some(Outcome::Interrupted),
            _ => None,
        })
}
