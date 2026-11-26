//! Chat presents the recorder's existing messages, streamed delivery, and ACP rich events
//! folded into one canonical `Block` model (`F-PURE-CORE`, `C-ACP-EDGE-ISOLATION`).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use rig_core::message::{AssistantContent, Message, UserContent};
use serde::Serialize;

use crate::{
    acp::{
        PermissionOption, PermissionRecord, PermissionStatus, PlanStep, ToolDiff, ToolLocation,
        ToolStatus,
    },
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
    Thought {
        text: String,
    },
    Tool {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        call_id: Option<String>,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        arguments: String,
        result: Option<ToolResult>,
        tool_kind: ToolKind,
        status: ToolStatus,
        summary: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        locations: Vec<ToolLocation>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        diff: Option<ToolDiff>,
    },
    Plan {
        steps: Vec<PlanStep>,
    },
    Permission {
        permission_id: String,
        title: String,
        tool_kind: ToolKind,
        summary: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
        options: Vec<PermissionOption>,
        status: PermissionStatus,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selected_option_id: Option<String>,
    },
    Notice {
        text: String,
    },
    Handoff {
        from: String,
        to: String,
        summary: String,
    },
    Run {
        card: RunCard,
    },
}

/// A completed or failed tool result; a pending/running call has no result yet.
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TurnState {
    Running,
    /// Derived live from `session.running && has_pending_permission`; never persisted in `Activity`.
    WaitingPermission,
    Completed,
    Stopped,
    Interrupted,
    Failed {
        message: String,
    },
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
    delivery_blocks: BTreeMap<(usize, usize, DeliveryKind, DeliveryBlock), usize>,
    calls: HashMap<String, usize>,
    permissions: HashMap<String, usize>,
    plan_block: Option<usize>,
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
            permissions: HashMap::new(),
            plan_block: None,
        }
    }

    fn event(&mut self, event: &Event) {
        let blocks = &mut self.turn.blocks;
        match &event.kind {
            Kind::Delivery(delivery) if !delivery.block.is_attempt() => {
                let key = (
                    delivery.turn,
                    delivery.attempt,
                    delivery.kind,
                    delivery.block.clone(),
                );
                let index = *self.delivery_blocks.entry(key).or_insert_with(|| {
                    let index = blocks.len();
                    match delivery.kind {
                        DeliveryKind::Text => blocks.push(Block::Text {
                            text: String::new(),
                        }),
                        DeliveryKind::Summary => blocks.push(Block::Thought {
                            text: String::new(),
                        }),
                    }
                    index
                });
                match &mut blocks[index] {
                    Block::Text { text } | Block::Thought { text } => {
                        if delivery.state == DeliveryState::Delta {
                            text.push_str(&delivery.text);
                        } else {
                            text.clone_from(&delivery.text);
                        }
                    }
                    _ => unreachable!("recorded delivery block"),
                }
                if delivery.kind == DeliveryKind::Text {
                    self.delivered.insert(delivery.turn);
                }
            }
            Kind::AcpToolCall(call) => {
                let index = blocks.len();
                self.calls.insert(call.call_id.clone(), index);
                let result = call.output.as_ref().map(|output| ToolResult {
                    output: output.clone(),
                    failed: Some(call.status == ToolStatus::Failed),
                    exit_code: None,
                    omitted_lines: 0,
                    output_truncated: call.output_truncated,
                });
                blocks.push(Block::Tool {
                    call_id: Some(call.call_id.clone()),
                    name: call.name.clone(),
                    title: (!call.title.is_empty()).then(|| call.title.clone()),
                    arguments: call.arguments.clone(),
                    result,
                    tool_kind: call.tool_kind,
                    status: call.status,
                    summary: call.summary.clone(),
                    locations: call.locations.clone(),
                    diff: call.diff.clone(),
                });
            }
            Kind::AcpToolUpdate(update) => {
                let Some(Block::Tool {
                    title,
                    tool_kind,
                    status,
                    summary,
                    locations,
                    diff,
                    result,
                    ..
                }) = self
                    .calls
                    .get(&update.call_id)
                    .and_then(|index| blocks.get_mut(*index))
                else {
                    return;
                };
                if let Some(new_title) = &update.title {
                    *title = Some(new_title.clone());
                }
                if let Some(kind) = update.tool_kind {
                    *tool_kind = kind;
                }
                if let Some(new_status) = update.status {
                    *status = new_status;
                }
                if let Some(new_summary) = &update.summary {
                    *summary = new_summary.clone();
                }
                if let Some(new_locations) = &update.locations {
                    *locations = new_locations.clone();
                }
                if let Some(new_diff) = &update.diff {
                    *diff = Some(new_diff.clone());
                }
                if update.output.is_some() || update.failed.is_some() || update.exit_code.is_some()
                {
                    let failed = update
                        .failed
                        .or_else(|| Some(*status == ToolStatus::Failed));
                    let existing_output = result
                        .as_ref()
                        .map(|r| r.output.clone())
                        .unwrap_or_default();
                    let output = update.output.clone().unwrap_or(existing_output);
                    *result = Some(ToolResult {
                        output,
                        failed,
                        exit_code: update.exit_code,
                        omitted_lines: 0,
                        output_truncated: update.output_truncated,
                    });
                }
            }
            Kind::AcpPlan(plan) => {
                if let Some(index) = self.plan_block
                    && let Some(Block::Plan { steps }) = blocks.get_mut(index)
                {
                    *steps = plan.steps.clone();
                } else {
                    let index = blocks.len();
                    blocks.push(Block::Plan {
                        steps: plan.steps.clone(),
                    });
                    self.plan_block = Some(index);
                }
            }
            Kind::AcpPermission(perm) => {
                if let Some(&index) = self.permissions.get(&perm.permission_id)
                    && let Some(Block::Permission {
                        status,
                        selected_option_id,
                        ..
                    }) = blocks.get_mut(index)
                {
                    *status = perm.status;
                    if perm.selected_option_id.is_some() {
                        *selected_option_id = perm.selected_option_id.clone();
                    }
                } else {
                    let index = blocks.len();
                    self.permissions.insert(perm.permission_id.clone(), index);
                    blocks.push(Block::Permission {
                        permission_id: perm.permission_id.clone(),
                        title: perm.title.clone(),
                        tool_kind: perm.tool_kind,
                        summary: perm.summary.clone(),
                        detail: perm.detail.clone(),
                        options: perm.options.clone(),
                        status: perm.status,
                        selected_option_id: perm.selected_option_id.clone(),
                    });
                }
            }
            Kind::Notice { text } => {
                blocks.push(Block::Notice { text: text.clone() });
            }
            Kind::Handoff(handoff) => {
                blocks.push(Block::Handoff {
                    from: handoff.from.clone(),
                    to: handoff.to.clone(),
                    summary: handoff.summary.clone(),
                });
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
                                let wire_id = call.id.wire().into_owned();
                                self.calls.insert(wire_id.clone(), index);
                                let name = call.function.name.as_str();
                                let tool_kind = ToolKind::for_name(name);
                                let summary = if tool_kind == ToolKind::Read {
                                    let (path, pattern) = message.builtin_target_parts(part_index);
                                    details::read_summary(name, path, pattern)
                                } else {
                                    details::summary(name, &call.function.arguments)
                                };
                                blocks.push(Block::Tool {
                                    call_id: Some(wire_id),
                                    name: name.to_owned(),
                                    title: None,
                                    arguments: call.function.arguments.to_string(),
                                    result: None,
                                    tool_kind,
                                    status: ToolStatus::Running,
                                    summary,
                                    locations: Vec::new(),
                                    diff: None,
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
                            status,
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
                        *status = if failed == Some(true) {
                            ToolStatus::Failed
                        } else {
                            ToolStatus::Completed
                        };
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

pub(super) fn turns(
    conversation: Option<&Conversation>,
    running: bool,
    live_permissions: &[PermissionRecord],
) -> Vec<Turn> {
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

    // Reconcile live SQLite permission rows into the newest turn so remote windows always see
    // authoritative CAS status even before the next JSONL event flushes.
    if let Some((_, last_fold)) = turns.iter_mut().next_back() {
        for perm in live_permissions {
            if let Some(&idx) = last_fold.permissions.get(&perm.permission_id)
                && let Some(Block::Permission {
                    status,
                    selected_option_id,
                    ..
                }) = last_fold.turn.blocks.get_mut(idx)
            {
                *status = perm.status;
                if perm.selected_option_id.is_some() {
                    *selected_option_id = perm.selected_option_id.clone();
                }
            } else if matches!(
                perm.status,
                PermissionStatus::Pending
                    | PermissionStatus::Selected
                    | PermissionStatus::Delivering
            ) {
                let idx = last_fold.turn.blocks.len();
                last_fold
                    .permissions
                    .insert(perm.permission_id.clone(), idx);
                last_fold.turn.blocks.push(Block::Permission {
                    permission_id: perm.permission_id.clone(),
                    title: perm.title.clone(),
                    tool_kind: perm.tool_kind,
                    summary: perm.summary.clone(),
                    detail: perm.detail.clone(),
                    options: perm.options.clone(),
                    status: perm.status,
                    selected_option_id: perm.selected_option_id.clone(),
                });
            }
        }
    }

    let has_pending_permission = live_permissions
        .iter()
        .any(|perm| perm.status == PermissionStatus::Pending);
    if running
        && let Some(mut last) = turns.last_entry()
        && matches!(last.get().turn.state, TurnState::Interrupted)
    {
        last.get_mut().turn.state = if has_pending_permission {
            TurnState::WaitingPermission
        } else {
            TurnState::Running
        };
    }

    // Mark any unfinished tools on settled turns as Cancelled/Interrupted rather than Running.
    for fold in turns.values_mut() {
        if !matches!(
            fold.turn.state,
            TurnState::Running | TurnState::WaitingPermission
        ) {
            for block in &mut fold.turn.blocks {
                if let Block::Tool { status, .. } = block
                    && matches!(status, ToolStatus::Pending | ToolStatus::Running)
                {
                    *status = ToolStatus::Cancelled;
                }
            }
        }
    }

    turns.into_values().map(|fold| fold.turn).collect()
}

/// Merge any admitted Runs for this Chat session whose `Kind::RunReference` JSONL append failed
/// (`Origin::Chat.send` recovery), then hydrate live Run status onto every `RunCard`.
pub(super) async fn update_runs(
    state: &std::path::Path,
    worktree: &std::path::Path,
    session: &crate::types::SessionId,
    turns: &mut [Turn],
) {
    let mut present_runs: BTreeSet<RunId> = turns
        .iter()
        .flat_map(|turn| turn.blocks.iter())
        .filter_map(|block| match block {
            Block::Run { card } => Some(card.id.clone()),
            _ => None,
        })
        .collect();

    if let Ok(admitted) = store::read_chat_runs(state, worktree, session).await {
        for (run_id, send) in admitted {
            if present_runs.contains(&run_id) {
                continue;
            }
            let target_idx = send
                .and_then(|s| s.checked_sub(1))
                .filter(|&idx| idx < turns.len())
                .or_else(|| turns.len().checked_sub(1));
            if let Some(turn) = target_idx.and_then(|idx| turns.get_mut(idx)) {
                turn.blocks.push(Block::Run {
                    card: RunCard {
                        id: run_id.clone(),
                        state: None,
                        failed: Vec::new(),
                        waiting: Vec::new(),
                    },
                });
                present_runs.insert(run_id);
            }
        }
    }

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
                if let Ok(view) = store::read_run_snapshot(state, card.id.as_str()).await
                    && matches!(
                        &view.run.origin,
                        Some(runs::Origin::Chat { session_id, .. }) if session_id == session
                    )
                {
                    *card = run_card(view);
                }
                slot.insert(card.clone());
            }
        }
    }
}

fn run_card(view: store::RunView) -> RunCard {
    let mut card = RunCard {
        id: view.run.id.clone(),
        state: Some(runs::saved_state(&view)),
        failed: Vec::new(),
        waiting: Vec::new(),
    };
    for request in view.requests {
        match request.status() {
            RequestStatus::Red | RequestStatus::Error => card.failed.push(request.eval_id.clone()),
            status if status.is_human_wait() => card.waiting.push(request.eval_id.clone()),
            _ => {}
        }
    }
    card
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
