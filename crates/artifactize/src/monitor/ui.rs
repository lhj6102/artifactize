use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Cell, Paragraph, Row, Table, Wrap},
};
use time::OffsetDateTime;

use super::{
    App, Screen,
    view::{EvalRow, Progress, RunRow, text},
};

pub(super) fn draw(frame: &mut Frame, app: &mut App, now: OffsetDateTime) {
    let [header, body, error, keys] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(0),
        Constraint::Length(u16::from(app.error.is_some())),
        Constraint::Length(2),
    ])
    .areas(frame.area());
    let scope = app.repo.as_ref().map_or_else(
        || "all repositories".into(),
        |repo| text(&repo.to_string_lossy()),
    );
    frame.render_widget(
        Paragraph::new(format!(
            "artifactize monitor | read-only | {scope}\nLast refresh: {} | every 1s",
            app.last_refresh.as_deref().unwrap_or("not yet refreshed")
        )),
        header,
    );
    match &app.screen {
        Screen::Runs => runs(frame, body, app, now),
        Screen::Progress(_) => {
            if let Some(saved) = &app.progress {
                let progress = Progress::new(saved, now);
                let block = Block::bordered().title("Run progress (saved state)");
                let inner = block.inner(body);
                let paragraph =
                    Paragraph::new(progress_lines(&progress)).wrap(Wrap { trim: false });
                let maximum = paragraph
                    .line_count(inner.width)
                    .saturating_sub(inner.height as usize);
                app.scroll = app.scroll.min(maximum.min(u16::MAX as usize) as u16);
                frame.render_widget(paragraph.block(block).scroll((app.scroll, 0)), body);
            }
        }
    }
    if let Some(message) = &app.error {
        frame.render_widget(
            Paragraph::new(format!("Read error (showing last-known data): {message}")),
            error,
        );
    }
    let help = match app.screen {
        Screen::Runs => {
            "j/k Up/Down: select | Enter: progress | n/p PgDn/PgUp: older/newer Runs\nr: refresh | q/Esc: quit"
        }
        Screen::Progress(_) => {
            "j/k Up/Down PgDn/PgUp: scroll | b/Backspace/Left: Runs\nr: refresh | q/Esc: quit | Human review: artifactize request (CLI only)"
        }
    };
    frame.render_widget(Paragraph::new(help), keys);
}

fn runs(frame: &mut Frame, area: Rect, app: &mut App, now: OffsetDateTime) {
    let title = format!(
        "Runs | page {}{}",
        app.offset / super::PAGE_SIZE + 1,
        if app.more { " | older available" } else { "" }
    );
    let block = Block::bordered().title(title);
    if app.runs.is_empty() {
        frame.render_widget(
            Paragraph::new("No saved Runs on this page.").block(block),
            area,
        );
        return;
    }
    let rows = app.runs.iter().map(|run| {
        let run = RunRow::new(run, now);
        let height = run.counts.len().clamp(1, u16::MAX as usize) as u16;
        let counts = run
            .counts
            .iter()
            .map(|(status, count)| format!("{}={count}", text(status)))
            .collect::<Vec<_>>()
            .join("\n");
        Row::new(vec![
            Cell::from(text(&run.id)),
            Cell::from(run.repo),
            Cell::from(run.status),
            Cell::from(counts),
            Cell::from(run.age),
        ])
        .height(height)
    });
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(28),
            Constraint::Percentage(24),
            Constraint::Length(17),
            Constraint::Min(18),
            Constraint::Length(8),
        ],
    )
    .header(
        Row::new(["ID", "REPO", "STATUS", "COUNTS", "AGE"])
            .style(Style::new().add_modifier(Modifier::BOLD)),
    )
    .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED))
    .highlight_symbol("> ")
    .block(block);
    frame.render_stateful_widget(table, area, &mut app.selection);
}

fn progress_lines(progress: &Progress) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(format!("Run: {}", progress.id)),
        Line::from(format!("Repo: {}", progress.repo)),
        Line::from(format!(
            "Status: {} | duration: {} | saved satisfaction: {}",
            progress.status,
            progress.duration,
            match progress.satisfied {
                Some(true) => "SATISFIED",
                Some(false) => "NOT SATISFIED",
                None => "not recorded",
            }
        )),
        Line::from(format!(
            "Created: {} | completed: {}",
            progress.created_at,
            progress.completed_at.as_deref().unwrap_or("-")
        )),
    ];
    if let Some(error) = &progress.error {
        lines.push(Line::from(format!("Run error: {error}")));
    }
    lines.push(Line::from(""));
    lines.push(Line::from("Request state counts").style(Style::new().add_modifier(Modifier::BOLD)));
    lines.push(Line::from(
        progress
            .counts
            .iter()
            .map(|(status, count)| format!("{status}={count}"))
            .collect::<Vec<_>>()
            .join("  "),
    ));
    section(
        &mut lines,
        "Running evals (elapsed)",
        &progress.running,
        false,
    );
    section(
        &mut lines,
        "Waiting Human requests (elapsed)",
        &progress.waiting,
        true,
    );
    section(
        &mut lines,
        "Other evals (duration, errors and waiting reasons)",
        &progress.other,
        false,
    );
    lines
}

fn section(lines: &mut Vec<Line<'static>>, title: &str, rows: &[EvalRow], request_ids: bool) {
    lines.push(Line::from(""));
    lines.push(Line::from(title.to_owned()).style(Style::new().add_modifier(Modifier::BOLD)));
    if rows.is_empty() {
        lines.push(Line::from("  (none)"));
    }
    for row in rows {
        lines.push(Line::from(format!(
            "  {} | {} | {}",
            row.id, row.status, row.duration
        )));
        if request_ids {
            lines.push(Line::from(format!("    request: {}", row.request)));
        }
        if let Some(error) = &row.error {
            lines.push(Line::from(format!("    error: {error}")));
        }
        if let Some(reason) = &row.reason {
            lines.push(Line::from(format!("    reason: {reason}")));
        }
    }
}
