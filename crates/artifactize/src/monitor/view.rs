//! Drawing only; all text comes from the view-model.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Cell, Paragraph, Row, Table, Wrap},
};
use time::OffsetDateTime;
use tui_tree_widget::{Tree, TreeItem};

use super::{Monitor, model};

pub(crate) fn clock(time: OffsetDateTime) -> String {
    format!(
        "{:02}:{:02}:{:02} UTC",
        time.hour(),
        time.minute(),
        time.second()
    )
}

fn color(status: Option<&str>) -> Style {
    Style::new().fg(match status {
        Some("GREEN") => Color::Green,
        Some("RED") => Color::Red,
        Some("ERROR") => Color::Magenta,
        Some("RUNNING") => Color::Yellow,
        Some("WAITING_HUMAN") => Color::Cyan,
        Some("BLOCKED" | "BUDGET_EXHAUSTED" | "INCOMPLETE") => Color::LightRed,
        _ => Color::Gray,
    })
}

fn status(status: &str) -> Span<'static> {
    Span::styled(status.to_owned(), color(Some(status)))
}

fn item(node: model::Node) -> std::io::Result<TreeItem<'static, String>> {
    let text = Line::from(vec![
        Span::styled(
            format!("{} ", model::glyph(node.status.as_deref())),
            color(node.status.as_deref()),
        ),
        Span::raw(node.text),
    ]);
    let children = node
        .children
        .into_iter()
        .map(item)
        .collect::<Result<_, _>>()?;
    TreeItem::new(node.id, text, children)
}

impl Monitor {
    pub fn draw(&mut self, frame: &mut Frame) {
        let now = OffsetDateTime::now_utc();
        let [header, body, error, keys] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(u16::from(self.error.is_some() || self.notice.is_some())),
            Constraint::Length(1),
        ])
        .areas(frame.area());
        let scope = self
            .repo
            .as_ref()
            .map_or("all repositories".into(), |repo| {
                format!("repo {}", repo.display())
            });
        let refreshed = self.refreshed.map_or("loading".into(), |time| {
            format!("refreshed {}", clock(time))
        });
        frame.render_widget(
            Line::from(vec![
                "artifactize monitor".bold(),
                format!("  {scope} · state {} · {refreshed}", self.state.display()).into(),
            ]),
            header,
        );
        if let Some(message) = self.error.as_ref().or(self.notice.as_ref()) {
            frame.render_widget(Line::from(message.as_str()).red(), error);
        }
        let help = if self.open.is_some() {
            "j/k ↑/↓ move · h/l ←/→ collapse/expand · Enter toggle · o review waiting Human · PgUp/PgDn scroll detail · r refresh · Esc back · q quit"
        } else {
            "j/k ↑/↓ move · Enter open Run · r refresh · q/Esc quit"
        };
        frame.render_widget(Line::from(help).dark_gray(), keys);
        if self.open.is_some() {
            self.draw_run(frame, body, now);
        } else {
            self.draw_runs(frame, body, now);
        }
    }

    fn draw_runs(&mut self, frame: &mut Frame, area: Rect, now: OffsetDateTime) {
        let block = Block::bordered().title(format!(" Runs ({}) ", self.runs.len()));
        if self.runs.is_empty() {
            let text = if self.refreshed.is_some() {
                "No saved Runs. Start one with `artifactize verify`."
            } else {
                "Loading…"
            };
            frame.render_widget(Paragraph::new(text).block(block), area);
            return;
        }
        let rows = model::run_rows(&self.runs, now).into_iter().map(|row| {
            Row::new(vec![
                Cell::from(row.id),
                Cell::from(row.repo),
                Cell::from(status(&row.status)),
                Cell::from(row.counts),
                Cell::from(row.age),
            ])
        });
        let table = Table::new(
            rows,
            [
                Constraint::Length(12),
                Constraint::Fill(2),
                Constraint::Length(13),
                Constraint::Fill(3),
                Constraint::Length(8),
            ],
        )
        .header(Row::new(["RUN", "REPO", "STATUS", "COUNTS", "AGE"]).bold())
        .row_highlight_style(Modifier::REVERSED)
        .block(block);
        frame.render_stateful_widget(table, area, &mut self.list);
    }

    fn draw_run(&mut self, frame: &mut Frame, area: Rect, now: OffsetDateTime) {
        let Some((run, requests)) = &self.run else {
            frame.render_widget(Paragraph::new("Loading…").block(Block::bordered()), area);
            return;
        };
        let progress = model::progress(run, requests, now);
        let mut lines = vec![
            Line::from(vec![
                status(&progress.status),
                format!("  validation {} · {}", progress.validation, progress.timing).into(),
            ]),
            Line::from(format!("repo {}", progress.repo)),
            Line::from(
                progress
                    .counts
                    .iter()
                    .flat_map(|(state, count)| [status(state), format!(" {count}  ").into()])
                    .collect::<Vec<_>>(),
            ),
            Line::from(progress.work.clone()),
        ];
        for (label, style, items) in [
            ("running", color(Some("RUNNING")), &progress.running),
            (
                "waiting Human",
                color(Some("WAITING_HUMAN")),
                &progress.waiting,
            ),
            ("error", color(Some("ERROR")), &progress.errors),
        ] {
            lines.extend(items.iter().map(|(eval, note)| {
                Line::from(vec![
                    Span::styled(format!("{label} "), style),
                    format!("{eval} · {note}").into(),
                ])
            }));
        }
        let height = (lines.len() as u16 + 2).min(area.height / 2);
        let [top, bottom] =
            Layout::vertical([Constraint::Length(height), Constraint::Fill(1)]).areas(area);
        let [left, right] =
            Layout::horizontal([Constraint::Percentage(45), Constraint::Fill(1)]).areas(bottom);
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .block(Block::bordered().title(format!(" Run {} ", run.run.id))),
            top,
        );
        let items = model::tree(run, requests, now)
            .into_iter()
            .map(item)
            .collect::<Result<Vec<_>, _>>();
        let block = Block::bordered().title(" Artifacts and evals ");
        let tree = match &items {
            Ok(items) => Tree::new(items),
            Err(error) => Err(std::io::Error::other(error.to_string())),
        };
        match tree {
            Ok(tree) => frame.render_stateful_widget(
                tree.block(block)
                    .highlight_style(Style::new().add_modifier(Modifier::REVERSED)),
                left,
                &mut self.tree,
            ),
            Err(error) => {
                frame.render_widget(Paragraph::new(error.to_string()).red().block(block), left)
            }
        }
        let detail = self
            .target()
            .map(|target| model::detail(run, requests, &target, now))
            .unwrap_or_default();
        let mut text = Vec::new();
        for (key, value) in detail.fields {
            let mut values = value.lines();
            text.push(Line::from(vec![
                Span::styled(format!("{key}: "), Modifier::BOLD),
                values.next().unwrap_or_default().to_owned().into(),
            ]));
            text.extend(values.map(|line| Line::from(format!("  {line}"))));
        }
        frame.render_widget(
            Paragraph::new(text)
                .wrap(Wrap { trim: false })
                .scroll((self.scroll, 0))
                .block(Block::bordered().title(format!(" {} ", detail.title))),
            right,
        );
    }
}
