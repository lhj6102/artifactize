//! One Human review as a Detail component, shared by `monitor` and the standalone `review`.
//!
//! The instruction comes first, under a one-line metadata row (status, claim, waiting time);
//! IDs, the repository and the raw owner schemas move to a folded Technical section (`t`).
//! The focused sub-area grows: the instruction (`i`), the tools with their output, or the
//! CLAIM/REVIEW fields. Keys follow one protocol in both entry points.
use super::{Action, Area, Control, Mode, Review, Runs, SCROLL_PAGE, Tool, view};
use crate::{
    config::HumanToolKind,
    monitor::{duration, plain},
    store::RequestView,
    types::RequestStatus,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Position, Rect},
    style::{Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Paragraph, Wrap},
};
use serde_json::Value;
use time::OffsetDateTime;

/// The expanded instruction (or Technical section) takes this share of the body height.
const EXPANDED: u16 = 70;
/// A bordered box never shrinks below one line of text between its borders.
const MIN_BOX: u16 = 3;
/// Instruction rows kept above the tools and fields once the review is claimed or completed.
const FOLDED: u16 = 2;
/// Width shares of the tools column with and without focus.
const TOOLS_FOCUSED: u16 = 60;
const TOOLS_UNFOCUSED: u16 = 40;
const BUTTON_GAP: u16 = 1;
/// A wrapped builtin action continues two columns deeper than its first row.
const WRAP_INDENT: u16 = 4;

/// Where a key left a Human review Detail.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Handled {
    Action(Action),
    /// Step back out of the Detail, after any form or job is closed.
    Back,
    Quit,
    /// A Detail-wide key for the caller: `?` help or `!` next attention, never while editing.
    Pass,
}

/// Geometry of the last drawn Detail for mouse hit tests.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Hits {
    pub instruction: Rect,
    /// The tools and output column, and the fields or result column.
    pub tools: Rect,
    pub fields: Rect,
    pub tool_rows: Vec<(Rect, usize)>,
    pub field_rows: Vec<(Rect, usize)>,
    pub buttons: Vec<(Rect, Control)>,
    pub close: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Claim,
    Review,
    Completed,
}

impl Stage {
    fn label(self) -> &'static str {
        match self {
            Stage::Claim => "CLAIM",
            Stage::Review => "REVIEW (yours)",
            Stage::Completed => "completed",
        }
    }
}

fn kind(kind: HumanToolKind) -> &'static str {
    match kind {
        HumanToolKind::Launch => "launch",
        HumanToolKind::Output => "output",
    }
}

/// Words of `text` in rows of at most `width` columns; a longer word keeps a row of its own.
/// The first row is indented like the other detail rows, the rest `WRAP_INDENT` deep.
fn wrap(text: &str, width: u16) -> Vec<String> {
    text.split(' ')
        .fold(Vec::new(), |mut rows: Vec<String>, word| {
            match rows.last_mut() {
                Some(row) if Line::from(format!("{row} {word}")).width() <= usize::from(width) => {
                    row.push(' ');
                    row.push_str(word);
                }
                Some(_) => rows.push(format!("{:1$}{word}", "", usize::from(WRAP_INDENT))),
                None => rows.push(format!("  {word}")),
            }
            rows
        })
}

fn focused_border(focused: bool) -> Style {
    if focused {
        Style::new().cyan()
    } else {
        Style::default()
    }
}

fn title(view: &RequestView) -> String {
    plain(&format!(
        "{} · {}",
        view.request.eval_id, view.request.title
    ))
    .into_owned()
}

/// Status, claim and waiting time in one row; errors keep their first line.
pub(super) fn meta(view: &RequestView, reviewer: &str, now: OffsetDateTime) -> String {
    let request = &view.request;
    let waiting = request.status == RequestStatus::WaitingHuman;
    let mut parts = vec![request.status.to_string()];
    if let Some(error) = &request.error {
        let first = error.lines().next().unwrap_or_default();
        parts.push(match &request.error_code {
            Some(code) => format!("[{code}] {first}"),
            None => first.to_owned(),
        });
    }
    match &view.claim {
        None if waiting => parts.push("unclaimed".into()),
        None => {}
        Some(claim) if claim.reviewer == reviewer => {
            parts.push(format!("claimed by you ({reviewer})"))
        }
        Some(claim) => parts.push(format!("claimed by {} · read-only", claim.reviewer)),
    }
    if waiting {
        parts.push(format!(
            "waiting {}",
            duration((now - request.created_at.time()).whole_seconds())
        ));
    }
    plain(&parts.join(" · ")).into_owned()
}

/// A follower's actions go to the request that owns the shared execution.
pub(super) fn shared(view: &RequestView) -> Option<String> {
    let source = view
        .execution
        .as_ref()
        .map(|execution| &execution.provenance)?;
    (source.request_id != view.request.id).then(|| {
        let text = format!(
            "actions go to {} in {}",
            source.request_id,
            crate::platform::path_text(&source.repo_path)
        );
        plain(&text).into_owned()
    })
}

fn schema(schema: Option<&Value>) -> String {
    schema.map_or("none (no owner fields)".into(), |schema| {
        serde_json::to_string_pretty(schema).unwrap_or_default()
    })
}

/// The folded Technical section: identifiers, the repository and the raw owner schemas.
pub(super) fn technical(view: &RequestView, reviewer: &str) -> Vec<(&'static str, String)> {
    let request = &view.request;
    let definition = request.human_definition.as_ref();
    let declaration = definition.map(|definition| &definition["eval"]["declaration"]);
    let status = match (&request.error, &request.error_code) {
        (Some(error), Some(code)) => format!("{} [{code}] {error}", request.status),
        (Some(error), None) => format!("{} {error}", request.status),
        _ => request.status.to_string(),
    };
    let claim = match &view.claim {
        None if request.status == RequestStatus::WaitingHuman => {
            format!("unclaimed; c claims it for {reviewer}")
        }
        None => "none".into(),
        Some(claim) if claim.reviewer == reviewer => {
            format!("claimed by you ({reviewer}) at {}", claim.claimed_at)
        }
        Some(claim) => format!(
            "claimed by {} at {}; read-only",
            claim.reviewer, claim.claimed_at
        ),
    };
    let mut fields = vec![
        ("Request", request.id.to_string()),
        ("Run", request.run_id.to_string()),
        (
            "Repository",
            definition
                .and_then(|definition| definition["repo"].as_str())
                .unwrap_or("-")
                .to_owned(),
        ),
        ("Status", status),
        ("Claim", claim),
    ];
    if let Some(shared) = shared(view) {
        fields.push(("Shared", shared));
    }
    fields.push(("Created", request.created_at.to_string()));
    let owner = |key| schema(declaration.and_then(|declaration| declaration.get(key)));
    fields.push(("GREEN fields", owner("passSchema")));
    fields.push(("RED fields", owner("failSchema")));
    fields
}

/// The instruction, followed by the Technical section when it is unfolded.
fn instruction(view: &RequestView, reviewer: &str, technical: bool) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = view
        .request
        .payload
        .instruction()
        .lines()
        .map(|line| Line::from(line.to_owned()))
        .collect();
    if technical {
        lines.push(Line::default());
        lines.push(Line::from("Technical").bold());
        lines.extend(view::lines(self::technical(view, reviewer)));
    }
    lines
}

/// Tools in one line: name and kind, then the way in when there is one.
fn tools_line(tools: &[Tool], way_in: Option<&str>) -> Line<'static> {
    if tools.is_empty() {
        return Line::from("Tools  none declared in this eval's scope").dark_gray();
    }
    let names = tools
        .iter()
        .map(|tool| plain(&format!("{} ({})", tool.name, kind(tool.kind))).into_owned())
        .collect::<Vec<_>>()
        .join(" · ");
    let mut spans = vec![Span::styled("Tools  ", Modifier::BOLD), Span::raw(names)];
    if let Some(way_in) = way_in {
        spans.push(Span::raw(format!("  · {way_in}")).dark_gray());
    }
    Line::from(spans)
}

/// The Preview of a listed request: metadata, the instruction and its tools, read-only.
pub(super) fn draw_peek(frame: &mut Frame, area: Rect, view: &RequestView, reviewer: &str) {
    let outer = Block::bordered().title(format!(" {} ", title(view)));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    let mut lines = vec![Line::from(meta(view, reviewer, OffsetDateTime::now_utc())).bold()];
    if let Some(shared) = shared(view) {
        lines.push(Line::from(shared).yellow());
    }
    lines.push(Line::default());
    lines.extend(instruction(view, reviewer, false));
    lines.push(Line::default());
    let tools = super::tools(&view.request);
    lines.push(tools_line(&tools, None));
    lines.push(Line::default());
    lines.push(
        Line::from(if tools.is_empty() {
            "Enter: open review"
        } else {
            "Enter: open review · Tab: tools"
        })
        .dark_gray(),
    );
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

impl Review {
    fn stage(&self) -> Stage {
        if self.settled() {
            Stage::Completed
        } else if self.owned() {
            Stage::Review
        } else {
            Stage::Claim
        }
    }

    /// The key that moves focus to the Tools pane from here: Tab, or Shift-Tab while a form
    /// field takes Tab.
    fn tools_key(&self) -> &'static str {
        if self.editing() && self.area == Area::Fields {
            "Shift-Tab"
        } else {
            "Tab"
        }
    }

    /// The Tools pane title names how to reach it, or how to run a tool once it has focus.
    fn tools_title(&self, count: usize) -> String {
        if count == 0 {
            format!(" Tools ({count}) ")
        } else if self.area == Area::Tools {
            format!(" Tools ({count}) · ↑↓ Enter run ")
        } else {
            format!(" Tools ({count}) · {} ", self.tools_key())
        }
    }

    /// The key line for this Detail's current state: the keys that work in the focused area,
    /// including how to reach the tools and run one.
    pub(crate) fn detail_hints(&self) -> String {
        if self.busy() {
            return "Esc or Ctrl-C cancel · PgUp/PgDn scroll output".into();
        }
        if self.settled() {
            return "PgUp/PgDn scroll · i instruction · t technical · Esc back · q quit".into();
        }
        let tools = !self.tools().is_empty();
        let owned = self.owned();
        // Unclaimed and waiting; a request claimed by someone else is read-only.
        let claimable = matches!(self.actionable(), Ok((_, true)));
        let editing = owned && self.editing();
        let mut hints: Vec<String> = Vec::new();
        if self.area == Area::Tools {
            if tools {
                hints.push("↑↓ tool".into());
                if owned {
                    hints.push("Enter run".into());
                } else if claimable {
                    hints.push("c claim, then Enter run".into());
                }
            } else if claimable {
                hints.push("c claim".into());
            }
            hints.push("Tab fields".into());
        } else if editing {
            hints.extend(["Ctrl-S submit".into(), "Ctrl-G/R verdict".into()]);
            if self.standalone {
                hints.push("Ctrl-E $EDITOR".into());
            }
        } else if owned {
            hints.extend(["g GREEN".into(), "r RED".into(), "u release".into()]);
        } else if claimable {
            hints.push("c claim".into());
        }
        if self.area != Area::Tools {
            let key = self.tools_key();
            hints.push(if tools && (owned || claimable) {
                format!("{key} tools, Enter run")
            } else {
                format!("{key} tools")
            });
        }
        if editing {
            hints.push("Esc stop editing".into());
        } else {
            hints.extend([
                "i instruction".into(),
                "t technical".into(),
                "Esc back".into(),
                "q quit".into(),
            ]);
        }
        hints.join(" · ")
    }

    /// A left click inside the Detail at `point`, over the geometry of the last drawn frame:
    /// buttons act, the instruction, tools or fields take focus, a click selects a tool row and
    /// a click on the selected tool of the focused Tools pane runs it.
    pub(crate) fn click(&mut self, hits: &Hits, point: Position) -> Action {
        if let Some((_, control)) = hits.buttons.iter().find(|(rect, _)| rect.contains(point)) {
            return self.control(*control);
        }
        if self.busy() {
            return Action::None;
        }
        if let Some((_, index)) = hits.tool_rows.iter().find(|(rect, _)| rect.contains(point)) {
            let run = self.area == Area::Tools && self.tool == *index;
            self.area = Area::Tools;
            self.selected_tool(*index);
            return if run {
                self.control(Control::RunTool)
            } else {
                Action::None
            };
        }
        if let Some((_, index)) = hits
            .field_rows
            .iter()
            .find(|(rect, _)| rect.contains(point))
        {
            self.area = Area::Fields;
            self.select_field(*index);
        } else if hits.tools.contains(point) {
            self.area = Area::Tools;
        } else if hits.fields.contains(point) {
            self.area = Area::Fields;
        } else if hits.instruction.contains(point) && !self.editing() {
            self.area = Area::Instruction;
        }
        Action::None
    }

    /// Keys of a Human review Detail, in monitor and the standalone review alike. Review keys
    /// act only here; while a form is edited letters are text and only Ctrl keys act.
    pub(crate) fn key_detail(&mut self, key: KeyEvent) -> Handled {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let page = |down: bool| {
            if down {
                SCROLL_PAGE as i16
            } else {
                -(SCROLL_PAGE as i16)
            }
        };
        if control && matches!(key.code, KeyCode::PageDown | KeyCode::PageUp) {
            self.scroll_instruction(page(key.code == KeyCode::PageDown));
            return Handled::Action(Action::None);
        }
        if let Some(busy) = &self.busy {
            match key.code {
                KeyCode::Esc => busy.cancel.cancel(),
                KeyCode::Char('c') if control => busy.cancel.cancel(),
                KeyCode::PageDown | KeyCode::PageUp => {
                    self.scroll_single(page(key.code == KeyCode::PageDown), false)
                }
                _ => {}
            }
            return Handled::Action(Action::None);
        }
        // Esc closes one thing at a time: the form, then the Detail.
        if key.code == KeyCode::Esc {
            if self.editing() {
                self.stop_editing();
                return Handled::Action(Action::None);
            }
            return Handled::Back;
        }
        let editing = self.editing();
        if matches!(key.code, KeyCode::PageDown | KeyCode::PageUp) {
            let delta = page(key.code == KeyCode::PageDown);
            match self.area {
                Area::Instruction => self.scroll_instruction(delta),
                Area::Fields if editing || self.settled() => self.scroll_single(delta, true),
                Area::Tools | Area::Fields => self.scroll_single(delta, false),
            }
            return Handled::Action(Action::None);
        }
        if key.code == KeyCode::BackTab
            || key.code == KeyCode::Tab && (self.area != Area::Fields || !editing)
        {
            self.area = if self.area == Area::Tools {
                Area::Fields
            } else {
                Area::Tools
            };
            return Handled::Action(Action::None);
        }
        // An unowned form (after losing the claim) still takes c to claim it again.
        if !self.owned() && !control && key.code == KeyCode::Char('c') {
            return Handled::Action(self.control(Control::Claim));
        }
        if control {
            let action = match key.code {
                KeyCode::Char('g') => self.control(Control::Green),
                KeyCode::Char('r') => self.control(Control::Red),
                KeyCode::Char('u') => self.control(Control::Release),
                KeyCode::Char('s') => self.control(Control::Submit),
                KeyCode::Char('e') if self.standalone && self.owned() => match &mut self.mode {
                    Mode::Form(form) => {
                        let draft = form.draft();
                        form.set_json(draft.clone());
                        Action::Edit(draft)
                    }
                    _ => Action::None,
                },
                _ => Action::None,
            };
            return Handled::Action(action);
        }
        if !editing {
            let action = match key.code {
                KeyCode::Char('c') => self.control(Control::Claim),
                KeyCode::Char('g') => self.control(Control::Green),
                KeyCode::Char('r') => self.control(Control::Red),
                KeyCode::Char('u') => self.control(Control::Release),
                KeyCode::Char('i') => {
                    self.area = if self.area == Area::Instruction {
                        Area::Fields
                    } else {
                        Area::Instruction
                    };
                    Action::None
                }
                KeyCode::Char('t') => {
                    self.technical = !self.technical;
                    Action::None
                }
                KeyCode::Char('q') => return Handled::Quit,
                KeyCode::Char('?' | '!') => return Handled::Pass,
                KeyCode::Left => return Handled::Back,
                _ => return Handled::Action(self.area_key(key, false)),
            };
            return Handled::Action(action);
        }
        Handled::Action(self.area_key(key, true))
    }

    /// Keys for the focused sub-area: tool choice and runs, instruction scrolling, the form.
    fn area_key(&mut self, key: KeyEvent, editing: bool) -> Action {
        match self.area {
            Area::Tools => match key.code {
                KeyCode::Enter => return self.control(Control::RunTool),
                KeyCode::Up => self.move_tool(false),
                KeyCode::Down => self.move_tool(true),
                KeyCode::Char('k') if !editing => self.move_tool(false),
                KeyCode::Char('j') if !editing => self.move_tool(true),
                _ => {}
            },
            Area::Instruction => match key.code {
                KeyCode::Up | KeyCode::Char('k') => self.scroll_instruction(-1),
                KeyCode::Down | KeyCode::Char('j') => self.scroll_instruction(1),
                _ => {}
            },
            Area::Fields => {
                if editing
                    && self.owned()
                    && let Mode::Form(form) = &mut self.mode
                {
                    form.inline_key(key);
                }
            }
        }
        Action::None
    }

    /// Draw the Full Detail of the open request into `area`, borders included.
    pub(crate) fn draw_detail(&mut self, frame: &mut Frame, area: Rect, focused: bool) -> Hits {
        let mut hits = Hits::default();
        let Some(view) = self.request.clone() else {
            let text = self.error.clone().unwrap_or_else(|| "Loading…".into());
            frame.render_widget(
                Paragraph::new(text)
                    .wrap(Wrap { trim: false })
                    .block(Block::bordered().border_style(focused_border(focused))),
                area,
            );
            return hits;
        };
        let stage = self.stage();
        let outer = Block::bordered()
            .title(format!(" {} · {} ", title(&view), stage.label()))
            .border_style(focused_border(focused));
        let inner = outer.inner(area);
        frame.render_widget(outer, area);
        let status = self.status_line();
        let shared = shared(&view);
        let [meta_area, shared_area, body, status_area, buttons] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(u16::from(shared.is_some())),
            Constraint::Fill(1),
            Constraint::Length(status.len() as u16),
            Constraint::Length(1),
        ])
        .areas(inner);
        let fold = Line::from(if self.technical {
            "t technical ▾"
        } else {
            "t technical ▸"
        })
        .dark_gray();
        let [meta_text, fold_area] = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Length(fold.width() as u16 + 1),
        ])
        .areas(meta_area);
        frame.render_widget(
            Line::from(meta(&view, &self.reviewer, OffsetDateTime::now_utc())).bold(),
            meta_text,
        );
        frame.render_widget(fold.right_aligned(), fold_area);
        if let Some(shared) = shared {
            frame.render_widget(Line::from(shared).yellow(), shared_area);
        }
        frame.render_widget(Paragraph::new(status), status_area);

        let expanded = self.area == Area::Instruction || self.technical;
        // Before a claim the instruction is the body, with the tools in one line below it.
        let summary = stage == Stage::Claim && !expanded && self.area != Area::Tools;
        let text = instruction(&view, &self.reviewer, self.technical);
        let rows = Paragraph::new(text.clone())
            .wrap(Wrap { trim: false })
            .line_count(body.width.saturating_sub(2));
        let height = if expanded {
            (body.height * EXPANDED / 100).max(MIN_BOX)
        } else if summary {
            (u16::try_from(rows).unwrap_or(u16::MAX).saturating_add(2))
                .min(body.height.saturating_sub(2))
        } else {
            u16::try_from(rows).unwrap_or(u16::MAX).min(FOLDED) + 2
        }
        .min(body.height);
        let [instruction_area, lower] =
            Layout::vertical([Constraint::Length(height), Constraint::Fill(1)]).areas(body);
        let label = match (self.technical, expanded) {
            (true, _) => " Instruction · Technical · Ctrl-PgUp/PgDn ",
            (false, true) => " Instruction · i fold · Ctrl-PgUp/PgDn ",
            (false, false) => " Instruction · i expand · Ctrl-PgUp/PgDn ",
        };
        frame.render_widget(
            Paragraph::new(text)
                .wrap(Wrap { trim: false })
                .scroll((self.instruction_scroll, 0))
                .block(
                    Block::bordered()
                        .title(label)
                        .border_style(focused_border(self.area == Area::Instruction)),
                ),
            instruction_area,
        );
        hits.instruction = instruction_area;
        if summary {
            frame.render_widget(
                Paragraph::new(vec![
                    tools_line(&self.tools(), Some("Tab or click: tools")),
                    Line::from(self.claim_text(&view)).dark_gray(),
                ])
                .wrap(Wrap { trim: false }),
                lower,
            );
            hits.tools = lower;
        } else {
            let share = if self.area == Area::Tools {
                TOOLS_FOCUSED
            } else {
                TOOLS_UNFOCUSED
            };
            let [left, right] =
                Layout::horizontal([Constraint::Percentage(share), Constraint::Fill(1)])
                    .areas(lower);
            hits.tools = left;
            hits.fields = right;
            hits.tool_rows = self.draw_tools(frame, left);
            hits.field_rows = self.draw_result(frame, right, &view);
        }
        self.draw_buttons(frame, buttons, &mut hits);
        hits
    }

    /// One row per tool; the selected tool shows what it runs and its description when focused.
    fn draw_tools(&self, frame: &mut Frame, area: Rect) -> Vec<(Rect, usize)> {
        let tools = self.tools();
        let focused = self.area == Area::Tools;
        let mut rows: Vec<(Line, usize)> = Vec::new();
        for (index, tool) in tools.iter().enumerate() {
            let name =
                Line::from(plain(&format!("{}  {}", tool.name, kind(tool.kind))).into_owned());
            if index == self.tool {
                rows.push((
                    name.style(if focused {
                        Modifier::REVERSED.into()
                    } else {
                        Style::from(Modifier::BOLD)
                    }),
                    index,
                ));
                if focused {
                    // What Enter runs: a command's resolved line once known, else as declared;
                    // a builtin's action on its logical target.
                    match (&tool.declared, self.command(&tool.name)) {
                        (Runs::Command(_), Some(Ok(command))) => {
                            let words =
                                std::iter::once(command.program.to_string_lossy().into_owned())
                                    .chain(command.args.iter().cloned())
                                    .collect::<Vec<_>>();
                            let command = super::shell(words.iter().map(String::as_str));
                            rows.push((Line::from(format!("  $ {}", plain(&command))), index));
                        }
                        (declared @ Runs::Command(_), _) => {
                            rows.push((
                                Line::from(format!("  {}", plain(&declared.line()))),
                                index,
                            ));
                        }
                        // A builtin's target wraps rather than being cut off at the border.
                        (declared, _) => {
                            let width = area.width.saturating_sub(2);
                            rows.extend(
                                wrap(&plain(&declared.line()), width)
                                    .into_iter()
                                    .map(|row| (Line::from(row), index)),
                            );
                        }
                    }
                    if let Some(Err(error)) = self.command(&tool.name) {
                        rows.push((Line::from(format!("  {}", plain(error))).red(), index));
                    }
                    rows.push((
                        Line::from(format!("  {}", plain(&tool.description))).dark_gray(),
                        index,
                    ));
                }
            } else {
                rows.push((name, index));
            }
        }
        let height = (rows.len().max(1) as u16 + 2).min((area.height / 2).max(MIN_BOX));
        let [list, output] =
            Layout::vertical([Constraint::Length(height), Constraint::Fill(1)]).areas(area);
        let block = Block::bordered()
            .title(self.tools_title(tools.len()))
            .border_style(focused_border(focused));
        if tools.is_empty() {
            let recorded = self
                .request
                .as_ref()
                .is_some_and(|view| view.request.human_definition.is_some());
            let text = if recorded {
                "No Human tools are declared in this eval's scope."
            } else {
                "Unknown: no Human definition is recorded."
            };
            frame.render_widget(Paragraph::new(text).dark_gray().block(block), list);
        } else {
            let inner = block.inner(list);
            let selected = rows.iter().position(|(_, index)| *index == self.tool);
            let last = rows
                .iter()
                .rposition(|(_, index)| *index == self.tool)
                .unwrap_or(0);
            let scroll = (last + 1)
                .saturating_sub(usize::from(inner.height))
                .min(selected.unwrap_or(0));
            let mut hits = Vec::new();
            for (row, (_, index)) in rows.iter().enumerate().skip(scroll) {
                let y = row - scroll;
                if y >= usize::from(inner.height) {
                    break;
                }
                hits.push((
                    Rect::new(inner.x, inner.y + y as u16, inner.width, 1),
                    *index,
                ));
            }
            let lines: Vec<Line> = rows.into_iter().map(|(line, _)| line).collect();
            frame.render_widget(
                Paragraph::new(lines)
                    .scroll((u16::try_from(scroll).unwrap_or(u16::MAX), 0))
                    .block(block),
                list,
            );
            self.draw_output(frame, output);
            return hits;
        }
        self.draw_output(frame, output);
        Vec::new()
    }

    /// The fields or result column: the completed result, the owner
    /// form, or the CLAIM state. Returns the visible field rows.
    fn draw_result(&self, frame: &mut Frame, area: Rect, view: &RequestView) -> Vec<(Rect, usize)> {
        let focused = self.area == Area::Fields;
        let block = |title: &str| {
            Block::bordered()
                .title(format!(" {title} "))
                .border_style(focused_border(focused))
        };
        if self.settled() {
            let request = &view.request;
            let result = request.result.as_ref().map_or_else(
                || request.error.clone().unwrap_or_default(),
                |result| serde_json::to_string_pretty(result).unwrap_or_default(),
            );
            frame.render_widget(
                Paragraph::new(format!("{}\n{result}", request.status))
                    .wrap(Wrap { trim: false })
                    .scroll((self.field_scroll, 0))
                    .block(block("Completed result")),
                area,
            );
            return Vec::new();
        }
        if self.owned() {
            if let Mode::Form(form) = &self.mode {
                view::draw_inline_form(frame, area, form, self.field_scroll, focused);
                return self.field_hits(area);
            }
            frame.render_widget(
                Paragraph::new(
                    "REVIEW\nPress g for GREEN or r for RED, fill its fields, then Ctrl-S submits.",
                )
                .wrap(Wrap { trim: false })
                .block(block("Result fields")),
                area,
            );
            return Vec::new();
        }
        frame.render_widget(
            Paragraph::new(format!("CLAIM\n{}", self.claim_text(view)))
                .wrap(Wrap { trim: false })
                .block(block("Human sign-off")),
            area,
        );
        Vec::new()
    }

    /// What the CLAIM stage allows this reviewer.
    fn claim_text(&self, view: &RequestView) -> String {
        view.claim.as_ref().map_or_else(
            || {
                format!(
                    "Unclaimed. c claims it for {} and starts REVIEW.",
                    self.reviewer
                )
            },
            |claim| format!("Claimed by {}. Read-only until released.", claim.reviewer),
        )
    }

    fn draw_buttons(&self, frame: &mut Frame, area: Rect, hits: &mut Hits) {
        let editing = self.editing();
        let verdict = |letter: &'static str, control: &'static str| {
            if editing { control } else { letter }
        };
        let controls: Vec<(String, Control)> = if self.busy() || self.settled() {
            Vec::new()
        } else if self.owned() {
            vec![
                (format!("Release {}", verdict("u", "^U")), Control::Release),
                (format!("GREEN {}", verdict("g", "^G")), Control::Green),
                (format!("RED {}", verdict("r", "^R")), Control::Red),
                ("Submit ^S".into(), Control::Submit),
                ("Run tool ⏎".into(), Control::RunTool),
            ]
        } else {
            vec![("Claim c".into(), Control::Claim)]
        };
        let mut x = area.x;
        let mut place = |label: &str| {
            let text = format!("[{label}]");
            let width = Line::from(text.as_str()).width() as u16;
            if x.saturating_add(width) > area.right() {
                return None;
            }
            let rect = Rect::new(x, area.y, width, area.height);
            frame.render_widget(Line::from(text).bold().cyan(), rect);
            x = x.saturating_add(width).saturating_add(BUTTON_GAP);
            Some(rect)
        };
        for (label, control) in controls {
            match place(&label) {
                Some(rect) => hits.buttons.push((rect, control)),
                None => return,
            }
        }
        if let Some(rect) = place("Close") {
            hits.close = rect;
        }
    }
}
