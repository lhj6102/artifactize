//! Form, field hit regions and Tool log shared by the Human Detail in both screens.
use super::{Form, Input, Review, Row as FormRow, log};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Paragraph, Wrap},
};

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

impl Review {
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
                log::Stream::Stdout => Line::from(text.clone()),
                log::Stream::Stderr => Line::from(text.clone()).red(),
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
    text.extend(
        rows.into_iter()
            .enumerate()
            .map(|(index, row)| row_line(form, row, index == form.selected)),
    );
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
