//! Shared mouse and keyboard routing for a Human review Detail.
use super::{Action, Area, Control, Mode, Review, SCROLL_PAGE, detail::Hits};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Position;

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

impl Review {
    fn move_tool(&mut self, down: bool) {
        self.tool = if down {
            (self.tool + 1).min(self.tools().len().saturating_sub(1))
        } else {
            self.tool.saturating_sub(1)
        };
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
                KeyCode::Char('d') if self.owned() && self.area == Area::Fields => {
                    if let Mode::Form(form) = &mut self.mode {
                        form.remove_selected();
                    }
                    Action::None
                }
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
                    let (key, modified) = super::form_key(key);
                    form.inline_key(key, modified);
                }
            }
        }
        Action::None
    }
}
