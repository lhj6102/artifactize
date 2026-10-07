//! Single-request controller used by monitor; jobs and ownership protocol are shared with review.
use super::{Action, Form, Job, Mode, Outcome, Review, shell, view};
use crate::{store::RequestView, types::RequestStatus};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    text::Line,
    widgets::{Block, Paragraph, Wrap},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Claim,
    Release,
    Green,
    Red,
    Submit,
    RunTool,
    Confirm,
    Cancel,
}

impl Review {
    pub(crate) fn load_single(&mut self, view: RequestView) {
        self.open = Some(view.request.id.to_string());
        self.request = Some(view);
    }

    pub(crate) fn owned(&self) -> bool {
        self.mine()
    }
    pub(crate) fn settled(&self) -> bool {
        self.request
            .as_ref()
            .is_some_and(|view| view.request.status != RequestStatus::WaitingHuman)
    }
    pub(crate) fn editing(&self) -> bool {
        matches!(self.mode, Mode::Form(_))
    }
    pub(crate) fn confirming(&self) -> bool {
        matches!(self.mode, Mode::Confirm { .. })
    }
    pub(crate) fn tool_index(&self) -> usize {
        self.tool
    }
    pub(crate) fn selected_tool(&mut self, index: usize) {
        self.tool = index.min(self.tools().len().saturating_sub(1));
    }
    pub(crate) fn scroll_instruction(&mut self, delta: i16) {
        self.instruction_scroll = self.instruction_scroll.saturating_add_signed(delta);
    }
    pub(crate) fn scroll_single(&mut self, delta: i16, fields: bool) {
        if fields {
            self.field_scroll = self.field_scroll.saturating_add_signed(delta);
        } else {
            self.scroll = self.scroll.saturating_add_signed(delta);
        }
    }
    pub(crate) fn paste_single(&mut self, text: &str) {
        if !self.busy()
            && self.owned()
            && let Mode::Form(form) = &mut self.mode
        {
            form.paste(text);
        }
    }
    pub(crate) fn field_hits(&self, area: Rect) -> Vec<(Rect, usize)> {
        if self.owned()
            && let Mode::Form(form) = &self.mode
        {
            view::field_hits(area, form, self.field_scroll)
        } else {
            Vec::new()
        }
    }
    pub(crate) fn select_field(&mut self, index: usize) {
        if let Mode::Form(form) = &mut self.mode {
            form.selected = index.min(form.fields.len().saturating_sub(1));
        }
    }

    pub(crate) fn control(&mut self, control: Control) -> Action {
        if self.busy() {
            if control == Control::Cancel
                && let Some(busy) = &self.busy
            {
                busy.cancel.cancel();
            }
            return Action::None;
        }
        if self.confirming() {
            let action = match control {
                Control::Confirm => self.key(KeyEvent::from(KeyCode::Enter)),
                Control::Cancel => self.key(KeyEvent::from(KeyCode::Esc)),
                _ => Action::None,
            };
            if control == Control::Cancel
                && let Some(form) = self.tool_draft.take()
            {
                self.mode = Mode::Form(form);
            }
            return action;
        }
        if self.settled() {
            return Action::None;
        }
        if control == Control::Claim {
            return match self.actionable() {
                Ok((id, true)) => match id.parse() {
                    Ok(id) => Action::Start(Job::Claim { id }),
                    Err(error) => self.notify(error, true),
                },
                Ok(_) => Action::None,
                Err(error) => self.notify(error, true),
            };
        }
        if self.actionable().is_err() || !self.owned() {
            return self.notify("Claim this request before reviewing it.", true);
        }
        match control {
            Control::Green | Control::Red => {
                let verdict = if control == Control::Green {
                    "GREEN"
                } else {
                    "RED"
                };
                if matches!(&self.mode, Mode::Form(form) if form.verdict == verdict) {
                    return Action::None;
                }
                let key = if control == Control::Green {
                    "passSchema"
                } else {
                    "failSchema"
                };
                let schema = self
                    .request
                    .as_ref()
                    .and_then(|view| view.request.human_definition.as_ref())
                    .and_then(|definition| definition["eval"]["declaration"].get(key));
                let form = self
                    .drafts
                    .remove(verdict)
                    .unwrap_or_else(|| Form::new(verdict, schema));
                if let Mode::Form(previous) = &self.mode {
                    self.drafts.insert(previous.verdict, previous.clone());
                }
                self.mode = Mode::Form(form);
                Action::None
            }
            Control::Submit => {
                let result = match &self.mode {
                    Mode::Form(form) => form.result(),
                    _ => return self.notify("Choose GREEN or RED first.", true),
                };
                match result {
                    Ok(result) => self.submit(result),
                    Err(error) => {
                        if let Mode::Form(form) = &mut self.mode {
                            form.error = Some(error);
                        }
                        Action::None
                    }
                }
            }
            Control::Release => self.request_key(KeyEvent::from(KeyCode::Char('u'))),
            Control::RunTool => {
                if let Mode::Form(form) = &self.mode {
                    self.tool_draft = Some(form.clone());
                }
                self.request_key(KeyEvent::from(KeyCode::Char('t')))
            }
            Control::Cancel => {
                self.mode = Mode::Request;
                Action::None
            }
            _ => Action::None,
        }
    }

    pub(crate) fn key_single(&mut self, key: KeyEvent, tools_focused: bool) -> Action {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::PageDown | KeyCode::PageUp)
        {
            self.scroll_instruction(if key.code == KeyCode::PageDown {
                crate::monitor::input::SCROLL_PAGE as i16
            } else {
                -(crate::monitor::input::SCROLL_PAGE as i16)
            });
            return Action::None;
        }
        if self.busy() {
            return self.key(key);
        }
        if self.confirming() {
            return match key.code {
                KeyCode::Char('y') | KeyCode::Enter => self.control(Control::Confirm),
                KeyCode::Char('n' | 'q') | KeyCode::Esc => self.control(Control::Cancel),
                _ => Action::None,
            };
        }
        if !self.owned() && key.code == KeyCode::Char('c') {
            return self.control(Control::Claim);
        }
        if !self.editing() {
            match key.code {
                KeyCode::Char('c') => return self.control(Control::Claim),
                KeyCode::Char('g') => return self.control(Control::Green),
                KeyCode::Char('r') => return self.control(Control::Red),
                KeyCode::Char('u') => return self.control(Control::Release),
                _ => {}
            }
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('g') => return self.control(Control::Green),
                KeyCode::Char('r') => return self.control(Control::Red),
                KeyCode::Char('u') => return self.control(Control::Release),
                _ => {}
            }
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('s') {
            return self.control(Control::Submit);
        }
        if key.code == KeyCode::Esc {
            return self.control(Control::Cancel);
        }
        if tools_focused {
            return match key.code {
                KeyCode::Enter => self.control(Control::RunTool),
                KeyCode::Up | KeyCode::Down => self.request_key(key),
                _ => Action::None,
            };
        }
        if self.owned()
            && let Mode::Form(form) = &mut self.mode
        {
            form.inline_key(key);
        }
        Action::None
    }

    pub(crate) fn finish_single(&mut self, outcome: Outcome) -> Action {
        let submitted = match &outcome {
            Outcome::Submitted {
                result: Ok(request),
                ..
            } => Some((**request).clone()),
            _ => None,
        };
        let restore_form = matches!(
            &outcome,
            Outcome::Ran { .. } | Outcome::Inspected { result: Err(_), .. }
        );
        let old = self.request.clone();
        let action = self.finish(outcome);
        if restore_form && let Some(form) = self.tool_draft.take() {
            self.mode = Mode::Form(form);
        }
        if let Some(request) = submitted
            && let Some(mut view) = old
        {
            view.request = request;
            view.claim = None;
            self.load_single(view);
            self.mode = Mode::Request;
            self.submitted = false;
        }
        action
    }

    pub(crate) fn cancel_single(&self) {
        if let Some(busy) = &self.busy {
            busy.cancel.cancel();
        }
    }

    /// Tools and their saved output on the left; the existing field form on the right.
    pub(crate) fn draw_single(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        tools_focused: bool,
    ) -> (Rect, Rect, Rect) {
        let instruction = self.request.as_ref().map_or_else(Vec::new, |request| {
            let mut facts = vec![(
                "Instruction",
                request.request.payload.instruction().to_owned(),
            )];
            facts.extend(
                view::details(request, &self.reviewer)
                    .into_iter()
                    .filter(|(name, _)| *name != "Instruction"),
            );
            view::lines(facts)
        });
        // One quarter of the modal keeps criteria visible without taking over tools or fields.
        let height = u16::try_from(instruction.len())
            .unwrap_or(u16::MAX)
            .saturating_add(2)
            .min(area.height / 4);
        let [summary, body] =
            Layout::vertical([Constraint::Length(height), Constraint::Fill(1)]).areas(area);
        frame.render_widget(
            Paragraph::new(instruction)
                .wrap(Wrap { trim: false })
                .scroll((self.instruction_scroll, 0))
                .block(
                    Block::bordered().title(" Request / instruction · wheel or Ctrl-PgUp/PgDn "),
                ),
            summary,
        );
        let [left, right] =
            Layout::horizontal([Constraint::Percentage(45), Constraint::Fill(1)]).areas(body);
        let tool_height = u16::try_from(self.tools().len())
            .unwrap_or(u16::MAX)
            .saturating_mul(3)
            .saturating_add(2)
            .min(left.height / 2);
        let [tools, output] =
            Layout::vertical([Constraint::Length(tool_height), Constraint::Fill(1)]).areas(left);
        self.draw_tools(frame, tools);
        self.draw_output(frame, output);
        if self.settled() {
            let text = self.request.as_ref().map(|view| &view.request).map_or_else(
                String::new,
                |request| {
                    format!(
                        "{}\n{}",
                        request.status,
                        request.result.as_ref().map_or_else(
                            || request.error.clone().unwrap_or_default(),
                            |result| serde_json::to_string_pretty(result).unwrap_or_default()
                        )
                    )
                },
            );
            frame.render_widget(
                Paragraph::new(text)
                    .wrap(Wrap { trim: false })
                    .scroll((self.field_scroll, 0))
                    .block(Block::bordered().title(" Completed result ")),
                right,
            );
        } else if self.confirming() {
            if let Mode::Confirm { tool, command } = &self.mode {
                let words = std::iter::once(command.program.to_string_lossy().into_owned())
                    .chain(command.args.iter().cloned())
                    .collect::<Vec<_>>();
                frame.render_widget(Paragraph::new(format!("Run {tool}?\nRepository: {}\nDirectory: {}\n$ {}\nEnter/Confirm runs; Esc/Cancel returns.", command.repo.display(), command.cwd.display(), shell(words.iter().map(String::as_str)))).wrap(Wrap { trim: false }).block(Block::bordered().title(" Confirm Human tool ")), right);
            }
        } else if self.owned() {
            if let Mode::Form(form) = &self.mode {
                view::draw_inline_form(frame, right, form, self.field_scroll);
            } else {
                frame.render_widget(Paragraph::new("REVIEW\nChoose GREEN or RED, fill its fields, then Submit.\nTab switches tools and fields.").block(Block::bordered().title(" Result fields ")), right);
            }
        } else {
            let claim = self
                .request
                .as_ref()
                .and_then(|view| view.claim.as_ref())
                .map_or("Unclaimed. Claim to enter REVIEW.".into(), |claim| {
                    format!("Claimed by {}. Read-only until released.", claim.reviewer)
                });
            frame.render_widget(
                Paragraph::new(format!("CLAIM\n{claim}"))
                    .wrap(Wrap { trim: false })
                    .block(Block::bordered().title(" Human sign-off ")),
                right,
            );
        }
        let status = self.status_line();
        if !status.is_empty() && area.height > 0 {
            frame.render_widget(
                status.first().cloned().unwrap_or_else(Line::default),
                Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1),
            );
        }
        let _ = tools_focused;
        (tools, right, summary)
    }
}
