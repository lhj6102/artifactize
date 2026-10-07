//! Ratatui rendering only; semantic selection and review lifecycle live in their controllers.
use super::{
    Monitor, Pane,
    input::{Button, Hits},
    model,
};
use crate::review::Control;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Cell, Clear, Paragraph, Row, Table, Wrap},
};
use time::OffsetDateTime;
use tui_tree_widget::{Tree, TreeItem};

/// Reserve most of the terminal for the three panes; the modal keeps a small visible border.
const MODAL_MARGIN: u16 = 2;
/// Buttons stay two columns apart so adjacent hit areas never overlap.
const BUTTON_GAP: u16 = 2;

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
fn lines(detail: model::Detail) -> Vec<Line<'static>> {
    detail
        .fields
        .into_iter()
        .flat_map(|(key, value)| {
            let mut values = value.lines();
            std::iter::once(Line::from(vec![
                Span::styled(format!("{key}: "), Modifier::BOLD),
                Span::raw(values.next().unwrap_or_default().to_owned()),
            ]))
            .chain(values.map(|line| Line::from(format!("  {line}"))))
            .collect::<Vec<_>>()
        })
        .collect()
}
fn block(title: String, focused: bool) -> Block<'static> {
    Block::bordered().title(title).border_style(if focused {
        Style::new().cyan()
    } else {
        Style::default()
    })
}
impl Monitor {
    pub fn draw(&mut self, frame: &mut Frame) {
        self.hits = Hits::default();
        let now = OffsetDateTime::now_utc();
        let [header, body, notice, keys] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(u16::from(self.error.is_some() || self.notice.is_some())),
            Constraint::Length(1),
        ])
        .areas(frame.area());
        let refreshed = self.refreshed.map_or("loading".into(), |time| {
            format!("refreshed {}", clock(time))
        });
        frame.render_widget(
            Line::from(vec![
                "artifactize monitor".bold(),
                format!(
                    "  all repositories · {refreshed} · mouse {} (F2)",
                    if self.mouse_capture { "on" } else { "off" }
                )
                .into(),
            ]),
            header,
        );
        if let Some(message) = self.error.as_ref().or(self.notice.as_ref()) {
            frame.render_widget(Line::from(message.as_str()).red(), notice);
        }
        frame.render_widget(Line::from(if self.modal.is_some() { "Esc close/cancel · Tab focus · Ctrl-S Submit · F2 mouse capture · paste works with mouse on/off" } else { "Tab panes · ↑/↓ select · ←/→ tree · Enter/double-click detail · Space expand · r refresh · F2 mouse · q quit" }).dark_gray(), keys);
        let [repositories, runs, artifacts] = Layout::horizontal([
            Constraint::Percentage(25),
            Constraint::Percentage(30),
            Constraint::Fill(1),
        ])
        .areas(body);
        self.hits.panes = [repositories, runs, artifacts];
        let rows = self.catalog.rows.iter().map(|row| {
            Row::new([format!(
                "{}{}{}",
                "  ".repeat(row.depth),
                row.label,
                row.badge.text()
            )])
        });
        frame.render_stateful_widget(
            Table::new(rows, [Constraint::Fill(1)])
                .row_highlight_style(Modifier::REVERSED)
                .block(block(
                    " Repository / worktree ".into(),
                    self.focus == Pane::Repositories,
                )),
            repositories,
            &mut self.repositories,
        );
        self.draw_runs(frame, runs, now);
        self.draw_artifacts(frame, artifacts, now);
        if self.modal.is_some() {
            self.draw_modal(frame, body);
        }
    }
    fn draw_runs(&mut self, frame: &mut Frame, area: Rect, now: OffsetDateTime) {
        let block = block(
            format!(" Runs ({}) ", self.runs.len()),
            self.focus == Pane::Runs,
        );
        if self.runs.is_empty() {
            frame.render_widget(
                Paragraph::new(if self.refreshed.is_some() {
                    "No saved Runs. Start one with `artifactize verify`."
                } else {
                    "Loading…"
                })
                .wrap(Wrap { trim: false })
                .block(block),
                area,
            );
            return;
        }
        let rows = model::run_rows(&self.runs, now).into_iter().map(|row| {
            Row::new([
                Cell::from(row.id),
                Cell::from(status(&row.status)),
                Cell::from(format!("{} · {}", row.age, row.repo)),
            ])
        });
        frame.render_stateful_widget(
            Table::new(
                rows,
                [
                    Constraint::Length(12),
                    Constraint::Length(13),
                    Constraint::Fill(1),
                ],
            )
            .header(Row::new(["RUN", "STATUS", "AGE / WORKSPACE"]).bold())
            .row_highlight_style(Modifier::REVERSED)
            .block(block),
            area,
            &mut self.list,
        );
    }
    fn draw_artifacts(&mut self, frame: &mut Frame, area: Rect, now: OffsetDateTime) {
        let Some((run, requests)) = &self.run else {
            frame.render_widget(
                Paragraph::new("Select a Run.").block(block(
                    " Artifacts and evals ".into(),
                    self.focus == Pane::Artifacts,
                )),
                area,
            );
            return;
        };
        let progress = model::progress(run, requests, now);
        let mut summary = vec![
            Line::from(vec![
                status(&progress.status),
                format!(" · validation {}", progress.validation).into(),
            ]),
            Line::from(progress.work),
        ];
        summary.extend(
            progress
                .counts
                .into_iter()
                .map(|(status, count)| Line::from(format!("{status} {count}"))),
        );
        summary.extend(
            progress
                .running
                .into_iter()
                .map(|(eval, time)| Line::from(format!("running {eval} · {time}"))),
        );
        summary.extend(
            progress
                .waiting
                .into_iter()
                .map(|(eval, claim)| Line::from(format!("waiting Human {eval} · {claim}"))),
        );
        let height = (summary.len() as u16 + 2).min(area.height / 2);
        let [top, tree_area] =
            Layout::vertical([Constraint::Length(height), Constraint::Fill(1)]).areas(area);
        frame.render_widget(
            Paragraph::new(summary)
                .wrap(Wrap { trim: false })
                .block(Block::bordered().title(format!(" Run {} ", run.run.id))),
            top,
        );
        self.hits.panes[2] = tree_area;
        let items = model::tree(run, requests, now)
            .into_iter()
            .map(item)
            .collect::<Result<Vec<_>, _>>();
        match items
            .as_ref()
            .map_err(ToString::to_string)
            .and_then(|items| Tree::new(items).map_err(|error| error.to_string()))
        {
            Ok(tree) => frame.render_stateful_widget(
                tree.block(block(
                    " Artifacts and evals ".into(),
                    self.focus == Pane::Artifacts,
                ))
                .highlight_style(Modifier::REVERSED.into()),
                tree_area,
                &mut self.tree,
            ),
            Err(error) => frame.render_widget(Paragraph::new(error).red(), tree_area),
        }
    }
    fn draw_modal(&mut self, frame: &mut Frame, area: Rect) {
        let Some(modal) = &mut self.modal else {
            return;
        };
        let margin = MODAL_MARGIN.min(area.width / 2).min(area.height / 2);
        let area = Rect::new(
            area.x + margin,
            area.y + margin,
            area.width.saturating_sub(margin * 2),
            area.height.saturating_sub(margin * 2),
        );
        frame.render_widget(Clear, area);
        self.hits.modal = area;
        let detail = modal.detail.clone();
        let outer = Block::bordered()
            .title(format!(
                " {}{} ",
                detail.title,
                modal
                    .request
                    .as_ref()
                    .map_or(String::new(), |id| format!(" · {id}"))
            ))
            .cyan();
        let inner = outer.inner(area);
        frame.render_widget(outer, area);
        let [content, buttons] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(inner);
        if let Some(review) = &mut modal.review {
            let (tools, fields, instruction) =
                review.draw_single(frame, content, modal.focus == super::ModalPane::Tools);
            self.hits.instruction = instruction;
            self.hits.tools = tools;
            self.hits.field_rows = review.field_hits(fields);
            let [left, right] =
                Layout::horizontal([Constraint::Percentage(45), Constraint::Fill(1)])
                    .areas(content);
            self.hits.modal_panes = [left, right];
            let controls = if review.confirming() {
                vec![
                    ("Confirm", Button::Review(Control::Confirm)),
                    ("Cancel", Button::Review(Control::Cancel)),
                ]
            } else if review.settled() {
                Vec::new()
            } else if review.owned() {
                vec![
                    ("Release", Button::Review(Control::Release)),
                    ("GREEN", Button::Review(Control::Green)),
                    ("RED", Button::Review(Control::Red)),
                    ("Submit", Button::Review(Control::Submit)),
                    ("Run tool", Button::Review(Control::RunTool)),
                ]
            } else {
                vec![("Claim", Button::Review(Control::Claim))]
            };
            self.draw_buttons(frame, buttons, controls);
        } else {
            let [left, right] =
                Layout::horizontal([Constraint::Percentage(45), Constraint::Fill(1)])
                    .areas(content);
            self.hits.modal_panes = [left, right];
            frame.render_widget(
                Paragraph::new(lines(detail))
                    .wrap(Wrap { trim: false })
                    .scroll((modal.scroll[0], 0))
                    .block(Block::bordered().title(" Summary / result ")),
                left,
            );
            frame.render_widget(
                Paragraph::new(modal.evidence.text.clone())
                    .wrap(Wrap { trim: false })
                    .scroll((modal.scroll[1], 0))
                    .block(Block::bordered().title(format!(" {} ", modal.evidence.title))),
                right,
            );
            self.draw_buttons(frame, buttons, Vec::new());
        }
    }
    fn draw_buttons(&mut self, frame: &mut Frame, area: Rect, mut controls: Vec<(&str, Button)>) {
        controls.push(("Close", Button::Close));
        let mut x = area.x;
        for (label, button) in controls {
            let width = label.len() as u16 + 2;
            if x.saturating_add(width) > area.right() {
                break;
            }
            let rect = Rect::new(x, area.y, width, area.height);
            frame.render_widget(Line::from(format!("[{label}]")).bold().cyan(), rect);
            self.hits.buttons.push((rect, button));
            x = x.saturating_add(width).saturating_add(BUTTON_GAP);
        }
    }
}
