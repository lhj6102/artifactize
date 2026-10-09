//! Ratatui rendering only; semantic selection and review lifecycle live in their controllers.
use super::{
    DetailArea, Monitor, Pane, Scope,
    input::{Button, Hits},
    layout::{self, Density, Hints},
    model::{self, Section},
    rows::fit,
};
use crate::review::Control;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Wrap},
};
use time::OffsetDateTime;

/// Buttons stay two columns apart so adjacent hit areas never overlap.
const BUTTON_GAP: u16 = 2;
/// Evidence sits beside the sections from this inner width, and below them otherwise.
const SIDE_BY_SIDE: u16 = 90;
/// Rows of a running Agent's transcript the peek reads to find its last line.
const PEEK_ROWS: usize = 4;
/// The peek lays the transcript out this wide, so its last row is a whole line, not the end
/// of a wrapped one; the row is then cut to the pane.
const PEEK_WIDTH: usize = 512;
/// Rows the Run tree keeps under the headline: its rule and three rows.
const TREE_ROWS: u16 = 4;
/// Compact Run ids keep this many trailing characters.
const SHORT_ID: usize = 6;

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
fn glyph(status: &str) -> Span<'static> {
    Span::styled(model::glyph(Some(status)).to_owned(), color(Some(status)))
}
fn field_lines(key: &str, value: &str, indent: &str) -> Vec<Line<'static>> {
    let mut values = value.lines();
    std::iter::once(Line::from(vec![
        Span::styled(format!("{indent}{key}: "), Modifier::BOLD),
        Span::raw(values.next().unwrap_or_default().to_owned()),
    ]))
    .chain(values.map(|line| Line::from(format!("{indent}  {line}"))))
    .collect()
}
/// Outcome → What → Provenance → Technical. A peek shows only the Outcome fields.
/// The `Waits for` Artifacts, and beneath each the evals it has not finished when `pending`.
fn waits_lines(value: &str, pending: bool, indent: &str) -> Vec<Line<'static>> {
    let hidden = value.lines().filter(|line| line.starts_with(' ')).count();
    let mut lines: Vec<Line> = value
        .lines()
        .filter(|line| pending || !line.starts_with(' '))
        .map(|line| Line::from(format!("{indent}{line}")))
        .collect();
    if !pending && hidden > 0 {
        lines.push(Line::from(format!("{indent}{hidden} pending evals · w shows")).dark_gray());
    }
    lines
}
/// Outcome → Waits for → What → Provenance → Technical. A peek shows the Outcome and the
/// Artifacts the eval waits for.
fn section_lines(
    detail: &model::Detail,
    technical: bool,
    pending: bool,
    peek: bool,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for section in Section::ALL {
        if peek && !matches!(section, Section::Outcome | Section::Waits) {
            break;
        }
        let fields: Vec<_> = detail
            .fields
            .iter()
            .filter(|(key, _)| Section::of(key) == section)
            .collect();
        if fields.is_empty() {
            continue;
        }
        if peek {
            lines.extend(fields.into_iter().flat_map(|(key, value)| {
                if section == Section::Waits {
                    let mut lines =
                        vec![Line::from(Span::styled(format!("{key}:"), Modifier::BOLD))];
                    lines.extend(waits_lines(value, false, "  "));
                    lines
                } else {
                    field_lines(key, value, "")
                }
            }));
            continue;
        }
        let heading = Style::new().bold().cyan();
        if section == Section::Waits {
            lines.push(Line::from(Span::styled(section.name(), heading)));
            for (_, value) in fields {
                lines.extend(waits_lines(value, pending, " "));
            }
            continue;
        }
        if section == Section::Technical && !technical {
            let keys: Vec<_> = fields.iter().map(|(key, _)| *key).collect();
            lines.push(Line::from(vec![
                Span::styled("▸ Technical", heading),
                Span::raw(format!("  {} · t shows", keys.join(", "))).dark_gray(),
            ]));
            continue;
        }
        lines.push(Line::from(Span::styled(
            if section == Section::Technical {
                "▾ Technical".to_owned()
            } else {
                section.name().to_owned()
            },
            heading,
        )));
        lines.extend(
            fields
                .into_iter()
                .flat_map(|(key, value)| field_lines(key, value, " ")),
        );
    }
    lines
}
fn block(title: String, focused: bool) -> Block<'static> {
    Block::bordered().title(title).border_style(if focused {
        Style::new().cyan()
    } else {
        Style::default()
    })
}
/// The selected row is reversed only where focus is, and bold elsewhere.
fn highlight(focused: bool) -> Style {
    if focused {
        Modifier::REVERSED.into()
    } else {
        Modifier::BOLD.into()
    }
}
/// Display columns. Natural widths add up in `usize` and become `u16` only through `cap`, so a
/// saved text of any length cannot overflow them.
fn width(text: &str) -> usize {
    Line::from(text).width()
}
fn cap(columns: usize) -> u16 {
    u16::try_from(columns).unwrap_or(u16::MAX)
}
/// Widest tree row: indentation, the fold marker, the glyph and the text.
fn tree_width(nodes: &[model::Node], depth: usize) -> usize {
    nodes
        .iter()
        .map(|node| {
            (depth * 2 + 3 + width(&node.line())).max(tree_width(&node.children, depth + 1))
        })
        .max()
        .unwrap_or(0)
}
/// Runs columns: glyph, status, id, age, counts, took and workspace.
fn run_cells(row: &model::RunRow, workspace: bool) -> [String; 7] {
    let name = std::path::Path::new(&row.repo)
        .file_name()
        .map_or_else(|| row.repo.clone(), |name| name.to_string_lossy().into());
    [
        model::glyph(Some(&row.status)).to_owned(),
        row.status.clone(),
        row.id.clone(),
        row.age.clone(),
        row.counts.clone(),
        if row.took.is_empty() {
            String::new()
        } else {
            format!("took {}", row.took)
        },
        if workspace { name } else { String::new() },
    ]
}
/// Column widths of non-empty columns; narrow widths drop workspace, took, counts and then
/// the status word, so glyph, id and age always remain.
fn run_widths(rows: &[[String; 7]], available: usize) -> [usize; 7] {
    let mut widths = [0usize; 7];
    for row in rows {
        for (column, cell) in row.iter().enumerate() {
            widths[column] = widths[column].max(width(cell));
        }
    }
    for column in [6, 5, 4, 1] {
        if total(&widths) <= available {
            break;
        }
        widths[column] = 0;
    }
    widths
}
fn total(widths: &[usize]) -> usize {
    let shown = widths.iter().filter(|width| **width > 0).count();
    widths.iter().sum::<usize>() + shown.saturating_sub(1)
}
/// The Run pane's natural width covers its tree rows and headline up to this bound; rows cut
/// their status text, and the rest of a wide terminal goes to the Detail peek.
const RUN_BOUND: usize = 80;

/// Columns of the Run headline and its attention lines.
fn strip_width(strip: &model::Strip) -> usize {
    let headline = width(&strip.status) + width(&strip.headline) + 5;
    let attention = strip
        .attention
        .iter()
        .map(|(_, text)| width(text) + 2)
        .max()
        .unwrap_or(0);
    headline.max(attention)
}
fn short_id(id: &str) -> &str {
    let id = id.strip_prefix("run-").unwrap_or(id);
    let start = id
        .char_indices()
        .rev()
        .nth(SHORT_ID - 1)
        .map_or(0, |(start, _)| start);
    &id[start..]
}

impl Monitor {
    pub fn draw(&mut self, frame: &mut Frame) {
        self.hits = Hits::default();
        self.size = frame.area();
        let now = OffsetDateTime::now_utc();
        let [header, body, notice, keys] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(u16::from(self.error.is_some() || self.notice.is_some())),
            Constraint::Length(1),
        ])
        .areas(frame.area());
        self.draw_header(frame, header);
        if let Some(message) = self.error.as_ref().or(self.notice.as_ref()) {
            frame.render_widget(Line::from(message.as_str()).red(), notice);
        }
        frame.render_widget(Line::from(self.key_hints()).dark_gray(), keys);
        // The cached tree, with the Run row above it.
        let tree = self.nodes();
        let nodes = self.run.as_ref().map(|(run, _)| {
            std::iter::once(model::run_node(run))
                .chain(tree.iter().cloned())
                .collect::<Vec<_>>()
        });
        let hints = Hints {
            scope: self.scope_width(),
            runs: self.runs_width(now),
            tree: nodes.as_deref().map_or(0, |nodes| {
                let strip = self.run.as_ref().map_or(0, |(run, requests)| {
                    strip_width(&model::strip(&model::progress(run, requests, now)))
                });
                cap((tree_width(nodes, 0).max(strip) + 3).min(RUN_BOUND))
            }),
        };
        let plan = layout::plan(body, self.focus, hints);
        for (pane, area, density) in plan.panes {
            self.hits.panes.push((pane, area));
            match pane {
                Pane::Repositories => self.draw_scope(frame, area, density),
                Pane::Runs => self.draw_runs(frame, area, density, now),
                Pane::Artifacts => self.draw_run(frame, area, density, nodes.clone(), now),
                Pane::Detail if density == Density::Full && self.detail.is_some() => {
                    self.draw_detail(frame, area)
                }
                Pane::Detail => self.draw_peek(frame, area, now),
            }
        }
        if self.help {
            draw_help(frame, body);
        }
    }
    fn scope_label(&self) -> String {
        if self.scope == Scope::All {
            return "all repositories".into();
        }
        self.catalog
            .rows
            .iter()
            .find(|row| row.fold.is_none() && row.scope == self.scope)
            .map_or_else(
                || match &self.scope {
                    Scope::Worktree(_, path) => path.file_name().map_or_else(
                        || path.display().to_string(),
                        |name| name.to_string_lossy().into(),
                    ),
                    _ => "repository".into(),
                },
                |row| row.label.clone(),
            )
    }
    /// `artifactize › scope › Run › node`, up to the focused level, and global attention.
    fn draw_header(&self, frame: &mut Frame, area: Rect) {
        let level = layout::level(self.focus);
        let mut segments = vec![self.scope_label()];
        if level >= 1
            && let Some(run) = &self.open
        {
            segments.push(run.to_string());
        }
        if level >= 2 {
            match self.target() {
                Some(model::Target::Run) => segments.push("Run".into()),
                Some(model::Target::Artifact(id) | model::Target::Eval(id)) => segments.push(id),
                None => {}
            }
        }
        let mut right: Vec<Span> = Vec::new();
        if self.refreshed.is_none() {
            right.push("loading…  ".dark_gray());
        }
        let badge = self
            .catalog
            .rows
            .first()
            .filter(|row| row.scope == Scope::All)
            .map(|row| row.badge.parts())
            .unwrap_or_default();
        for (status, count) in badge {
            right.push(Span::styled(
                format!("{}{count} ", model::glyph(Some(status))),
                color(Some(status)),
            ));
        }
        if !self.mouse_capture {
            right.push(" mouse off · F2".yellow());
        }
        let right = Line::from(right);
        let room = usize::from(area.width).saturating_sub(right.width() + 2);
        let crumb = breadcrumb(&segments, room);
        let [left, end] = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Length(u16::try_from(right.width()).unwrap_or(u16::MAX)),
        ])
        .areas(area);
        frame.render_widget(Line::from(crumb).bold(), left);
        frame.render_widget(right, end);
    }
    fn key_hints(&mut self) -> String {
        if self.help {
            return "any key closes help".into();
        }
        if self.focus == Pane::Artifacts {
            return format!(
                "↑↓ node · Enter open · h/l fold · ! next attention{} · ← Runs · ? keys · q quit",
                self.blocker_hint()
            );
        }
        match self.focus {
            Pane::Repositories => {
                "↑↓ scope · → Runs · Space worktrees without Runs · ? keys · q quit"
            }
            Pane::Runs | Pane::Artifacts => {
                "↑↓ Run · → Artifacts · ← scope · ! attention · ? keys · q quit"
            }
            Pane::Detail => match &self.detail {
                Some(pane) => match &pane.review {
                    Some(review) if review.confirming() => "Enter confirm · Esc cancel",
                    Some(review) if review.busy() => "Esc or Ctrl-C cancel",
                    Some(review) if review.settled() => "PgUp/PgDn scroll · Esc back",
                    Some(review) if review.owned() && review.editing() => {
                        "Ctrl-S submit · Ctrl-G/R verdict · Tab tools/fields · Esc back"
                    }
                    Some(review) if review.owned() => {
                        "g GREEN · r RED · u release · Tab tools · Esc back"
                    }
                    Some(_) => "c claim · Tab tools · Esc back",
                    None if pane.live.is_some() && !pane.show_details => {
                        "↑↓ PgUp/PgDn scroll · Home/End · Space tools · d details · Esc back"
                    }
                    None if pane.detail.field("Waits for").is_some() => {
                        "↑↓ scroll · Tab area · t technical · w pending · ! attention · Esc back"
                    }
                    None => "↑↓ scroll · Tab area · t technical · ! next attention · Esc back",
                },
                None => "Esc back",
            },
        }
        .into()
    }
    fn scope_width(&self) -> u16 {
        let label = self
            .catalog
            .rows
            .iter()
            .map(|row| row.depth * 2 + width(&row.label))
            .max()
            .unwrap_or(0);
        let badge = self
            .catalog
            .rows
            .iter()
            .map(|row| width(&row.badge.text()))
            .max()
            .unwrap_or(0);
        cap(label + badge + 4)
    }
    fn draw_scope(&mut self, frame: &mut Frame, area: Rect, density: Density) {
        let focused = self.focus == Pane::Repositories;
        // Compact keeps only the most urgent badge, so names stay readable.
        let parts = |row: &super::catalog::CatalogRow| {
            let mut parts = row.badge.parts();
            if density == Density::Compact {
                parts.truncate(1);
            }
            parts
        };
        let badge = self
            .catalog
            .rows
            .iter()
            .map(|row| {
                let parts = parts(row);
                parts
                    .iter()
                    .map(|(_, count)| count.to_string().len() as u16 + 2)
                    .sum::<u16>()
            })
            .max()
            .unwrap_or(0);
        let rows = self.catalog.rows.iter().map(|row| {
            let label = format!("{}{}", "  ".repeat(row.depth), row.label);
            let label = if row.fold.is_some() {
                Span::raw(label).dark_gray()
            } else {
                Span::raw(label)
            };
            let parts = parts(row).into_iter().flat_map(|(status, count)| {
                [
                    Span::styled(
                        format!("{}{count}", model::glyph(Some(status))),
                        color(Some(status)),
                    ),
                    Span::raw(" "),
                ]
            });
            Row::new([
                Cell::from(label),
                Cell::from(Line::from(parts.collect::<Vec<_>>())),
            ])
        });
        frame.render_stateful_widget(
            Table::new(rows, [Constraint::Fill(1), Constraint::Length(badge)])
                .row_highlight_style(highlight(focused))
                .block(block(" Scope ".into(), focused)),
            area,
            &mut self.repositories,
        );
    }
    fn runs_width(&self, now: OffsetDateTime) -> u16 {
        let workspace = !matches!(self.scope, Scope::Worktree(..));
        let rows: Vec<_> = model::run_rows(&self.runs, now)
            .iter()
            .map(|row| run_cells(row, workspace))
            .collect();
        cap(total(&run_widths(&rows, usize::MAX)) + 3)
    }
    fn draw_runs(&mut self, frame: &mut Frame, area: Rect, density: Density, now: OffsetDateTime) {
        let focused = self.focus == Pane::Runs;
        let block = block(format!(" Runs ({}) ", self.runs.len()), focused);
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
        let workspace = !matches!(self.scope, Scope::Worktree(..));
        let rows = model::run_rows(&self.runs, now);
        let (rows, widths): (Vec<Row>, Vec<Constraint>) = if density == Density::Compact {
            let rows = rows.iter().map(|row| {
                Row::new([
                    Cell::from(Line::from(vec![
                        glyph(&row.status),
                        format!(" {}", short_id(&row.id)).into(),
                    ])),
                    Cell::from(row.age.split(' ').next().unwrap_or_default().to_owned()),
                ])
            });
            (
                rows.collect(),
                vec![Constraint::Length(SHORT_ID as u16 + 2), Constraint::Fill(1)],
            )
        } else {
            let cells: Vec<_> = rows.iter().map(|row| run_cells(row, workspace)).collect();
            let widths = run_widths(&cells, usize::from(area.width.saturating_sub(3)));
            let rows = rows.iter().zip(&cells).map(|(row, cells)| {
                Row::new(
                    cells
                        .iter()
                        .enumerate()
                        .filter(|(column, _)| widths[*column] > 0)
                        .map(|(column, cell)| match column {
                            0 => Cell::from(glyph(&row.status)),
                            1 => Cell::from(status(&row.status)),
                            5 | 6 => Cell::from(Span::raw(cell.clone()).dark_gray()),
                            _ => Cell::from(cell.clone()),
                        }),
                )
            });
            (
                rows.collect(),
                widths
                    .iter()
                    .filter(|width| **width > 0)
                    .map(|width| Constraint::Length(cap(*width)))
                    .collect(),
            )
        };
        frame.render_stateful_widget(
            Table::new(rows, widths)
                .row_highlight_style(highlight(focused))
                .block(block),
            area,
            &mut self.list,
        );
    }
    /// The Run pane: its headline and attention lines, then the Artifacts and evals tree.
    fn draw_run(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        density: Density,
        nodes: Option<Vec<model::Node>>,
        now: OffsetDateTime,
    ) {
        let focused = self.focus == Pane::Artifacts;
        let (Some((run, requests)), Some(nodes)) = (&self.run, nodes) else {
            frame.render_widget(
                Paragraph::new("Select a Run.").block(block(" Run ".into(), focused)),
                area,
            );
            return;
        };
        let progress = model::progress(run, requests, now);
        let strip = model::strip(&progress);
        let outer = block(format!(" Run {} ", run.run.id), focused);
        let inner = outer.inner(area);
        frame.render_widget(outer, area);
        // Compact keeps the status and its counts; the rest waits for focus.
        let headline = if density == Density::Compact {
            model::counts(
                progress
                    .counts
                    .iter()
                    .map(|(status, n)| (status.as_str(), *n)),
            )
        } else {
            strip.headline.clone()
        };
        let headline = Paragraph::new(Line::from(vec![
            glyph(&strip.status),
            " ".into(),
            status(&strip.status),
            format!(" · {headline}").into(),
        ]))
        .wrap(Wrap { trim: false });
        // The headline wraps into at most a third of the pane; its height is measured after
        // wrapping, so nothing below it is pushed off.
        let cap = if density == Density::Compact {
            1
        } else {
            (inner.height / 3).max(1)
        };
        let wrapped = u16::try_from(headline.line_count(inner.width)).unwrap_or(u16::MAX);
        let headline_height = wrapped.clamp(1, cap);
        // One row per attention item; its full text is in the peek and the Run detail. The tree
        // keeps its rule and a few rows; items that do not fit are counted in `+N more`.
        let mut attention: Vec<Line> = Vec::new();
        if density != Density::Compact {
            let room = usize::from(
                inner
                    .height
                    .saturating_sub(headline_height)
                    .saturating_sub(TREE_ROWS),
            );
            let items = strip.attention.len() + strip.more;
            let shown = if strip.attention.len() + usize::from(strip.more > 0) <= room {
                strip.attention.len()
            } else {
                room.saturating_sub(1)
            };
            attention.extend(strip.attention.iter().take(shown).map(|(state, text)| {
                Line::from(vec![
                    glyph(state),
                    format!(" {}", fit(text, usize::from(inner.width).saturating_sub(2))).into(),
                ])
            }));
            if shown < items && room > 0 {
                attention.push(
                    Line::from(format!("+{} more · Enter on the Run node", items - shown))
                        .dark_gray(),
                );
            }
        }
        let attention_height = u16::try_from(attention.len()).unwrap_or(u16::MAX);
        let height = headline_height.saturating_add(attention_height);
        let [top, rows] =
            Layout::vertical([Constraint::Length(height), Constraint::Fill(1)]).areas(inner);
        let [headline_area, attention_area] =
            Layout::vertical([Constraint::Length(headline_height), Constraint::Fill(1)]).areas(top);
        frame.render_widget(headline, headline_area);
        frame.render_widget(Paragraph::new(attention), attention_area);
        // The tree's top border is the rule under the headline and its bottom border is the
        // pane's own, so `↑N above` and `↑N below` count off-screen upstream rows there.
        let tree_area = Rect::new(rows.x, rows.y, rows.width, rows.height.saturating_add(1));
        self.hits.tree = tree_area;
        let upstream = self.highlighted(&nodes);
        super::rows::draw(
            frame,
            tree_area,
            Block::new()
                .borders(Borders::TOP | Borders::BOTTOM)
                .border_style(if focused {
                    Style::new().cyan()
                } else {
                    Style::default()
                }),
            &nodes,
            &mut self.tree,
            super::rows::Layout {
                total: 0,
                compact: density == Density::Compact,
                upstream: &upstream,
                now,
            },
        );
    }
    /// The tree-focused Detail: the selected node's outcome, following the selection.
    fn draw_peek(&mut self, frame: &mut Frame, area: Rect, now: OffsetDateTime) {
        let target = self.target();
        let detail = match (&self.run, &target) {
            (Some((run, requests)), Some(target)) => {
                Some(model::detail(run, requests, target, now))
            }
            _ => None,
        };
        let outer = block(
            format!(
                " {} ",
                detail
                    .as_ref()
                    .map_or("Detail", |detail| detail.title.as_str())
            ),
            false,
        );
        let inner = outer.inner(area);
        frame.render_widget(outer, area);
        let Some(detail) = detail else {
            frame.render_widget(Paragraph::new("Select a node.").dark_gray(), inner);
            return;
        };
        let mut lines = section_lines(&detail, false, false, true);
        if let Some(peek) = &mut self.peek {
            match &mut peek.live {
                Some(live) => {
                    live.geometry(PEEK_WIDTH, PEEK_ROWS);
                    let last = live.window.status.clone().or_else(|| {
                        live.window
                            .rows
                            .iter()
                            .rev()
                            .find(|row| !row.trim().is_empty())
                            .map(|row| row.trim().to_owned())
                    });
                    let last = last.unwrap_or_else(|| "reading the session…".into());
                    lines.push(Line::from(vec![
                        Span::styled("last: ", Modifier::BOLD),
                        Span::raw(fit(&last, usize::from(inner.width).saturating_sub(6))),
                    ]));
                }
                None if !peek.text.is_empty() => {
                    lines.push(Line::from(peek.text.clone()).dark_gray())
                }
                None => {}
            }
        }
        lines.push(Line::default());
        lines.push(Line::from("Enter: open").dark_gray());
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    }
    /// The opened Detail: sections and evidence, an Agent session, or a Human review.
    fn draw_detail(&mut self, frame: &mut Frame, area: Rect) {
        let Some(pane) = &mut self.detail else {
            return;
        };
        self.hits.detail = area;
        let detail = pane.detail.clone();
        let title = match &pane.review {
            Some(review) => {
                let stage = if review.settled() {
                    "completed"
                } else if review.owned() {
                    "REVIEW (yours)"
                } else {
                    "CLAIM"
                };
                let status = detail.summary.split(' ').next().unwrap_or_default();
                format!(" {} · {status} · {stage} ", detail.title)
            }
            None if detail.summary.is_empty() => format!(" {} ", detail.title),
            None => format!(" {} · {} ", detail.title, detail.summary),
        };
        let outer = Block::bordered().title(title).cyan();
        let inner = outer.inner(area);
        frame.render_widget(outer, area);
        let [content, buttons] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(inner);
        if let Some(review) = &mut pane.review {
            let (tools, fields, instruction) =
                review.draw_single(frame, content, pane.focus == DetailArea::Tools);
            self.hits.instruction = instruction;
            self.hits.tools = tools;
            self.hits.field_rows = review.field_hits(fields);
            let [left, right] =
                Layout::horizontal([Constraint::Percentage(45), Constraint::Fill(1)])
                    .areas(content);
            self.hits.areas = [left, right];
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
        } else if let Some(live) = &mut pane.live {
            let [transcript, footer] =
                Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(content);
            self.hits.areas = [Rect::default(), transcript];
            live.geometry(
                usize::from(transcript.width),
                usize::from(transcript.height),
            );
            frame.render_widget(
                Line::from(format!("{} · Space tools · d details", live.indicator())).dark_gray(),
                footer,
            );
            if pane.show_details {
                frame.render_widget(
                    Paragraph::new(section_lines(&detail, pane.technical, pane.pending, false))
                        .wrap(Wrap { trim: false })
                        .scroll((pane.scroll[0], 0)),
                    transcript,
                );
                self.hits.areas = [transcript, Rect::default()];
            } else {
                self.hits.session_groups = live
                    .window
                    .groups
                    .iter()
                    .filter_map(|(row, id)| {
                        (*row < usize::from(transcript.height)).then_some((
                            Rect::new(
                                transcript.x,
                                transcript.y + *row as u16,
                                transcript.width,
                                1,
                            ),
                            *id,
                        ))
                    })
                    .collect();
                let rows = if let Some(status) = &live.window.status {
                    vec![Line::from(status.as_str())]
                } else if live.window.rows.is_empty() {
                    vec![Line::from(
                        "Waiting for provider text; public thinking summary may be unavailable.",
                    )]
                } else {
                    live.window
                        .rows
                        .iter()
                        .enumerate()
                        .map(|(row, text)| {
                            let line = Line::from(text.as_str());
                            if live.window.thinking.contains(&row)
                                || live.window.groups.iter().any(|(group, _)| *group == row)
                            {
                                line.dark_gray()
                            } else {
                                line
                            }
                        })
                        .collect()
                };
                frame.render_widget(Paragraph::new(rows), transcript);
            }
            self.draw_buttons(
                frame,
                buttons,
                vec![
                    ("Top", Button::SessionTop),
                    ("Bottom", Button::SessionBottom),
                    ("Details (d)", Button::SessionDetails),
                ],
            );
        } else {
            let sections =
                Paragraph::new(section_lines(&detail, pane.technical, pane.pending, false))
                    .wrap(Wrap { trim: false })
                    .scroll((pane.scroll[0], 0));
            if pane.evidence.is_empty() {
                self.hits.areas = [content, Rect::default()];
                frame.render_widget(sections, content);
            } else {
                let [left, right] = if content.width >= SIDE_BY_SIDE {
                    Layout::horizontal([Constraint::Percentage(45), Constraint::Fill(1)])
                        .areas(content)
                } else {
                    Layout::vertical([Constraint::Percentage(50), Constraint::Fill(1)])
                        .areas(content)
                };
                self.hits.areas = [left, right];
                let focused = |area| {
                    if pane.focus == area {
                        Style::new().cyan()
                    } else {
                        Style::default()
                    }
                };
                frame.render_widget(
                    sections.block(
                        Block::bordered()
                            .title(" Sections ")
                            .border_style(focused(DetailArea::Summary)),
                    ),
                    left,
                );
                frame.render_widget(
                    Paragraph::new(pane.evidence.text.as_str())
                        .wrap(Wrap { trim: false })
                        .scroll((pane.scroll[1], 0))
                        .block(
                            Block::bordered()
                                .title(format!(" {} ", pane.evidence.title))
                                .border_style(focused(DetailArea::Evidence)),
                        ),
                    right,
                );
            }
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

/// `artifactize › scope › Run › node` in `room` columns. Earlier segments shorten first, then
/// the program name goes, and the focused end of the path stays longest.
pub(super) fn breadcrumb(segments: &[String], room: usize) -> String {
    let fits = |text: &str| Line::from(text).width() <= room;
    for limit in [usize::MAX, 24, 16, 10] {
        let shown: Vec<String> = segments
            .iter()
            .enumerate()
            .map(|(index, segment)| {
                if index + 1 < segments.len() && segment.chars().count() > limit {
                    format!("{}…", segment.chars().take(limit - 1).collect::<String>())
                } else {
                    segment.clone()
                }
            })
            .collect();
        let path = shown.join(" › ");
        let full = format!("artifactize › {path}");
        if fits(&full) {
            return full;
        }
        if limit == 10 {
            if fits(&path) {
                return path;
            }
            let tail: Vec<char> = path.chars().collect();
            let keep = room.saturating_sub(1).min(tail.len());
            return format!("…{}", tail[tail.len() - keep..].iter().collect::<String>());
        }
    }
    unreachable!("the last limit returns")
}

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
    ("Ctrl-S", "Human: submit the review"),
    ("Mouse", ""),
    ("click", "focus and select; double-click opens"),
    ("wheel", "scroll the pane under the pointer"),
    ("F2", "mouse capture on or off; paste works either way"),
];

fn draw_help(frame: &mut Frame, area: Rect) {
    let lines: Vec<Line> = HELP
        .iter()
        .map(|(key, text)| {
            if text.is_empty() {
                Line::from(Span::styled(*key, Style::new().bold().cyan()))
            } else {
                Line::from(vec![
                    Span::styled(format!("  {key:<16}"), Modifier::BOLD),
                    Span::raw(*text),
                ])
            }
        })
        .collect();
    let width = area.width.min(72);
    let height = area.height.min(lines.len() as u16 + 2);
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
