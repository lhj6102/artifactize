//! Keyboard and paste routing, including the opened Detail's review or transcript.
use super::{
    Action, DetailArea, Focus, Monitor, Node, Pane, detail::SessionView, input::SCROLL_PAGE,
    layout, loader, model,
};
use crate::review;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

impl Monitor {
    /// Select the next ERROR, RED or waiting Human eval after the selection, wrapping around.
    fn next_attention(&mut self) -> bool {
        fn walk(
            nodes: &[Node],
            parent: &[model::NodeId],
            order: &mut Vec<(Vec<model::NodeId>, bool)>,
        ) {
            for node in nodes {
                let mut path = parent.to_vec();
                path.push(node.id.clone());
                order.push((path.clone(), node.urgent()));
                walk(&node.children, &path, order);
            }
        }
        if self.run.is_none() {
            self.notice = Some("Open a Run to find what needs attention.".into());
            return false;
        }
        let mut order = Vec::new();
        walk(&self.nodes(), &[], &mut order);
        let start = order
            .iter()
            .position(|(path, _)| path.as_slice() == self.tree.selected())
            .map_or(0, |index| index + 1);
        let Some(index) = (0..order.len())
            .map(|offset| (start + offset) % order.len())
            .find(|index| order[*index].1)
        else {
            self.notice = Some("Nothing in this Run needs attention.".into());
            return false;
        };
        let path = order[index].0.clone();
        for end in 1..path.len() {
            self.tree.open(path[..end].to_vec());
        }
        self.tree.select(path);
        self.focus_on(Pane::Artifacts);
        true
    }
    pub fn key(&mut self, key: KeyEvent) -> Action {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            if let Some(review) = self.review_mut()
                && review.busy()
            {
                return Action::Review(review.control(review::Control::Cancel));
            }
            return Action::Quit;
        }
        if key.code == KeyCode::F(2) {
            self.mouse_capture = !self.mouse_capture;
            return Action::Capture(self.mouse_capture);
        }
        if self.help {
            self.help = false;
            return Action::None;
        }
        if matches!(self.focus, Focus::Detail(_)) {
            return self.detail_key(key);
        }
        self.notice = None;
        match key.code {
            KeyCode::Char('q') => return Action::Quit,
            KeyCode::Char('?') => {
                self.help = true;
                return Action::None;
            }
            KeyCode::Char('!') => {
                self.next_attention();
                return Action::None;
            }
            // Back one level; the first level stays, and only q or Ctrl-C quit.
            KeyCode::Left | KeyCode::Esc => {
                self.focus_on(layout::back(self.focus()));
                return Action::None;
            }
            KeyCode::Right => {
                return match self.focus() {
                    Pane::Repositories => {
                        self.focus = Focus::Runs;
                        Action::None
                    }
                    // Keep the former Runs→Artifacts refresh without dispatching to the tree.
                    Pane::Runs => {
                        self.focus = Focus::Artifacts;
                        self.navigation = true;
                        Action::Refresh
                    }
                    Pane::Artifacts | Pane::Detail => Action::OpenDetail,
                };
            }
            KeyCode::Char('r') => {
                self.navigation = false;
                return Action::Resync;
            }
            KeyCode::Tab => {
                self.focus = match self.focus() {
                    Pane::Repositories => Focus::Runs,
                    Pane::Runs => Focus::Artifacts,
                    Pane::Artifacts | Pane::Detail => Focus::Repositories,
                }
            }
            KeyCode::BackTab => {
                self.focus = match self.focus() {
                    Pane::Repositories => Focus::Artifacts,
                    Pane::Runs => Focus::Repositories,
                    Pane::Artifacts | Pane::Detail => Focus::Runs,
                }
            }
            _ => {}
        }
        match self.focus() {
            Pane::Repositories => match key.code {
                KeyCode::Down | KeyCode::Char('j') => {
                    return self
                        .select_scope(self.repositories.selected().unwrap_or(0).saturating_add(1));
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    return self
                        .select_scope(self.repositories.selected().unwrap_or(0).saturating_sub(1));
                }
                KeyCode::Char(' ') => {
                    let fold = self
                        .repositories
                        .selected()
                        .and_then(|index| self.catalog.rows.get(index))
                        .and_then(|row| row.fold.clone());
                    if let Some(repository) = fold {
                        self.catalog.toggle(&repository);
                        self.load_revision += 1;
                        self.navigation = true;
                        return Action::Refresh;
                    }
                }
                KeyCode::Enter => self.focus = Focus::Runs,
                _ => {}
            },
            Pane::Runs => match key.code {
                KeyCode::Down | KeyCode::Char('j') => {
                    let next = self.list.selected().map_or(0, |index| index + 1);
                    if next < self.runs.len() {
                        return self.select_run(next);
                    }
                    if let Some(offset) =
                        loader::older_offset(self.offset, self.limit, self.runs.len() as u32)
                    {
                        self.offset = offset;
                        self.page_tail = false;
                        self.load_revision += 1;
                        self.navigation = true;
                        self.runs.clear();
                        self.list.select(None);
                        self.open = None;
                        return Action::Refresh;
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if self.list.selected().unwrap_or(0) == 0
                        && let Some(offset) = loader::newer_offset(self.offset, self.limit)
                    {
                        self.offset = offset;
                        self.page_tail = true;
                        self.load_revision += 1;
                        self.navigation = true;
                        self.runs.clear();
                        self.list.select(None);
                        self.open = None;
                        return Action::Refresh;
                    }
                    return self.select_run(self.list.selected().unwrap_or(0).saturating_sub(1));
                }
                KeyCode::Enter => {
                    self.focus = Focus::Artifacts;
                    self.navigation = true;
                    return Action::Refresh;
                }
                _ => {}
            },
            Pane::Artifacts => match key.code {
                KeyCode::Down | KeyCode::Char('j') => {
                    self.tree.key_down();
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.tree.key_up();
                }
                KeyCode::Char('h' | 'l' | ' ') => {
                    let before = self.tree.opened().clone();
                    match key.code {
                        KeyCode::Char('h') => self.tree.key_left(),
                        KeyCode::Char('l') => self.tree.key_right(),
                        _ => self.tree.toggle_selected(),
                    };
                    self.touched(&before);
                }
                KeyCode::Char('b') => self.jump_blocker(),
                KeyCode::Backspace => self.jump_back(),
                KeyCode::Enter | KeyCode::Char('o') => return Action::OpenDetail,
                _ => {}
            },
            Pane::Detail => {}
        }
        Action::None
    }
    /// Keys while Detail has focus. A Human review owns them through the shared component.
    fn detail_key(&mut self, key: KeyEvent) -> Action {
        let Focus::Detail(pane) = &mut self.focus else {
            return Action::None;
        };
        if let Some(review) = pane.content.review_mut() {
            match review.key_detail(key) {
                review::Handled::Action(action) => return Action::Review(action),
                review::Handled::Back => {
                    self.leave_detail();
                    return Action::None;
                }
                review::Handled::Quit => return Action::Quit,
                // `?` and `!` outside a form act as in any Detail; leaving keeps the draft.
                review::Handled::Pass => {}
            }
        }
        if key.code == KeyCode::Esc {
            if pane.content.session_view() == Some(SessionView::Sections) {
                pane.toggle_session_view();
                return Action::None;
            }
            self.leave_detail();
            return Action::None;
        }
        // A Human review passes only `?` and `!`, outside a form or job.
        match key.code {
            KeyCode::Char('q') => return Action::Quit,
            KeyCode::Char('?') => {
                self.help = true;
                return Action::None;
            }
            KeyCode::Left => {
                self.leave_detail();
                return Action::None;
            }
            // The next attention item opens in Detail, so causes read one after another.
            KeyCode::Char('!') => {
                self.leave_detail();
                return if self.next_attention() {
                    Action::OpenDetail
                } else {
                    Action::None
                };
            }
            _ => {}
        }
        let Focus::Detail(pane) = &mut self.focus else {
            return Action::None;
        };
        let transcript = pane.content.session_view() == Some(SessionView::Transcript);
        match key.code {
            KeyCode::Char('t') if !transcript => {
                pane.technical = !pane.technical;
                return Action::None;
            }
            KeyCode::Char('w') if !transcript => {
                pane.pending = !pane.pending;
                return Action::None;
            }
            _ => {}
        }
        if key.code == KeyCode::Char('d') {
            pane.toggle_session_view();
            return Action::None;
        }
        if transcript {
            pane.focus = DetailArea::Evidence;
        }
        if pane.focus == DetailArea::Evidence
            && let Some(live) = pane.content.live_mut()
        {
            use crate::agent::session::document::Move;
            if matches!(key.code, KeyCode::Enter | KeyCode::Char(' ')) {
                live.toggle_visible();
                return Action::None;
            }
            let movement = match key.code {
                KeyCode::Up | KeyCode::Char('k') => Some(Move::Up(1)),
                KeyCode::Down | KeyCode::Char('j') => Some(Move::Down(1)),
                KeyCode::PageUp => Some(Move::Up(live.scroll.height.max(1))),
                KeyCode::PageDown => Some(Move::Down(live.scroll.height.max(1))),
                KeyCode::Home => Some(Move::Top),
                KeyCode::End => Some(Move::Bottom),
                _ => None,
            };
            if let Some(movement) = movement {
                live.movement(movement);
                return Action::None;
            }
        }
        let index = usize::from(pane.focus == DetailArea::Evidence);
        match key.code {
            KeyCode::Tab | KeyCode::BackTab => {
                pane.focus = if pane.focus == DetailArea::Summary && !pane.evidence.is_empty() {
                    DetailArea::Evidence
                } else {
                    DetailArea::Summary
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                pane.scroll[index] = pane.scroll[index].saturating_add(1)
            }
            KeyCode::Up | KeyCode::Char('k') => {
                pane.scroll[index] = pane.scroll[index].saturating_sub(1)
            }
            KeyCode::PageDown => {
                pane.scroll[index] = pane.scroll[index].saturating_add(SCROLL_PAGE)
            }
            KeyCode::PageUp => pane.scroll[index] = pane.scroll[index].saturating_sub(SCROLL_PAGE),
            KeyCode::Home => pane.scroll[index] = 0,
            _ => {}
        }
        Action::None
    }
    pub fn paste(&mut self, text: &str) {
        if let Some(review) = self.review_mut()
            && review.area() == review::Area::Fields
        {
            review.paste_single(text);
        }
    }
}
