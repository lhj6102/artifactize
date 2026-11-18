//! Monitor's key reference and the help overlay that shows it.
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
};

/// The help popup stays this wide, so its key column and descriptions read as a table.
const HELP_WIDTH: u16 = 72;
/// The help popup reserves one row for each of its top and bottom borders.
const HELP_BORDER_ROWS: u16 = 2;
/// One more than "Tab / Shift-Tab", the longest key label in [`HELP`], so every label gets
/// at least one space before its text.
const HELP_KEY_WIDTH: usize = 16;

const HELP: &[(&str, &str)] = &[
    ("Panes", ""),
    ("→ / Enter", "next level: Scope → Runs → Run → Detail"),
    ("← / Esc", "previous level; never quits"),
    ("Tab / Shift-Tab", "cycle Scope, Runs and the Run tree"),
    ("↑↓ j k", "select; in Detail, scroll"),
    ("!", "next ERROR, RED or waiting Human eval"),
    ("r", "refresh now"),
    ("q / Ctrl-C", "quit"),
    ("Scope", ""),
    ("Space", "show or hide worktrees without Runs"),
    ("Run tree", ""),
    ("h / l / Space", "fold, unfold, toggle"),
    ("Enter on Run", "Run detail: usage, budgets and counts"),
    ("Detail", ""),
    ("t", "show or hide the Technical section"),
    (
        "w",
        "show or hide the pending evals of what an eval waits for",
    ),
    ("Tab", "sections and evidence; Human tools and fields"),
    ("d / Space", "Agent: details, tool groups"),
    ("c g r u", "Human: claim, GREEN, RED, release"),
    ("i", "Human: expand or fold the instruction"),
    (
        "Ctrl-S",
        "Human: submit the review; Ctrl-G/R pick a verdict while editing",
    ),
    ("Esc", "Human: stop editing, then step back"),
    ("Mouse", ""),
    ("click", "focus and select; double-click opens"),
    ("wheel", "scroll the pane under the pointer"),
    ("F2", "mouse capture on or off; paste works either way"),
];

pub(super) fn draw(frame: &mut Frame, area: Rect) {
    let lines: Vec<Line> = HELP
        .iter()
        .map(|(key, text)| {
            if text.is_empty() {
                Line::from(Span::styled(*key, Style::new().bold().cyan()))
            } else {
                Line::from(vec![
                    Span::styled(format!("  {key:<HELP_KEY_WIDTH$}"), Modifier::BOLD),
                    Span::raw(*text),
                ])
            }
        })
        .collect();
    let width = area.width.min(HELP_WIDTH);
    let height = area.height.min(lines.len() as u16 + HELP_BORDER_ROWS);
    let area = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(" Keys · any key closes ").cyan()),
        area,
    );
}
