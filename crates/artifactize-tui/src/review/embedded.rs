//! Lifecycle controls of one open request: claim, verdict forms, tools, submit and release.
//! The shared Detail component and monitor's buttons drive them; jobs and the ownership
//! protocol are the same for monitor and the standalone review.
use super::{Action, Area, Form, Job, Mode, Outcome, Review, view};
use crate::store::RequestView;
use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Claim,
    Release,
    Green,
    Red,
    Submit,
    RunTool,
    Cancel,
}

impl Review {
    /// Monitor's single-request review: the waiting list is not read.
    pub(crate) fn load_single(&mut self, view: RequestView) {
        self.standalone = false;
        self.open = Some(view.request.id.clone());
        self.show(view);
    }

    /// Take the latest saved view of the open request. Once it is settled (here or by an
    /// external submission) nothing is left to edit: the form is set aside, so keys act on the
    /// completed Detail again.
    pub(super) fn show(&mut self, view: RequestView) {
        if self
            .commands
            .as_ref()
            .is_some_and(|(id, _)| id != &view.request.id)
        {
            self.commands = None;
        }
        self.shown = super::Shown::Human(Box::new(view));
        if self.settled() {
            self.stop_editing();
        }
    }

    pub(crate) fn owned(&self) -> bool {
        self.mine()
    }
    pub(crate) fn settled(&self) -> bool {
        self.shown
            .request()
            .is_some_and(|view| !view.request.status().is_human_wait())
    }
    /// A form takes the keys only while the request can still be reviewed.
    pub(crate) fn editing(&self) -> bool {
        matches!(self.mode, Mode::Form(_)) && !self.settled()
    }
    pub(crate) fn selected_tool(&mut self, index: usize) {
        self.tool = index.min(self.tools().len().saturating_sub(1));
    }
    pub(crate) fn scroll_instruction(&mut self, delta: i16) {
        self.instruction_scroll = self.instruction_scroll.saturating_add_signed(delta);
    }
    /// Scroll the fields, or the Tool log: scrolled up it stays where the reader left it;
    /// back at the bottom it follows new lines again.
    pub(crate) fn scroll_single(&mut self, delta: i16, fields: bool) {
        if fields {
            self.field_scroll = self.field_scroll.saturating_add_signed(delta);
            return;
        }
        let bottom = self.log_bottom.get();
        let top = self.scroll.unwrap_or(bottom).saturating_add_signed(delta);
        self.scroll = (top < bottom).then_some(top);
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
            form.selected = index.min(form.rows().len().saturating_sub(1));
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
            return self.notify(super::CLAIM_FIRST, true);
        }
        match control {
            Control::Green | Control::Red => {
                let verdict = if control == Control::Green {
                    crate::runtime::Verdict::Green
                } else {
                    crate::runtime::Verdict::Red
                };
                // Typing goes to the form, so the fields take focus.
                self.area = Area::Fields;
                if matches!(&self.mode, Mode::Form(form) if form.verdict == verdict) {
                    return Action::None;
                }
                let schema = match self.shown.request() {
                    Some(view) => super::schema(&view.request, verdict),
                    None => return Action::None,
                };
                let schema = match schema {
                    Ok(schema) => schema,
                    Err(error) => return self.notify(error, true),
                };
                let form = match self.drafts.remove(&verdict) {
                    Some(form) => form,
                    None => match Form::new(verdict, schema.as_ref()) {
                        Ok(form) => form,
                        Err(error) => return self.notify(error, true),
                    },
                };
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
            // The form stays open while the tool runs; no confirmation step.
            Control::RunTool => self.run_selected(),
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
        let old = self.shown.request().cloned();
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
