//! Drawing only; text comes from the review state and the saved request. The standalone screen
//! uses monitor's reactive layout: the waiting list, then the shared Human review Detail.

use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Cell, Clear, Paragraph, Row, Table, Wrap},
};
use time::OffsetDateTime;

use super::{Focus, Form, Input, Mode, Review, Row as FormRow, detail};
use crate::monitor::{
    crumbs, duration, fit,
    layout::{self, Density},
    plain, width,
};

/// The standalone review drills from its waiting list into Detail.
const LEVELS: usize = 2;
/// Keep a transient notice inside the fixed four-line footer rather than covering review content.
const MAX_NOTICE_LINES: usize = 4;
/// Two opposing borders consume one cell each in either dimension.
const BORDER_CELLS: u16 = 2;
/// Fixed columns of the Full waiting list: claim and waiting time.
const CLAIM_WIDTH: u16 = 16;
const WAITING_WIDTH: u16 = 8;
/// A popup takes this share of the terminal's width, leaving the review visible around it.
const POPUP_WIDTH: u16 = 80;
/// The Full list's column titles; a column is never narrower than its title.
const HEADERS: [&str; 6] = ["EVAL", "REQUEST", "STATUS", "CLAIM", "AGE", "REPO"];
/// The status column fits the longest request status, `BUDGET_EXHAUSTED`, and its glyph.
const STATUS_WIDTH: u16 = 18;

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
    let height = (text.len() as u16 + BORDER_CELLS).min(area.height);
    let [area] = Layout::horizontal([Constraint::Percentage(POPUP_WIDTH)])
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
        #[cfg(not(test))]
        let now = OffsetDateTime::now_utc();
        #[cfg(test)]
        let now = super::tests::now();
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
        let focused = usize::from(self.focus == Focus::Detail);
        let mut hits = Hits::default();
        for (level, area, density) in layout::columns(body, LEVELS, focused) {
            match (level, density) {
                (0, density) => {
                    hits.list = area;
                    hits.rows = self.draw_list(frame, area, density, rows.clone());
                }
                (_, Density::Full) => {
                    hits.detail = area;
                    match self.shown.record_mut() {
                        Some(record) => record.draw(frame, area, true),
                        None => hits.review = self.draw_detail(frame, area, true),
                    }
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

    /// `artifactize review › query › eval` and the reviewer.
    fn draw_header(&self, frame: &mut Frame, area: Rect) {
        // The query itself shows under the list; the breadcrumb names the usual one in words.
        let scope = match self.query.trim() {
            "" => "all requests".to_owned(),
            super::search::DEFAULT => "waiting Human reviews".to_owned(),
            query => query.to_owned(),
        };
        let mut segments = vec![scope];
        if self.focus == Focus::Detail {
            match (self.shown.request(), self.shown.record(), &self.open) {
                (Some(view), _, _) => segments.push(view.request.eval_id.to_string()),
                (None, Some(record), _) => segments.push(record.view.request.eval_id.to_string()),
                (None, None, Some(id)) => segments.push(id.to_string()),
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
        let room = usize::from(area.width).saturating_sub(right.width() + crate::HEADER_GAP);
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
                    duration(
                        elapsed
                            .try_into()
                            .expect("elapsed tool time fits time::Duration")
                    )
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
            Focus::List if self.editing_query.is_some() => {
                let candidates = self.candidates();
                if candidates.is_empty() {
                    "type to search · Tab complete · Enter done · Esc cancel · Ctrl-U clear".into()
                } else {
                    format!("Tab {candidates} · Enter done · Esc cancel")
                }
            }
            Focus::List => {
                "↑↓ request · Enter open · Tab tools · / search · PgUp/PgDn page · r refresh · q quit"
                    .into()
            }
            Focus::Detail => match self.shown.record() {
                Some(record) => record.hints().into(),
                None => self.detail_hints(),
            },
        }
    }

    /// Cells of the list: eval, request, status, claim, age and repository, as one-row
    /// [`plain`] text so widths are measured on what the terminal shows.
    fn list_rows(&self, now: OffsetDateTime) -> Vec<[String; 6]> {
        self.listed
            .iter()
            .zip(&self.entries)
            .map(|(view, entry)| {
                let request = &view.request;
                let claim = view.claim.as_ref().map_or("-".into(), |claim| {
                    if claim.reviewer == self.reviewer {
                        format!("{} (you)", claim.reviewer)
                    } else {
                        claim.reviewer.to_string()
                    }
                });
                let status = crate::monitor::Status::from(request.status());
                let age = duration(now - request.created_at.time());
                [
                    request.eval_id.to_string(),
                    request.id.to_string(),
                    format!("{} {status}", status.glyph()),
                    claim,
                    age,
                    entry.repository.clone(),
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
        rows: Vec<[String; 6]>,
    ) -> Vec<(Rect, usize)> {
        let focused = density == Density::Full;
        let name = if self.waiting_query() {
            "Waiting Human reviews"
        } else {
            "Requests"
        };
        let page = match (self.page(), self.next.is_some()) {
            (1, false) => String::new(),
            (page, more) => format!(" · page {page}{}", if more { ", PgDn more" } else { "" }),
        };
        let (query, error) = self.query_line();
        let block = Block::bordered()
            .title(format!(" {name} ({}){page} ", self.listed.len()))
            .title_bottom(Line::from(format!(" {query} ")))
            .border_style(if focused {
                Style::new().cyan()
            } else {
                Style::default()
            });
        if self.listed.is_empty() {
            let text = match error {
                Some(error) => error,
                None if self.refreshed.is_none() => "Loading…",
                None if self.waiting_query() => "No Human reviews are waiting.",
                None => "No request matches the search.",
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
            let room = usize::from(area.width.saturating_sub(BORDER_CELLS));
            let rows = rows.into_iter().zip(&self.listed).map(|(cells, view)| {
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
        let [eval, request, status, claim, age] = column_widths(&rows);
        let table = Table::new(
            rows.into_iter().map(Row::new),
            [
                Constraint::Length(eval),
                Constraint::Length(request),
                Constraint::Length(status),
                Constraint::Length(claim),
                Constraint::Length(age),
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
        (self.list.offset()..self.listed.len())
            .zip(top..inner.bottom())
            .map(|(index, y)| (Rect::new(inner.x, y, inner.width, 1), index))
            .collect()
    }

    /// The Detail Preview of the selected request, read-only.
    fn draw_preview(&self, frame: &mut Frame, area: Rect) {
        match self
            .list
            .selected()
            .and_then(|index| self.listed.get(index))
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

    /// The Tool log: each run's start line, its stdout and red stderr, and its end line. It
    /// wraps, scrolls on its own, and follows the bottom unless the reader scrolled up.
    pub(super) fn draw_log(&self, frame: &mut Frame, area: Rect) {
        let block = Block::bordered().title(" Tool log ");
        if self.log.is_empty() {
            frame.render_widget(
                Paragraph::new("What the tools print shows here while this review is open.")
                    .wrap(Wrap { trim: false })
                    .dark_gray()
                    .block(block),
                area,
            );
            return;
        }
        let mut lines = Vec::new();
        for entry in self.log.entries() {
            lines.push(Line::from(entry.start_text(time::UtcOffset::UTC)).dark_gray());
            lines.extend(entry.lines.iter().map(|(stream, text)| match stream {
                super::log::Stream::Stdout => Line::from(text.clone()),
                super::log::Stream::Stderr => Line::from(text.clone()).red(),
            }));
            if let Some(end) = &entry.end {
                let line = Line::from(end.text());
                lines.push(if end.error() {
                    line.red()
                } else {
                    line.dark_gray()
                });
            }
        }
        let inner = block.inner(area);
        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
        let rows = u16::try_from(paragraph.line_count(inner.width)).unwrap_or(u16::MAX);
        let bottom = rows.saturating_sub(inner.height);
        self.log_bottom.set(bottom);
        let top = self.scroll.map_or(bottom, |top| top.min(bottom));
        frame.render_widget(paragraph.scroll((top, 0)).block(block), area);
    }
}

/// Widths of the Full list's non-flexible columns; claim and waiting time keep fixed widths.
/// The repository column fills the remaining space rather than contributing a natural width.
fn column_widths(rows: &[[String; 6]]) -> [u16; 5] {
    let mut widths = [
        text_width(HEADERS[0]),
        text_width(HEADERS[1]),
        STATUS_WIDTH,
        CLAIM_WIDTH,
        WAITING_WIDTH,
    ];
    for row in rows {
        for column in [0, 1] {
            widths[column] = widths[column].max(text_width(&row[column]));
        }
    }
    widths
}

/// What one form row shows: a field, a field of one group (named by the group's number), or
/// a group list's `+ Add`.
fn row_line(form: &Form, row: FormRow, selected: bool) -> Line<'static> {
    let Some(field) = form.field(row) else {
        return Line::default();
    };
    let line = match row {
        FormRow::Add(_) => Line::from(vec![
            Span::styled(format!("+ Add {}", field.name), Modifier::BOLD),
            Span::styled(
                format!("   Enter adds a group · {}", field.hint),
                Style::new().dark_gray(),
            ),
        ]),
        FormRow::Field(_) | FormRow::Item { .. } => {
            let name = match row {
                FormRow::Item {
                    field: list, item, ..
                } => {
                    format!("{} {} · {}", form.fields[list].name, item + 1, field.name)
                }
                _ => field.name.clone(),
            };
            let marker = if field.required { "*" } else { "" };
            let mut value = field.display();
            if selected
                && matches!(
                    field.input,
                    Input::Text(_) | Input::Integer(_) | Input::Number(_) | Input::List(_)
                )
            {
                value.push('▏');
            }
            Line::from(vec![
                Span::styled(format!("{name}{marker}: "), Modifier::BOLD),
                Span::raw(value),
                Span::styled(format!("   {}", field.hint), Style::new().dark_gray()),
            ])
        }
    };
    if selected {
        line.add_modifier(Modifier::REVERSED)
    } else {
        line
    }
}

/// Same ratatui wrapping as rendering, so a continuation line belongs to its actual row.
pub(super) fn field_hits(area: Rect, form: &Form, scroll: u16) -> Vec<(Rect, usize)> {
    let inner = Block::bordered().inner(area);
    let mut top = 0usize;
    form.rows()
        .into_iter()
        .enumerate()
        .filter_map(|(index, row)| {
            let height = Paragraph::new(row_line(form, row, index == form.selected))
                .wrap(Wrap { trim: false })
                .line_count(inner.width);
            let first = top.saturating_sub(usize::from(scroll));
            let last = top
                .saturating_add(height)
                .saturating_sub(usize::from(scroll))
                .min(usize::from(inner.height));
            let visible = top.saturating_add(height) > usize::from(scroll) && first < last;
            top += height;
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
    let color = if form.verdict == crate::runtime::Verdict::Green {
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
    let rows = form.rows();
    let mut text = Vec::new();
    if rows.is_empty() {
        text.push(Line::from(
            "This verdict asks for nothing more; Ctrl-S or Submit records it.",
        ));
    }
    for (index, row) in rows.into_iter().enumerate() {
        text.push(row_line(form, row, index == form.selected));
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
