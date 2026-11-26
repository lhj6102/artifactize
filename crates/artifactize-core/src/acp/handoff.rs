//! Pure deterministic 128 KiB Handoff builder for cold ACP starts and cross-engine transitions.
//!
//! Includes user prompts, assistant prose, one-line completed tool summaries, and checkout move
//! notes; excludes raw tool arguments/outputs, diffs, thinking summaries, permissions, and the
//! current user prompt. Never makes an extra LLM call (`F-PURE-CORE`).

use std::path::Path;

use crate::{
    acp::ToolStatus,
    chat::{Block, Turn},
    platform,
};

/// Maximum serialized handoff size injected before a cold ACP turn's new user prompt.
pub const HANDOFF_MAX_BYTES: usize = 128 * 1024;
/// Guardrail text injected via system-prompt append extension when supported, or first-prompt
/// preamble otherwise.
pub const ACP_CHAT_GUARDRAILS: &str = "You are running inside Artifactize Chat in the user's selected worktree. Use your native coding tools for files and commands, and use the Artifactize Verify MCP tools to start verifies, inspect Runs, and explain failures or waiting Human reviews. Run acceptance is not approval. Never claim a Human review or submit a verdict for a person.";

/// Build a deterministic handoff summary from prior completed/stopped turns.
/// `prior_turns` must exclude the current turn being sent.
pub fn build_handoff(
    prior_turns: &[Turn],
    moved_from: Option<&Path>,
    worktree: &Path,
) -> Option<String> {
    let mut sections: Vec<String> = Vec::new();
    if let Some(from) = moved_from {
        sections.push(format!(
            "Workspace note: this Chat session moved from {:?} to {:?}. Work in the current worktree.",
            platform::path_text(from),
            platform::path_text(worktree),
        ));
    }

    let mut turn_entries: Vec<String> = Vec::new();
    for (index, turn) in prior_turns.iter().enumerate() {
        let mut lines = Vec::new();
        let user = turn.user.trim();
        if !user.is_empty() {
            lines.push(format!("User: {user}"));
        }
        let mut assistant_parts = Vec::new();
        let mut tool_summaries = Vec::new();
        for block in &turn.blocks {
            match block {
                Block::Text { text } => {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        assistant_parts.push(trimmed.to_owned());
                    }
                }
                Block::Tool {
                    summary,
                    status: ToolStatus::Completed,
                    ..
                } => {
                    let line = summary.lines().next().unwrap_or("").trim();
                    if !line.is_empty() {
                        tool_summaries.push(format!("- {line}"));
                    }
                }
                Block::Run { card } => {
                    let state = card
                        .state
                        .map(|state| format!("{state:?}"))
                        .unwrap_or_else(|| "unknown".into());
                    tool_summaries.push(format!("- Verify Run {} ({state})", card.id));
                }
                // Deliberately exclude thoughts, diffs/outputs, permissions, plans, notices,
                // failed/pending tools, and nested handoffs.
                Block::Thought { .. }
                | Block::Tool { .. }
                | Block::Plan { .. }
                | Block::Permission { .. }
                | Block::Notice { .. }
                | Block::Handoff { .. } => {}
            }
        }
        if !tool_summaries.is_empty() {
            lines.push("Completed actions:".into());
            lines.extend(tool_summaries);
        }
        if !assistant_parts.is_empty() {
            lines.push(format!("Assistant: {}", assistant_parts.join("\n\n")));
        }
        if !lines.is_empty() {
            turn_entries.push(format!("### Turn {}\n{}", index + 1, lines.join("\n")));
        }
    }

    if sections.is_empty() && turn_entries.is_empty() {
        return None;
    }

    let header = "Prior conversation handoff (chronological summary; newest turns preserved if truncated):\n";
    let mut budget = HANDOFF_MAX_BYTES.saturating_sub(header.len());
    let mut prefix = String::new();
    for section in sections {
        let piece = format!("{section}\n\n");
        if piece.len() <= budget {
            budget -= piece.len();
            prefix.push_str(&piece);
        }
    }

    // Keep the most recent turns within the remaining deterministic 128 KiB byte budget.
    let mut kept_rev = Vec::new();
    let mut omitted = 0usize;
    for entry in turn_entries.into_iter().rev() {
        let needed = entry.len() + 2;
        if needed <= budget {
            budget -= needed;
            kept_rev.push(entry);
        } else if kept_rev.is_empty() && budget > 32 {
            kept_rev.push(truncate_utf8(&entry, budget.saturating_sub(20)));
            omitted += 1;
        } else {
            omitted += 1;
        }
    }
    kept_rev.reverse();

    let mut out = String::with_capacity(HANDOFF_MAX_BYTES.min(4096));
    out.push_str(header);
    out.push_str(&prefix);
    if omitted > 0 {
        let note =
            format!("[{omitted} earlier turn(s) omitted to stay within 128 KiB handoff cap]\n\n");
        if out.len() + note.len() <= HANDOFF_MAX_BYTES {
            out.push_str(&note);
        }
    }
    for entry in kept_rev {
        if out.len() + entry.len() + 2 <= HANDOFF_MAX_BYTES {
            out.push_str(&entry);
            out.push_str("\n\n");
        }
    }
    let trimmed = out.trim_end().to_owned();
    (!trimmed.is_empty()).then_some(trimmed)
}

/// Assemble the first prompt sent to a cold ACP session, prepending guardrails when the agent
/// lacks a system-prompt append extension and prepending the deterministic handoff when present.
pub fn compose_prompt(
    user_text: &str,
    handoff: Option<&str>,
    include_guardrails_preamble: bool,
) -> String {
    let mut parts = Vec::new();
    if include_guardrails_preamble {
        parts.push(ACP_CHAT_GUARDRAILS.to_owned());
    }
    if let Some(handoff) = handoff.filter(|text| !text.trim().is_empty()) {
        parts.push(handoff.to_owned());
    }
    if parts.is_empty() {
        user_text.to_owned()
    } else {
        parts.push(format!("Current user message:\n{user_text}"));
        parts.join("\n\n")
    }
}

fn truncate_utf8(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        acp::{PermissionOption, PermissionOptionKind, PermissionStatus, ToolDiff},
        chat::{ToolKind, ToolResult, TurnState},
    };

    #[test]
    fn handoff_includes_prose_and_completed_tool_summaries_but_excludes_thoughts_diffs_and_permissions()
     {
        let turns = vec![Turn {
            user: "Fix checkout rounding".into(),
            blocks: vec![
                Block::Thought {
                    text: "secret internal thought".into(),
                },
                Block::Tool {
                    call_id: Some("c1".into()),
                    name: "edit".into(),
                    title: Some("Edit total.ts".into()),
                    arguments: "{\"raw\":\"huge\"}".into(),
                    result: Some(ToolResult {
                        output: "raw verbose output".into(),
                        failed: Some(false),
                        exit_code: Some(0),
                        omitted_lines: 0,
                        output_truncated: false,
                    }),
                    tool_kind: ToolKind::Edit,
                    status: ToolStatus::Completed,
                    summary: "Edited checkout/total.ts +2 -1".into(),
                    locations: Vec::new(),
                    diff: Some(ToolDiff {
                        path: "checkout/total.ts".into(),
                        old_text: Some("old diff secret".into()),
                        new_text: "new diff secret".into(),
                        truncated: false,
                    }),
                },
                Block::Permission {
                    permission_id: "p1".into(),
                    title: "Allow shell".into(),
                    tool_kind: ToolKind::Command,
                    summary: "rm -rf".into(),
                    detail: Some("perm detail".into()),
                    options: vec![PermissionOption {
                        option_id: "allow".into(),
                        name: "Allow once".into(),
                        kind: PermissionOptionKind::AllowOnce,
                        persists_in_agent_settings: false,
                    }],
                    status: PermissionStatus::Delivered,
                    selected_option_id: Some("allow".into()),
                },
                Block::Text {
                    text: "Moved rounding after discount.".into(),
                },
            ],
            state: TurnState::Completed,
        }];

        let handoff = build_handoff(&turns, None, Path::new("/repo")).unwrap();
        assert!(handoff.contains("Fix checkout rounding"));
        assert!(handoff.contains("Edited checkout/total.ts +2 -1"));
        assert!(handoff.contains("Moved rounding after discount."));
        assert!(!handoff.contains("secret internal thought"));
        assert!(!handoff.contains("raw verbose output"));
        assert!(!handoff.contains("old diff secret"));
        assert!(!handoff.contains("perm detail"));
        assert!(handoff.len() <= HANDOFF_MAX_BYTES);
    }

    #[test]
    fn handoff_respects_128_kib_cap_keeping_newest_turns() {
        let large_text = "x".repeat(40 * 1024);
        let turns: Vec<Turn> = (1..=6)
            .map(|i| Turn {
                user: format!("Prompt {i}"),
                blocks: vec![Block::Text {
                    text: format!("Answer {i}: {large_text}"),
                }],
                state: TurnState::Completed,
            })
            .collect();
        let handoff = build_handoff(&turns, None, Path::new("/repo")).unwrap();
        assert!(handoff.len() <= HANDOFF_MAX_BYTES);
        assert!(handoff.contains("Prompt 6"));
        assert!(!handoff.contains("Prompt 1"));
    }
}
