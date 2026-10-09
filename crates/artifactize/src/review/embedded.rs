//! Lifecycle controls of one open request: claim, verdict forms, tools, submit and release.
//! The shared Detail component and monitor's buttons drive them; jobs and the ownership
//! protocol are the same for monitor and the standalone review.
use super::{Action, Area, Form, Job, Mode, Outcome, Review, view};
use crate::{store::RequestView, types::RequestStatus};
use ratatui::layout::Rect;

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
    /// Monitor's single-request review: monitor owns the terminal, so no `$EDITOR`, and the
    /// waiting list is not read.
    pub(crate) fn load_single(&mut self, view: RequestView) {
        self.standalone = false;
        self.open = Some(view.request.id.clone());
        self.invalid_open = None;
        self.show(view);
    }

    /// Take the latest saved view of the open request. Once it is settled (here or by an
    /// external submission) nothing is left to edit or confirm: the form is set aside and a
    /// pending tool confirmation is dropped, so keys act on the completed Detail again.
    pub(super) fn show(&mut self, view: RequestView) {
        self.request = Some(view);
        if self.settled() {
            self.stop_editing();
            if matches!(self.mode, Mode::Confirm { .. }) {
                self.mode = Mode::Request;
                self.tool_draft = None;
            }
        }
    }

    pub(crate) fn owned(&self) -> bool {
        self.mine()
    }
    pub(crate) fn settled(&self) -> bool {
        self.request
            .as_ref()
            .is_some_and(|view| view.request.status != RequestStatus::WaitingHuman)
    }
    /// A form takes the keys only while the request can still be reviewed.
    pub(crate) fn editing(&self) -> bool {
        matches!(self.mode, Mode::Form(_)) && !self.settled()
    }
    /// A tool confirmation holds the keys (and monitor's focus) only while the request can
    /// still be reviewed.
    pub(crate) fn confirming(&self) -> bool {
        matches!(self.mode, Mode::Confirm { .. }) && !self.settled()
    }
    pub(crate) fn select_area(&mut self, area: Area) {
        self.area = area;
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
            && self.editing()
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
            return match control {
                Control::Confirm => self.confirm(true),
                Control::Cancel => self.confirm(false),
                _ => Action::None,
            };
        }
        if self.settled() {
            return Action::None;
        }
        if control == Control::Claim {
            return match self.actionable() {
                Ok((id, true)) => Action::Start(Job::Claim { id }),
                Ok(_) => Action::None,
                Err(error) => self.notify(error, true),
            };
        }
        if control == Control::Cancel {
            self.stop_editing();
            return Action::None;
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
                // Typing goes to the form, so the fields take focus.
                self.area = Area::Fields;
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
            Control::Release => self.release(),
            Control::RunTool => {
                if let Mode::Form(form) = &self.mode {
                    self.tool_draft = Some(form.clone());
                }
                let action = self.inspect();
                if !matches!(action, Action::Start(_)) {
                    self.tool_draft = None;
                }
                action
            }
            _ => Action::None,
        }
    }

    /// Monitor keeps a completed request open instead of returning to a list.
    pub(crate) fn finish_single(&mut self, outcome: Outcome) -> Action {
        let submitted = match &outcome {
            Outcome::Submitted {
                result: Ok(request),
                ..
            } => Some((**request).clone()),
            _ => None,
        };
        let old = self.request.clone();
        let action = self.finish(outcome);
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
}
