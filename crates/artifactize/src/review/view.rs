//! Drawing only; text comes from the review state and the saved request.

use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Cell, Clear, Paragraph, Row, Table, Wrap},
};
use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{Form, Input, Mode, Review, shell};
use crate::{
    config::HumanToolKind,
    monitor::{clock, duration},
    store::RequestView,
};

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

fn kind(kind: HumanToolKind) -> &'static str {
    match kind {
        HumanToolKind::Launch => "launch",
        HumanToolKind::Output => "output",
    }
}

fn compact(schema: Option<&Value>) -> String {
    schema.map_or("none (no owner fields)".into(), Value::to_string)
}

/// Request facts shown before any claim; a follower names the request its actions go to.
fn details(view: &RequestView, reviewer: &str) -> Vec<(&'static str, String)> {
    let request = &view.request;
    let definition = request.human_definition.as_ref();
    let declaration = definition.map(|definition| &definition["eval"]["declaration"]);
    let mut fields = vec![
        ("Request", request.id.clone()),
        ("Run", request.run_id.clone()),
        (
            "Repository",
            definition
                .and_then(|definition| definition["repo"].as_str())
                .unwrap_or("-")
                .to_owned(),
        ),
    ];
    let status = match (&request.error, &request.error_code) {
        (Some(error), Some(code)) => format!("{} [{code}] {error}", request.status),
        (Some(error), None) => format!("{} {error}", request.status),
        _ => request.status.clone(),
    };
    fields.push(("Status", status));
    let claim = match &view.claim {
        None if request.status == "WAITING_HUMAN" => {
            format!("unclaimed; running a tool or submitting claims it for {reviewer}")
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
    fields.push(("Claim", claim));
    let source = view
        .execution
        .as_ref()
        .map(|execution| &execution.provenance);
    if let Some(source) = source.filter(|source| source.request_id != request.id) {
        fields.push((
            "Shared",
            format!(
                "actions go to request {} in {}",
                source.request_id,
                source.repo_path.display()
            ),
        ));
    }
    fields.push((
        "Instruction",
        request.payload["instruction"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
    ));
    let schema = |key| compact(declaration.and_then(|declaration| declaration.get(key)));
    fields.push(("GREEN fields", schema("passSchema")));
    fields.push(("RED fields", schema("failSchema")));
    fields
}

fn lines(fields: Vec<(&'static str, String)>) -> Vec<Line<'static>> {
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

impl Review {
    pub fn draw(&mut self, frame: &mut Frame) {
        let now = OffsetDateTime::now_utc();
        let status = self.status_line();
        let [header, body, status_area, keys] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(status.len() as u16),
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
                "artifactize review".bold(),
                format!(
                    "  {scope} · reviewer {} · state {} · {refreshed}",
                    self.reviewer,
                    self.state.display()
                )
                .into(),
            ]),
            header,
        );
        frame.render_widget(Paragraph::new(status), status_area);
        frame.render_widget(Line::from(self.help()).dark_gray(), keys);
        if self.mode == Mode::List {
            self.draw_list(frame, body, now);
        } else {
            self.draw_request(frame, body);
        }
    }

    fn status_line(&self) -> Vec<Line<'static>> {
        if let Some(busy) = &self.busy {
            let elapsed = busy.since.elapsed();
            let frame = SPINNER[(elapsed.as_millis() / 100) as usize % SPINNER.len()];
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
            .take(4)
            .map(|line| Line::from(line.to_owned()).fg(style))
            .collect()
    }

    fn help(&self) -> &'static str {
        if self.busy.is_some() {
            return "Esc cancel · PgUp/PgDn scroll output";
        }
        match &self.mode {
            Mode::List => "j/k ↑/↓ move · Enter open · r refresh · q/Esc quit",
            Mode::Request => {
                "j/k ↑/↓ tool · Enter run tool · s submit · u unclaim · PgUp/PgDn scroll output · r refresh · Esc list · q quit"
            }
            Mode::Confirm { .. } => "y/Enter run · n/Esc cancel",
            Mode::Verdict => "g GREEN · r RED · Esc cancel",
            Mode::Form(Form { json: Some(_), .. }) => {
                "Enter submit · e edit in $EDITOR · Esc cancel"
            }
            Mode::Form(_) => {
                "↑/↓ Tab field · type to edit · Space/←/→ choose · Enter submit · Ctrl-E edit as JSON in $EDITOR · Esc cancel"
            }
            Mode::Leave => "k keep claims and quit · u release and quit · Esc stay",
        }
    }

    fn draw_list(&mut self, frame: &mut Frame, area: Rect, now: OffsetDateTime) {
        let block =
            Block::bordered().title(format!(" Waiting Human reviews ({}) ", self.waiting.len()));
        if self.waiting.is_empty() {
            let text = if self.refreshed.is_some() {
                "No Human reviews are waiting."
            } else {
                "Loading…"
            };
            frame.render_widget(Paragraph::new(text).block(block), area);
            return;
        }
        let rows = self.waiting.iter().map(|view| {
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
            Row::new(vec![
                Cell::from(request.eval_id.clone()),
                Cell::from(request.id.clone()),
                Cell::from(claim),
                Cell::from(age),
                Cell::from(repo.unwrap_or("-").to_owned()),
            ])
        });
        let table = Table::new(
            rows,
            [
                Constraint::Fill(2),
                Constraint::Fill(3),
                Constraint::Length(16),
                Constraint::Length(8),
                Constraint::Fill(2),
            ],
        )
        .header(Row::new(["EVAL", "REQUEST", "CLAIM", "WAITING", "REPO"]).bold())
        .row_highlight_style(Modifier::REVERSED)
        .block(block);
        frame.render_stateful_widget(table, area, &mut self.list);
    }

    fn draw_request(&mut self, frame: &mut Frame, area: Rect) {
        let Some(view) = &self.request else {
            frame.render_widget(Paragraph::new("Loading…").block(Block::bordered()), area);
            return;
        };
        let title = format!(" {} · {} ", view.request.eval_id, view.request.title);
        let text = lines(details(view, &self.reviewer));
        let height = (text.len() as u16 + 2).min(area.height / 2);
        let [top, bottom] =
            Layout::vertical([Constraint::Length(height), Constraint::Fill(1)]).areas(area);
        frame.render_widget(
            Paragraph::new(text)
                .wrap(Wrap { trim: false })
                .block(Block::bordered().title(title)),
            top,
        );
        if let Mode::Form(form) = &self.mode {
            draw_form(frame, bottom, form);
            return;
        }
        let [left, right] =
            Layout::horizontal([Constraint::Percentage(40), Constraint::Fill(1)]).areas(bottom);
        self.draw_tools(frame, left);
        self.draw_output(frame, right);
        let eval = view.request.eval_id.clone();
        match &self.mode {
            Mode::Confirm { tool, command } => {
                let words = std::iter::once(command.program.to_string_lossy().into_owned())
                    .chain(command.args.iter().cloned())
                    .collect::<Vec<_>>();
                let fields = vec![
                    ("Kind", kind(command.kind).to_owned()),
                    ("Repository", command.repo.display().to_string()),
                    ("Directory", command.cwd.display().to_string()),
                    ("Command", shell(words.iter().map(String::as_str))),
                ];
                let mut text = vec![
                    Line::from(format!("Run {tool} for the first time in this session?")).bold(),
                    Line::default(),
                ];
                text.extend(lines(fields));
                text.extend([Line::default(), Line::from("y/Enter run · n/Esc cancel")]);
                popup(frame, area, " Confirm Human tool ".into(), text);
            }
            Mode::Verdict => popup(
                frame,
                area,
                " Submit ".into(),
                vec![
                    Line::from(format!("Submit which verdict for {eval}?")).bold(),
                    Line::default(),
                    Line::from("  g  GREEN: criteria met, fill the GREEN fields").green(),
                    Line::from("  r  RED: criteria not met, fill the RED fields").red(),
                    Line::default(),
                    Line::from("Esc cancel"),
                ],
            ),
            Mode::Leave => {
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
                popup(frame, area, " Quit ".into(), text);
            }
            _ => {}
        }
    }

    fn draw_tools(&self, frame: &mut Frame, area: Rect) {
        let tools = self.tools();
        let block = Block::bordered().title(format!(" Human tools ({}) ", tools.len()));
        if tools.is_empty() {
            frame.render_widget(
                Paragraph::new("No Human tools are declared in this eval's scope.").block(block),
                area,
            );
            return;
        }
        let mut text = Vec::new();
        for (index, tool) in tools.iter().enumerate() {
            let name = Line::from(format!("{} ({})", tool.name, kind(tool.kind)));
            text.push(if index == self.tool {
                name.add_modifier(Modifier::REVERSED)
            } else {
                name.bold()
            });
            text.push(Line::from(format!("  $ {}", tool.declared)));
            text.push(Line::from(format!("  {}", tool.description)).dark_gray());
        }
        let inner = area.height.saturating_sub(2);
        let scroll = (3 * (self.tool as u16 + 1)).saturating_sub(inner);
        frame.render_widget(Paragraph::new(text).scroll((scroll, 0)).block(block), area);
    }

    fn draw_output(&self, frame: &mut Frame, area: Rect) {
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

fn draw_form(frame: &mut Frame, area: Rect, form: &Form) {
    let color = if form.verdict == "GREEN" {
        Color::Green
    } else {
        Color::Red
    };
    let block = Block::bordered().title(Span::styled(
        format!(" {} fields ", form.verdict),
        Style::new().fg(color),
    ));
    let mut text = Vec::new();
    if let Some(json) = &form.json {
        text.push(Line::from("Owner fields as JSON (e opens $EDITOR, Enter submits):").bold());
        text.extend(json.lines().map(|line| Line::from(format!("  {line}"))));
    } else if form.fields.is_empty() {
        text.push(Line::from(
            "This verdict has no owner fields; Enter submits it.",
        ));
    }
    if form.json.is_none() {
        for (index, field) in form.fields.iter().enumerate() {
            let marker = if field.required { "*" } else { "" };
            let mut value = field.display();
            let editable = matches!(
                field.input,
                Input::Text(_) | Input::Integer(_) | Input::Number(_)
            );
            if index == form.selected && editable {
                value.push('▏');
            }
            let mut line = Line::from(vec![
                Span::styled(format!("{}{marker}: ", field.name), Modifier::BOLD),
                Span::raw(value),
                Span::styled(format!("   {}", field.hint), Style::new().dark_gray()),
            ]);
            if index == form.selected {
                line = line.add_modifier(Modifier::REVERSED);
            }
            if matches!(field.input, Input::Fixed(_)) {
                line = line.dark_gray();
            }
            text.push(line);
        }
    }
    if let Some(error) = &form.error {
        text.push(Line::default());
        text.extend(error.lines().map(|line| Line::from(line.to_owned()).red()));
    }
    frame.render_widget(
        Paragraph::new(text).wrap(Wrap { trim: false }).block(block),
        area,
    );
}
