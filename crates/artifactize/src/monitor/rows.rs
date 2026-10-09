//! Rendering of the Artifacts and evals tree: aligned rows, the upstream `↑` markers of the
//! selected eval and their off-screen counts.
use super::model::{self, Completion, Node, Tone, Upstream, Weight};
use ratatui::{
    Frame,
    buffer::CellWidth,
    layout::Rect,
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};
use std::borrow::Cow;
use time::OffsetDateTime;
use tui_tree_widget::{Tree, TreeItem, TreeState};
use unicode_segmentation::UnicodeSegmentation;

/// The selection marker takes one column at the start of each row.
const MARKER: usize = 1;
/// The fold symbol and its space take two columns before a node's glyph.
const FOLD: usize = 2;
/// Each level of depth indents its children by two columns, as the tree widget draws them.
const INDENT: usize = 2;
/// A node's state glyph and the space after it.
const GLYPH: usize = 2;
/// The gap between the widest name and the status column.
const STATUS_GAP: usize = 2;
/// The mark after a changed node's status; every row keeps room for it once any node shows it.
const CHANGED_MARK: &str = " *";
/// A full-width row's name column never narrows below its glyph and the status gap, so a deep
/// node keeps its glyph even when the tree is barely wider than the indentation.
const MIN_COLUMN: usize = GLYPH + STATUS_GAP;

pub(super) fn color(tone: Tone) -> Color {
    match tone {
        Tone::Green => Color::Green,
        Tone::Red => Color::Red,
        Tone::Error => Color::Magenta,
        Tone::Running | Tone::Queued => Color::Yellow,
        Tone::Human => Color::Cyan,
        Tone::Blocked => Color::LightRed,
        Tone::Muted => Color::DarkGray,
    }
}

/// Text for one row: line breaks and tabs become spaces and other control characters go,
/// so widths are measured on what the terminal shows (and `cell_width` never sees a control).
pub(crate) fn plain(text: &str) -> Cow<'_, str> {
    if !text.contains(char::is_control) {
        return Cow::Borrowed(text);
    }
    Cow::Owned(
        text.chars()
            .filter_map(|character| match character {
                '\n' | '\t' => Some(' '),
                character if character.is_control() => None,
                character => Some(character),
            })
            .collect(),
    )
}

/// Terminal columns of [`plain`] text.
pub(crate) fn width(text: &str) -> usize {
    plain(text)
        .graphemes(true)
        .map(|grapheme| usize::from(grapheme.cell_width()))
        .sum()
}

/// At most `max` columns of [`plain`] text, ending in `…` when cut.
pub(crate) fn fit(text: &str, max: usize) -> String {
    let text = plain(text);
    if width(&text) <= max {
        return text.into_owned();
    }
    let mut fitted = String::new();
    let mut used = 0;
    for grapheme in text.graphemes(true) {
        let size = usize::from(grapheme.cell_width());
        if used + size + 1 > max {
            break;
        }
        used += size;
        fitted.push_str(grapheme);
    }
    if max > 0 {
        fitted.push('…');
    }
    fitted
}

fn lead(depth: usize) -> usize {
    MARKER + depth * INDENT + FOLD
}

/// Where status text starts: after the widest name, but leaving room for the text.
fn column(nodes: &[Node], depth: usize, total: usize) -> usize {
    let widest = nodes
        .iter()
        .map(|node| {
            let own = lead(depth) + GLYPH + width(&node.name) + width(&node.marks) + STATUS_GAP;
            own.max(column(&node.children, depth + 1, total))
        })
        .max()
        .unwrap_or(0);
    widest.min(total / 2)
}

pub(super) struct Layout<'h> {
    pub total: usize,
    pub compact: bool,
    /// Artifacts the selected eval depends on.
    pub upstream: &'h [Upstream],
    /// Elapsed times are drawn as of this instant.
    pub now: OffsetDateTime,
}

fn line(node: &Node, depth: usize, column: usize, star: bool, layout: &Layout) -> Line<'static> {
    let lead = lead(depth);
    let available = layout.total.saturating_sub(lead);
    let right = &node.right_at(layout.now, layout.compact);
    let tail = width(right) + if star { width(CHANGED_MARK) } else { 0 };
    let budget = if layout.compact {
        available.saturating_sub(tail + 1)
    } else {
        column.saturating_sub(lead).max(MIN_COLUMN).min(available)
    };
    let artifact = node.id.strip_prefix("a:");
    let emphasized = artifact.is_some_and(|id| {
        layout
            .upstream
            .iter()
            .any(|up| up.artifact == id && up.completion != Completion::Complete)
    });
    let dim = node.weight == Weight::Dim;
    let row = if dim {
        Style::new().fg(Color::DarkGray)
    } else {
        Style::new()
    };
    let mut name_style = row;
    if node.weight == Weight::Bold || emphasized {
        name_style = name_style.add_modifier(Modifier::BOLD);
    }
    // The glyph and its space, and in the full tree the gap before the status text.
    let room = budget.saturating_sub(if layout.compact {
        GLYPH
    } else {
        GLYPH + STATUS_GAP
    });
    let (name, marks) = if width(&node.name) + width(&node.marks) <= room {
        (
            plain(&node.name).into_owned(),
            plain(&node.marks).into_owned(),
        )
    } else if width(&node.name) < room {
        let marks = fit(&node.marks, room - width(&node.name));
        (plain(&node.name).into_owned(), marks)
    } else {
        (fit(&node.name, room), String::new())
    };
    let used = GLYPH + width(&name) + width(&marks);
    let mut spans = vec![
        Span::styled(
            format!("{} ", node.glyph),
            Style::new().fg(color(node.tone)),
        ),
        Span::styled(name, name_style),
        Span::styled(marks, Style::new().fg(Color::DarkGray)),
    ];
    let mut used = used;
    if !layout.compact && !node.text.is_empty() {
        spans.push(Span::raw(" ".repeat(budget.saturating_sub(used))));
        used = used.max(budget);
        let mut room = available.saturating_sub(used + tail + usize::from(!right.is_empty()));
        let text = match node.weight {
            Weight::Bold => Style::new().fg(color(node.tone)),
            _ => row,
        };
        for segment in &node.text {
            if room == 0 {
                break;
            }
            let fitted = fit(&segment.text, room);
            room = room.saturating_sub(width(&fitted));
            used += width(&fitted);
            let style = segment
                .tone
                .map_or(text, |tone| Style::new().fg(color(tone)));
            spans.push(Span::styled(fitted, style));
        }
    }
    if !right.is_empty() || node.changed {
        spans.push(Span::raw(
            " ".repeat(available.saturating_sub(used + tail).max(1)),
        ));
        let right_style = if artifact.is_some() || dim {
            Style::new().fg(Color::DarkGray)
        } else {
            Style::new()
        };
        spans.push(Span::styled(plain(right).into_owned(), right_style));
        if node.changed {
            spans.push(Span::styled(
                CHANGED_MARK,
                Style::new().fg(Color::DarkGray).add_modifier(Modifier::DIM),
            ));
        }
    }
    Line::from(spans)
}

fn item(
    node: &Node,
    depth: usize,
    column: usize,
    star: bool,
    layout: &Layout,
) -> std::io::Result<TreeItem<'static, String>> {
    let children = node
        .children
        .iter()
        .map(|child| item(child, depth + 1, column, star, layout))
        .collect::<Result<_, _>>()?;
    TreeItem::new(
        node.id.clone(),
        line(node, depth, column, star, layout),
        children,
    )
}

/// Tree items for an inner width of `layout.total` columns.
pub(super) fn items(
    nodes: &[Node],
    layout: &Layout,
) -> std::io::Result<Vec<TreeItem<'static, String>>> {
    let column = column(nodes, 0, layout.total);
    let star = nodes
        .iter()
        .any(|node| node.changed || node.children.iter().any(|child| child.changed));
    nodes
        .iter()
        .map(|node| item(node, 0, column, star, layout))
        .collect()
}

/// Rows of the Artifacts `upstream` names, as indices into the visible (unfolded) rows.
pub(super) fn marked(
    items: &[TreeItem<'static, String>],
    state: &TreeState<String>,
    upstream: &[Upstream],
) -> Vec<(usize, Completion)> {
    let visible = state.flatten(items);
    upstream
        .iter()
        .filter_map(|up| {
            let id = vec![format!("a:{}", up.artifact)];
            visible
                .iter()
                .position(|row| row.identifier == id)
                .map(|index| (index, up.completion))
        })
        .collect()
}

/// Draw the tree; mark the selected eval's upstream Artifact rows with `↑` and count the
/// marked rows scrolled out of view on the borders. The rows fill the block's inner width,
/// whatever `layout.total` says.
pub(super) fn draw(
    frame: &mut Frame,
    area: Rect,
    block: Block<'static>,
    nodes: &[Node],
    state: &mut TreeState<String>,
    layout: Layout,
) {
    let inner = block.inner(area);
    let layout = Layout {
        total: usize::from(inner.width),
        ..layout
    };
    let upstream = layout.upstream;
    let items = match items(nodes, &layout) {
        Ok(items) => items,
        Err(error) => {
            frame.render_widget(Paragraph::new(error.to_string()).red(), area);
            return;
        }
    };
    let tree = match Tree::new(&items) {
        Ok(tree) => tree,
        Err(error) => {
            frame.render_widget(Paragraph::new(error.to_string()).red(), area);
            return;
        }
    };
    frame.render_stateful_widget(
        tree.block(block)
            .highlight_style(Modifier::REVERSED.into())
            .highlight_symbol(" ")
            .node_open_symbol("▾ ")
            .node_closed_symbol("▸ "),
        area,
        state,
    );
    let offset = state.get_offset();
    let rows = usize::from(inner.height);
    let (mut above, mut below) = (0, 0);
    for (index, completion) in marked(&items, state, upstream) {
        if index < offset {
            above += 1;
        } else if index >= offset + rows {
            below += 1;
        } else {
            let (_, tone, _) = model::completion(completion);
            let style = if completion == Completion::Complete {
                Style::new().fg(Color::DarkGray)
            } else {
                Style::new().fg(color(tone)).add_modifier(Modifier::BOLD)
            };
            let y = inner.y + (index - offset) as u16;
            if let Some(cell) = frame.buffer_mut().cell_mut((inner.x, y)) {
                cell.set_symbol("↑").set_style(style);
            }
        }
    }
    // Right-aligned on the border, one dash before the corner.
    let border = |y: u16| Rect::new(area.x + 1, y, area.width.saturating_sub(3), 1);
    if above > 0 && area.height > 0 {
        frame.render_widget(
            Line::from(format!(" ↑{above} above ")).right_aligned(),
            border(area.y),
        );
    }
    if below > 0 && area.height > 1 {
        frame.render_widget(
            Line::from(format!(" ↑{below} below ")).right_aligned(),
            border(area.bottom() - 1),
        );
    }
}
