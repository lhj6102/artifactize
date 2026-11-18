//! Input for the standalone review: list, Detail, and the prompt when leaving.
use super::{
    Action, Area, Control, Focus, Handled, Job, Key, Mode, RequestView, Review, Shown, view,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Position;

/// A key event as the form edits with it, and whether Control or Alt is held.
pub(crate) fn form_key(key: KeyEvent) -> (Key, bool) {
    let modified = key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
    let code = match key.code {
        KeyCode::Char(character) => Key::Char(character),
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Delete => Key::Delete,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::Enter => Key::Enter,
        KeyCode::Tab => Key::Tab,
        KeyCode::BackTab => Key::BackTab,
        _ => Key::Other,
    };
    (code, modified)
}

/// A wheel tick moves three rows, as in monitor.
const WHEEL_ROWS: i16 = 3;

impl Review {
    pub(super) fn quit(&mut self) -> Action {
        if self.taken.is_empty() {
            return Action::Quit;
        }
        // The prompt never discards a form; Esc returns and g or r reopens the draft.
        self.stop_editing();
        self.mode = Mode::Leave;
        Action::None
    }

    /// Leave the form without discarding it; the same verdict reopens the draft.
    /// Only a form is set aside; any other mode, such as the quit prompt, stays.
    pub(super) fn stop_editing(&mut self) {
        if !matches!(self.mode, Mode::Form(_)) {
            return;
        }
        if let Mode::Form(form) = std::mem::replace(&mut self.mode, Mode::Request) {
            self.drafts.insert(form.verdict, form);
        }
    }

    /// Standalone keys: the waiting list, then the shared Human review Detail. Esc steps back
    /// and never quits; q and Ctrl-C quit and offer to release this session's claims.
    pub fn key(&mut self, key: KeyEvent) -> Action {
        let interrupt =
            key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c');
        // F2 turns mouse capture on or off in any state, as in monitor; paste works either way.
        if key.code == KeyCode::F(2) {
            self.mouse_capture = !self.mouse_capture;
            return Action::Capture(self.mouse_capture);
        }
        if self.busy.is_some() {
            return match self.key_detail(key) {
                Handled::Action(action) => action,
                Handled::Back | Handled::Quit | Handled::Pass => Action::None,
            };
        }
        if interrupt {
            return if self.mode == Mode::Leave {
                Action::Quit
            } else {
                self.quit()
            };
        }
        if !matches!(self.mode, Mode::Form(_)) {
            self.notice = None;
        }
        if self.mode == Mode::Leave {
            return match key.code {
                KeyCode::Char('k' | 'n' | 'q') => Action::Quit,
                KeyCode::Char('u' | 'r' | 'y') => Action::Start(Job::Release {
                    ids: self.taken.clone(),
                    quit: true,
                }),
                KeyCode::Esc => {
                    self.mode = Mode::Request;
                    Action::None
                }
                _ => Action::None,
            };
        }
        match self.focus {
            Focus::List if self.editing_query.is_some() => self.query_key(key),
            Focus::List => self.list_key(key),
            Focus::Detail if self.shown.record().is_some() || self.shown.request().is_none() => {
                self.record_key(key)
            }
            Focus::Detail => match self.key_detail(key) {
                Handled::Action(action) => action,
                Handled::Back => {
                    self.focus = Focus::List;
                    Action::Refresh
                }
                Handled::Quit => self.quit(),
                // The standalone review has no help overlay or attention order.
                Handled::Pass => Action::None,
            },
        }
    }

    /// Standalone mouse: a click focuses and selects, a click on the selected list row opens it,
    /// and inside the Detail it acts like monitor's Human Detail; the wheel scrolls.
    pub fn mouse(&mut self, event: MouseEvent) -> Action {
        if !self.mouse_capture || self.mode == Mode::Leave {
            return Action::None;
        }
        let point = Position::new(event.column, event.row);
        let hits = std::mem::take(&mut self.hits);
        let action = self.mouse_at(&hits, event.kind, point);
        self.hits = hits;
        action
    }

    fn mouse_at(&mut self, hits: &view::Hits, kind: MouseEventKind, point: Position) -> Action {
        let in_detail = hits.detail.contains(point);
        match kind {
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let down = kind == MouseEventKind::ScrollDown;
                if in_detail {
                    let delta = if down { WHEEL_ROWS } else { -WHEEL_ROWS };
                    if hits.review.instruction.contains(point) {
                        self.scroll_instruction(delta);
                    } else if hits.review.fields.contains(point) {
                        self.scroll_single(delta, true);
                    } else if hits.review.tools.contains(point) {
                        self.scroll_single(delta, false);
                    }
                } else if hits.list.contains(point) && self.focus == Focus::List {
                    return self.list_key(KeyEvent::from(if down {
                        KeyCode::Down
                    } else {
                        KeyCode::Up
                    }));
                }
                Action::None
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if in_detail {
                    if hits.review.close.contains(point) {
                        if self.busy() {
                            return self.control(Control::Cancel);
                        }
                        return match self.key_detail(KeyEvent::from(KeyCode::Esc)) {
                            Handled::Back => {
                                self.focus = Focus::List;
                                Action::Refresh
                            }
                            Handled::Action(action) => action,
                            Handled::Quit | Handled::Pass => Action::None,
                        };
                    }
                    return self.click(&hits.review, point);
                }
                // A working review or an open form keeps the Detail.
                if self.focus == Focus::Detail && (self.busy() || self.editing()) {
                    return Action::None;
                }
                if hits.preview.contains(point) {
                    return match self.selected().cloned() {
                        Some(view) => self.open_view(view),
                        None => Action::None,
                    };
                }
                let Some(&(_, index)) = hits.rows.iter().find(|(rect, _)| rect.contains(point))
                else {
                    if hits.list.contains(point) {
                        self.focus = Focus::List;
                    }
                    return Action::None;
                };
                let open = self.focus == Focus::List && self.list.selected() == Some(index);
                self.focus = Focus::List;
                self.list.select(Some(index));
                match (open, self.listed.get(index).cloned()) {
                    (true, Some(view)) => self.open_view(view),
                    _ => Action::None,
                }
            }
            _ => Action::None,
        }
    }

    /// Open a listed request in Detail; another request starts from a clean review.
    pub(super) fn open_view(&mut self, view: RequestView) -> Action {
        if view.request.profile.kind() != crate::config::ProfileKind::Human {
            // Read-only: the record loads with the refresh this returns.
            // A record of the same request stays, to be read again by the refresh.
            if self.open.as_ref() != Some(&view.request.id) || self.shown.request().is_some() {
                self.shown = Shown::Nothing;
            }
            self.open = Some(view.request.id.clone());
            self.focus = Focus::Detail;
            return Action::Refresh;
        }
        if self.open.as_ref() != Some(&view.request.id) {
            self.mode = Mode::Request;
            self.area = Area::Fields;
            self.tool = 0;
            self.log.clear();
            self.scroll = None;
            self.field_scroll = 0;
            self.instruction_scroll = 0;
            self.drafts.clear();
        }
        self.open = Some(view.request.id.clone());
        self.shown = Shown::Human(Box::new(view));
        self.focus = Focus::Detail;
        Action::Refresh
    }

    /// A bracketed paste goes to the focused field of the open review, as in monitor.
    /// Paste into the search being edited (one line, searched from the first page like typed
    /// text) or into the focused form field.
    pub fn paste(&mut self, text: &str) -> Action {
        if self.focus == Focus::List && self.editing_query.is_some() {
            return self.paste_query(text);
        }
        if self.focus == Focus::Detail && self.area == Area::Fields {
            self.paste_single(text);
        }
        Action::None
    }
}
