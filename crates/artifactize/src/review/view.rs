//! Drawing only; text comes from the review state and the saved request. The standalone screen
//! uses monitor's reactive layout: the waiting list, then the shared Human review Detail.

use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Cell, Clear, Paragraph, Row, Table, Wrap},
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{Focus, Form, Input, Mode, Review, detail};
use crate::monitor::{
    crumbs, duration, fit,
    layout::{self, Density, Level},
    plain, width,
};

/// Keep a transient notice inside the fixed four-line footer rather than covering review content.
const MAX_NOTICE_LINES: usize = 4;
/// The Compact waiting list, borders included, beside a Full Detail.
const LIST_COMPACT: u16 = 30;
/// Fixed columns of the Full waiting list: claim and waiting time.
const CLAIM_WIDTH: u16 = 16;
const WAITING_WIDTH: u16 = 8;
/// A long repository path counts only this far towards the Full list's width, so one deep
/// checkout does not push the Preview off a wide terminal.
const REPO_WIDTH: u16 = 40;
/// The Full waiting list's column titles; a column is never narrower than its title.
const HEADERS: [&str; 5] = ["EVAL", "REQUEST", "CLAIM", "WAITING", "REPO"];

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub(super) fn spinner(elapsed: std::time::Duration) -> &'static str {
    SPINNER[(elapsed.as_millis() / super::SPIN.as_millis()) as usize % SPINNER.len()]
}

pub(super) fn lines(fields: Vec<(&'static str, String)>) -> Vec<Line<'static>> {
    let mut text = Vec::new();
    for (key, value) in fields {
        let mut values = value.lines();
        text.push(Line::from(vec![
            Span::styled(format!("{key}: "), Modifier::BOLD),
            values.next().unwrap_or_default().to_owned().into(),
        ]));
        text.extend(values.map(|line| Line::from(format!("  {line}"))));
    }
    text
}

fn popup(frame: &mut Frame, area: Rect, title: String, text: Vec<Line<'static>>) {
    let height = (text.len() as u16 + 2).min(area.height);
    let [area] = Layout::horizontal([Constraint::Percentage(80)])
        .flex(Flex::Center)
        .areas(area);
    let [area] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(area);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .block(Block::bordered().title(title).cyan()),
        area,
    );
}

/// Geometry of the last drawn standalone frame for mouse hit tests.
#[derive(Debug, Clone, Default)]
pub(super) struct Hits {
    /// The waiting list, Full or Compact, and its visible rows by index.
    pub list: Rect,
    pub rows: Vec<(Rect, usize)>,
    /// The read-only Preview beside a focused list; a click opens the selected request.
    pub preview: Rect,
    /// The Full Detail and the shared component's own geometry.
    pub detail: Rect,
    pub review: detail::Hits,
}

fn text_width(text: &str) -> u16 {
    u16::try_from(width(text)).unwrap_or(u16::MAX)
}

impl Review {
    pub fn draw(&mut self, frame: &mut Frame) {
        let now = OffsetDateTime::now_utc();
        // With Detail focused the component shows the status itself.
        let status = if self.focus == Focus::List {
            self.status_line()
        } else {
            Vec::new()
        };
        let [header, body, status_area, keys] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(status.len() as u16),
            Constraint::Length(1),
        ])
        .areas(frame.area());
        self.draw_header(frame, header);
        frame.render_widget(Paragraph::new(status), status_area);
        frame.render_widget(Line::from(self.help()).dark_gray(), keys);
        let rows = self.list_rows(now);
        let levels = [
            Level {
                compact: LIST_COMPACT,
                natural: list_width(&rows),
            },
            Level {
                compact: 0,
                natural: u16::MAX,
            },
        ];
        let focused = usize::from(self.focus == Focus::Detail);
        let mut hits = Hits::default();
        for (level, area, density) in layout::columns(body, &levels, focused) {
            match (level, density) {
                (0, density) => {
                    hits.list = area;
                    hits.rows = self.draw_list(frame, area, density, rows.clone());
                }
                (_, Density::Full) => {
                    hits.detail = area;
                    hits.review = self.draw_detail(frame, area, true);
                }
                _ => {
                    hits.preview = area;
                    self.draw_preview(frame, area);
                }
            }
        }
        self.hits = hits;
        if self.mode == Mode::Leave {
            let mut text = vec![
                Line::from(format!(
                    "This session claimed {} request(s) without submitting:",
                    self.taken.len()
                ))
                .bold(),
            ];
            text.extend(self.taken.iter().map(|id| Line::from(format!("  {id}"))));
            text.extend([
                Line::default(),
                Line::from("k keep the claims and quit · u release them and quit · Esc stay"),
            ]);
            popup(frame, body, " Quit ".into(), text);
        }
    }

    /// `artifactize review › scope › eval` and the reviewer.
    fn draw_header(&self, frame: &mut Frame, area: Rect) {
        let scope = self
            .repo
            .as_ref()
            .map_or("all repositories".into(), |repo| {
                format!("repo {}", repo.display())
            });
        let mut segments = vec![scope];
        if self.focus == Focus::Detail {
            match (&self.request, &self.open, &self.invalid_open) {
                (Some(view), _, _) => segments.push(view.request.eval_id.clone()),
                (None, Some(id), _) => segments.push(id.to_string()),
                (None, None, Some(id)) => segments.push(id.clone()),
                _ => {}
            }
        }
        let segments: Vec<String> = segments
            .iter()
            .map(|segment| plain(segment).into_owned())
            .collect();
        let mut right = format!("reviewer {}", self.reviewer);
        if self.refreshed.is_none() {
            right = format!("loading… · {right}");
        }
        // As in monitor: only the off state needs saying.
        if !self.mouse_capture {
            right = format!("{right} · mouse off · F2");
        }
        let right = Line::from(right).dark_gray();
        let room = usize::from(area.width).saturating_sub(right.width() + 2);
        let [left, end] = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Length(u16::try_from(right.width()).unwrap_or(u16::MAX)),
        ])
        .areas(area);
        frame.render_widget(
            Line::from(crumbs("artifactize review", &segments, room)).bold(),
            left,
        );
        frame.render_widget(right, end);
    }

    pub(super) fn status_line(&self) -> Vec<Line<'static>> {
        if let Some(busy) = &self.busy {
            let elapsed = busy.since.elapsed();
            let frame = spinner(elapsed);
            return vec![
                Line::from(format!(
                    "{frame} {}… {}",
                    busy.label,
                    duration(elapsed.as_secs() as i64)
                ))
                .yellow(),
            ];
        }
        let (message, error) = match (&self.error, &self.notice) {
            (Some(error), _) => (error.as_str(), true),
            (None, Some((notice, error))) => (notice.as_str(), *error),
            (None, None) => return Vec::new(),
        };
        let style = if error { Color::Red } else { Color::Green };
        message
            .lines()
            .take(MAX_NOTICE_LINES)
            .map(|line| Line::from(line.to_owned()).fg(style))
            .collect()
    }

    fn help(&self) -> String {
        if self.mode == Mode::Leave {
            return "k keep claims and quit · u release and quit · Esc stay".into();
        }
        match self.focus {
            Focus::List => "↑↓ request · Enter open review · Tab tools · r refresh · q quit".into(),
            Focus::Detail => self.detail_hints(),
        }
    }

    /// Cells of the waiting list: eval, request, claim, waiting time and repository, as one-row
    /// [`plain`] text so widths are measured on what the terminal shows.
    fn list_rows(&self, now: OffsetDateTime) -> Vec<[String; 5]> {
        self.waiting
            .iter()
            .map(|view| {
                let request = &view.request;
                let claim = view.claim.as_ref().map_or("-".into(), |claim| {
                    if claim.reviewer == self.reviewer {
                        format!("{} (you)", claim.reviewer)
                    } else {
                        claim.reviewer.clone()
                    }
                });
                let age = OffsetDateTime::parse(&request.created_at, &Rfc3339)
                    .map(|created| duration((now - created).whole_seconds()))
                    .unwrap_or_default();
                let repo = request.human_definition.as_ref();
                let repo = repo.and_then(|definition| definition["repo"].as_str());
                [
                    request.eval_id.clone(),
                    request.id.to_string(),
                    claim,
                    age,
                    repo.unwrap_or("-").to_owned(),
                ]
                .map(|cell| plain(&cell).into_owned())
            })
            .collect()
    }

    fn draw_list(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        density: Density,
        rows: Vec<[String; 5]>,
    ) -> Vec<(Rect, usize)> {
        let focused = density == Density::Full;
        let block = Block::bordered()
            .title(format!(" Waiting Human reviews ({}) ", self.waiting.len()))
            .border_style(if focused {
                Style::new().cyan()
            } else {
                Style::default()
            });
        if self.waiting.is_empty() {
            let text = if self.refreshed.is_some() {
                "No Human reviews are waiting."
            } else {
                "Loading…"
            };
            frame.render_widget(
                Paragraph::new(text).wrap(Wrap { trim: false }).block(block),
                area,
            );
            return Vec::new();
        }
        // The selected row is reversed only where focus is, and bold elsewhere.
        let highlight = if focused {
            Modifier::REVERSED
        } else {
            Modifier::BOLD
        };
        if density == Density::Compact {
            let room = usize::from(area.width.saturating_sub(2));
            let rows = rows.into_iter().zip(&self.waiting).map(|(cells, view)| {
                let mark = match &view.claim {
                    Some(claim) if claim.reviewer == self.reviewer => " (you)",
                    Some(_) => " (claimed)",
                    None => "",
                };
                Row::new([Cell::from(fit(&format!("? {}{mark}", cells[0]), room))])
            });
            let table = Table::new(rows, [Constraint::Fill(1)])
                .row_highlight_style(highlight)
                .block(block);
            frame.render_stateful_widget(table, area, &mut self.list);
            return self.row_hits(area, 0);
        }
        let [eval, request, claim, waiting, _] = column_widths(&rows);
        let table = Table::new(
            rows.into_iter().map(Row::new),
            [
                Constraint::Length(eval),
                Constraint::Length(request),
                Constraint::Length(claim),
                Constraint::Length(waiting),
                Constraint::Fill(1),
            ],
        )
        .header(Row::new(HEADERS).bold())
        .row_highlight_style(highlight)
        .block(block);
        frame.render_stateful_widget(table, area, &mut self.list);
        self.row_hits(area, 1)
    }

    /// The visible list rows inside the bordered `area`, below `header` rows, from the table's
    /// scroll offset after rendering.
    fn row_hits(&self, area: Rect, header: u16) -> Vec<(Rect, usize)> {
        let inner = area.inner(ratatui::layout::Margin::new(1, 1));
        let top = inner.y.saturating_add(header);
        (self.list.offset()..self.waiting.len())
            .zip(top..inner.bottom())
            .map(|(index, y)| (Rect::new(inner.x, y, inner.width, 1), index))
            .collect()
    }

    /// The Detail Preview of the selected request, read-only.
    fn draw_preview(&self, frame: &mut Frame, area: Rect) {
        match self
            .list
            .selected()
            .and_then(|index| self.waiting.get(index))
        {
            Some(view) => detail::draw_peek(frame, area, view, &self.reviewer),
            None => frame.render_widget(
                Paragraph::new("Select a request.")
                    .dark_gray()
                    .block(Block::bordered().title(" Detail ")),
                area,
            ),
        }
    }

    pub(super) fn draw_output(&self, frame: &mut Frame, area: Rect) {
        let Some(output) = &self.output else {
            frame.render_widget(
                Paragraph::new(
                    "Enter runs the selected tool. A launch tool opens on its own; \
                     an output tool's text appears here.",
                )
                .wrap(Wrap { trim: false })
                .dark_gray()
                .block(Block::bordered().title(" Output ")),
                area,
            );
            return;
        };
        let title = format!(" Output · {} ", output.title);
        let block = if output.error {
            Block::bordered().title(Span::styled(title, Style::new().fg(Color::Red)))
        } else {
            Block::bordered().title(title)
        };
        frame.render_widget(
            Paragraph::new(output.text.clone())
                .wrap(Wrap { trim: false })
                .scroll((self.scroll, 0))
                .block(block),
            area,
        );
    }
}

/// Widths of the Full list's columns; claim and waiting time keep fixed widths, and a long
/// repository path counts only up to `REPO_WIDTH` towards the natural width.
fn column_widths(rows: &[[String; 5]]) -> [u16; 5] {
    let mut widths = HEADERS.map(text_width);
    widths[2] = CLAIM_WIDTH;
    widths[3] = WAITING_WIDTH;
    for row in rows {
        for column in [0, 1, 4] {
            widths[column] = widths[column].max(text_width(&row[column]));
        }
    }
    widths[4] = widths[4].min(REPO_WIDTH);
    widths
}

/// The Full list grows to its columns, so the rest of a wide terminal previews the request.
fn list_width(rows: &[[String; 5]]) -> u16 {
    let widths = column_widths(rows);
    widths.iter().sum::<u16>() + widths.len() as u16 - 1 + 2
}

fn field_line(field: &super::Field, selected: bool) -> Line<'static> {
    let marker = if field.required { "*" } else { "" };
    let mut value = field.display();
    if selected
        && matches!(
            field.input,
            Input::Text(_) | Input::Integer(_) | Input::Number(_)
        )
    {
        value.push('▏');
    }
    let line = Line::from(vec![
        Span::styled(format!("{}{marker}: ", field.name), Modifier::BOLD),
        Span::raw(value),
        Span::styled(format!("   {}", field.hint), Style::new().dark_gray()),
    ]);
    let line = if selected {
        line.add_modifier(Modifier::REVERSED)
    } else {
        line
    };
    if matches!(field.input, Input::Fixed(_)) {
        line.dark_gray()
    } else {
        line
    }
}

/// Same ratatui wrapping as rendering, so a continuation line belongs to its actual field.
pub(super) fn field_hits(area: Rect, form: &Form, scroll: u16) -> Vec<(Rect, usize)> {
    if form.json.is_some() {
        return Vec::new();
    }
    let inner = Block::bordered().inner(area);
    let mut row = 0usize;
    form.fields
        .iter()
        .enumerate()
        .filter_map(|(index, field)| {
            let height = Paragraph::new(field_line(field, index == form.selected))
                .wrap(Wrap { trim: false })
                .line_count(inner.width);
            let first = row.saturating_sub(usize::from(scroll));
            let last = row
                .saturating_add(height)
                .saturating_sub(usize::from(scroll))
                .min(usize::from(inner.height));
            let visible = row.saturating_add(height) > usize::from(scroll) && first < last;
            row += height;
            visible.then(|| {
                (
                    Rect::new(
                        inner.x,
                        inner.y + first as u16,
                        inner.width,
                        (last - first) as u16,
                    ),
                    index,
                )
            })
        })
        .collect()
}

pub(super) fn draw_inline_form(
    frame: &mut Frame,
    area: Rect,
    form: &Form,
    scroll: u16,
    focused: bool,
) {
    let border = if focused {
        Style::new().cyan()
    } else {
        Style::default()
    };
    let Some(json) = &form.json else {
        draw_fields(frame, area, form, scroll, border);
        return;
    };
    // Boundary-safe even for a cursor left inside a multibyte character.
    let cursor = super::form::boundary(json, form.cursor);
    let before = &json[..cursor];
    let row = before
        .chars()
        .filter(|character| *character == '\n')
        .count();
    let height = usize::from(area.height.saturating_sub(3));
    let scroll = if scroll == 0 {
        row.saturating_sub(height.saturating_sub(1))
    } else {
        usize::from(scroll)
    };
    let column = Line::from(before.rsplit('\n').next().unwrap_or_default()).width();
    let visible_width = usize::from(area.width.saturating_sub(3));
    let horizontal = column.saturating_sub(visible_width.saturating_sub(1));
    let mut text = format!("{}▏{}", before, &json[cursor..]);
    if let Some(error) = &form.error {
        text.push_str(&format!("\n{error}"));
    }
    frame.render_widget(
        Paragraph::new(text)
            .scroll((
                u16::try_from(scroll).unwrap_or(u16::MAX),
                u16::try_from(horizontal).unwrap_or(u16::MAX),
            ))
            .block(
                Block::bordered()
                    .title(format!(
                        " {} JSON · Enter newline · Ctrl-S submit ",
                        form.verdict
                    ))
                    .border_style(border),
            ),
        area,
    );
}

fn draw_fields(frame: &mut Frame, area: Rect, form: &Form, scroll: u16, border: Style) {
    let color = if form.verdict == "GREEN" {
        Color::Green
    } else {
        Color::Red
    };
    let block = Block::bordered()
        .title(Span::styled(
            format!(" {} fields ", form.verdict),
            Style::new().fg(color),
        ))
        .border_style(border);
    let mut text = Vec::new();
    if form.fields.is_empty() {
        text.push(Line::from(
            "No owner fields; Ctrl-S or Submit records this verdict.",
        ));
    }
    for (index, field) in form.fields.iter().enumerate() {
        text.push(field_line(field, index == form.selected));
    }
    if let Some(error) = &form.error {
        text.push(Line::default());
        text.extend(error.lines().map(|line| Line::from(line.to_owned()).red()));
    }
    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0))
            .block(block),
        area,
    );
}
