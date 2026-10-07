//! Terminal protocol and hit tests over geometry from the most recently rendered frame.
use super::{Action, ModalPane, Monitor, Pane};
use crate::review::Control;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Position, Rect};
use std::time::{Duration, Instant};

/// One page is ten logical rows, independent of terminal height and input source.
pub(crate) const SCROLL_PAGE: u16 = 10;
/// Treat a repeated left click within the normal double-click interval as opening detail.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);
/// A wheel tick moves three logical rows rather than jumping a full page.
const WHEEL_ROWS: i16 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Hit {
    Scope(usize),
    Run(usize),
    Artifact(Vec<String>),
}
pub(super) struct Click {
    hit: Hit,
    at: Instant,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Button {
    Review(Control),
    Close,
}
#[derive(Default)]
pub(super) struct Hits {
    pub panes: [Rect; 3],
    pub modal: Rect,
    pub instruction: Rect,
    pub modal_panes: [Rect; 2],
    pub tools: Rect,
    pub field_rows: Vec<(Rect, usize)>,
    pub buttons: Vec<(Rect, Button)>,
}

pub(super) fn protocols(mouse: bool, paste: bool) -> Result<(), String> {
    let mut out = std::io::stdout();
    if mouse {
        crossterm::execute!(out, EnableMouseCapture)
    } else {
        crossterm::execute!(out, DisableMouseCapture)
    }
    .map_err(|error| error.to_string())?;
    if paste {
        crossterm::execute!(out, EnableBracketedPaste)
    } else {
        crossterm::execute!(out, DisableBracketedPaste)
    }
    .map_err(|error| error.to_string())
}
fn contains(rect: Rect, point: Position) -> bool {
    rect.contains(point)
}
impl Monitor {
    pub fn mouse(&mut self, event: MouseEvent) -> Action {
        if !self.mouse_capture {
            return Action::None;
        }
        let point = Position::new(event.column, event.row);
        if let Some(modal) = &mut self.modal {
            // A modal owns every click, even outside its rectangle.
            if event.kind == MouseEventKind::Down(MouseButton::Left) {
                if let Some((_, button)) = self
                    .hits
                    .buttons
                    .iter()
                    .find(|(rect, _)| contains(*rect, point))
                {
                    return match button {
                        Button::Close => {
                            if let Some(review) = &mut modal.review
                                && review.busy()
                            {
                                return Action::Review(review.control(Control::Cancel));
                            }
                            self.close_modal();
                            Action::None
                        }
                        Button::Review(control) => {
                            modal.review.as_mut().map_or(Action::None, |review| {
                                Action::Review(review.control(*control))
                            })
                        }
                    };
                }
                if contains(self.hits.modal_panes[0], point) {
                    modal.focus = if modal.review.is_some() {
                        ModalPane::Tools
                    } else {
                        ModalPane::Summary
                    };
                }
                if contains(self.hits.modal_panes[1], point) {
                    modal.focus = if modal.review.is_some() {
                        ModalPane::Fields
                    } else {
                        ModalPane::Evidence
                    };
                }
                if let Some(review) = &mut modal.review {
                    if contains(self.hits.tools, point) {
                        let inner = self.hits.tools.height.saturating_sub(2);
                        let scroll = (3 * (review.tool_index() as u16 + 1)).saturating_sub(inner);
                        let index = usize::from(
                            point
                                .y
                                .saturating_sub(self.hits.tools.y + 1)
                                .saturating_add(scroll)
                                / 3,
                        );
                        review.selected_tool(index);
                    }
                    if let Some((_, index)) = self
                        .hits
                        .field_rows
                        .iter()
                        .find(|(rect, _)| contains(*rect, point))
                    {
                        review.select_field(*index);
                    }
                }
            }
            let delta = match event.kind {
                MouseEventKind::ScrollDown => WHEEL_ROWS,
                MouseEventKind::ScrollUp => -WHEEL_ROWS,
                _ => return Action::None,
            };
            if contains(self.hits.instruction, point)
                && let Some(review) = &mut modal.review
            {
                review.scroll_instruction(delta);
                return Action::None;
            }
            let index = if contains(self.hits.modal_panes[0], point) {
                0
            } else if contains(self.hits.modal_panes[1], point) {
                1
            } else {
                return Action::None;
            };
            if let Some(review) = &mut modal.review {
                review.scroll_single(delta, index == 1);
            } else {
                modal.scroll[index] = modal.scroll[index].saturating_add_signed(delta);
            }
            return Action::None;
        }
        let pane = self
            .hits
            .panes
            .iter()
            .position(|rect| contains(*rect, point));
        let Some(pane) = pane else {
            return Action::None;
        };
        self.focus = [Pane::Repositories, Pane::Runs, Pane::Artifacts][pane];
        match event.kind {
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let down = event.kind == MouseEventKind::ScrollDown;
                let steps = WHEEL_ROWS as usize;
                match self.focus {
                    Pane::Repositories => {
                        let index = self.repositories.selected().unwrap_or(0);
                        let next = if down {
                            index
                                .saturating_add(steps)
                                .min(self.catalog.rows.len().saturating_sub(1))
                        } else {
                            index.saturating_sub(steps)
                        };
                        self.select_scope(next)
                    }
                    Pane::Runs => {
                        let index = self.list.selected().unwrap_or(0);
                        let next = if down {
                            index
                                .saturating_add(steps)
                                .min(self.runs.len().saturating_sub(1))
                        } else {
                            index.saturating_sub(steps)
                        };
                        if down && next == index && self.runs.len() as u32 == self.limit {
                            self.limit = self.limit.saturating_add(super::PAGE);
                            Action::Refresh
                        } else {
                            self.select_run(next)
                        }
                    }
                    Pane::Artifacts => {
                        for _ in 0..steps {
                            if down {
                                self.tree.key_down();
                            } else {
                                self.tree.key_up();
                            }
                        }
                        Action::None
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let area = self.hits.panes[pane];
                // Borders and Run header are not rows.
                if point.y <= area.y || point.y >= area.bottom().saturating_sub(1) {
                    return Action::None;
                }
                let hit = match self.focus {
                    Pane::Repositories => {
                        Hit::Scope(self.repositories.offset() + usize::from(point.y - area.y - 1))
                    }
                    Pane::Runs if point.y > area.y + 1 => {
                        Hit::Run(self.list.offset() + usize::from(point.y - area.y - 2))
                    }
                    Pane::Runs => return Action::None,
                    Pane::Artifacts => match self.tree.rendered_at(point) {
                        Some(path) => Hit::Artifact(path.to_vec()),
                        None => return Action::None,
                    },
                };
                let now = Instant::now();
                let double = self.last_click.as_ref().is_some_and(|click| {
                    click.hit == hit && now.duration_since(click.at) <= DOUBLE_CLICK
                });
                self.last_click = Some(Click {
                    hit: hit.clone(),
                    at: now,
                });
                match hit {
                    Hit::Scope(index) => self.select_scope(index),
                    Hit::Run(index) => self.select_run(index),
                    Hit::Artifact(path) => {
                        self.tree.select(path);
                        if double {
                            self.last_click = None;
                            Action::OpenDetail
                        } else {
                            Action::None
                        }
                    }
                }
            }
            _ => Action::None,
        }
    }
}
