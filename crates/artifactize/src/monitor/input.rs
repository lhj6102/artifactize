//! Terminal protocol and hit tests over geometry from the most recently rendered frame.
use super::{Action, DetailArea, Monitor, Pane};
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
    Artifact(Vec<super::model::NodeId>),
}
pub(super) struct Click {
    hit: Hit,
    at: Instant,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Button {
    Review(Control),
    Close,
    SessionTop,
    SessionBottom,
    SessionDetails,
}
#[derive(Default)]
pub(super) struct Hits {
    /// Visible panes of the last frame, left to right.
    pub panes: Vec<(Pane, Rect)>,
    /// Tree rows inside the Run pane, below its headline.
    pub tree: Rect,
    /// The opened Detail pane; empty for a peek.
    pub detail: Rect,
    /// Sections and evidence, or a Human review's tools and fields.
    pub areas: [Rect; 2],
    /// The Human review component's own geometry.
    pub review: crate::review::Hits,
    pub buttons: Vec<(Rect, Button)>,
    pub session_groups: Vec<(Rect, crate::agent::session::transcript::BlockId)>,
}
impl Hits {
    pub fn pane(&self, pane: Pane) -> Option<Rect> {
        self.panes
            .iter()
            .find(|(visible, _)| *visible == pane)
            .map(|(_, area)| *area)
    }
}

pub(crate) fn protocols(mouse: bool, paste: bool) -> Result<(), String> {
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
        self.mouse_at(event, Instant::now())
    }

    /// `mouse`, for an event that happened at `at`: a second left click on the same row
    /// within [`DOUBLE_CLICK`] of the first opens it.
    pub(crate) fn mouse_at(&mut self, event: MouseEvent, at: Instant) -> Action {
        if !self.mouse_capture {
            return Action::None;
        }
        let point = Position::new(event.column, event.row);
        if self.help {
            if event.kind == MouseEventKind::Down(MouseButton::Left) {
                self.help = false;
            }
            return Action::None;
        }
        let pane = self
            .hits
            .panes
            .iter()
            .find(|(_, rect)| contains(*rect, point))
            .map(|(pane, _)| *pane);
        if self.focus == Pane::Detail && self.detail.is_some() {
            if contains(self.hits.detail, point) {
                return self.detail_mouse(event, point);
            }
            // A working or editing review keeps focus; the wheel still scrolls.
            if self.locked() && matches!(event.kind, MouseEventKind::Down(_)) {
                return Action::None;
            }
        }
        let Some(pane) = pane else {
            return Action::None;
        };
        match event.kind {
            // The wheel scrolls the pane under the pointer and never moves focus.
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                self.wheel(pane, event.kind == MouseEventKind::ScrollDown)
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let Some(area) = self.hits.pane(pane) else {
                    return Action::None;
                };
                if pane == Pane::Detail {
                    // Clicking the peek opens it.
                    return if self.focus == Pane::Artifacts {
                        Action::OpenDetail
                    } else {
                        Action::None
                    };
                }
                if self.focus == Pane::Detail {
                    self.leave_detail();
                }
                self.focus = pane;
                let area = if pane == Pane::Artifacts {
                    self.hits.tree
                } else {
                    area
                };
                let hit = match pane {
                    // Borders are not rows.
                    Pane::Repositories | Pane::Runs
                        if point.y <= area.y || point.y >= area.bottom().saturating_sub(1) =>
                    {
                        return Action::None;
                    }
                    Pane::Repositories => {
                        Hit::Scope(self.repositories.offset() + usize::from(point.y - area.y - 1))
                    }
                    Pane::Runs => Hit::Run(self.list.offset() + usize::from(point.y - area.y - 1)),
                    Pane::Artifacts => match self.tree.rendered_at(point) {
                        Some(path) => Hit::Artifact(path.to_vec()),
                        None => return Action::None,
                    },
                    Pane::Detail => return Action::None,
                };
                let double = self.last_click.as_ref().is_some_and(|click| {
                    click.hit == hit && at.saturating_duration_since(click.at) <= DOUBLE_CLICK
                });
                self.last_click = Some(Click {
                    hit: hit.clone(),
                    at,
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
    fn wheel(&mut self, pane: Pane, down: bool) -> Action {
        let steps = WHEEL_ROWS as usize;
        match pane {
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
            // The tree viewport scrolls; the selection, and so the peek, stays.
            Pane::Artifacts => {
                if down {
                    self.tree.scroll_down(steps);
                } else {
                    self.tree.scroll_up(steps);
                }
                Action::None
            }
            Pane::Detail => Action::None,
        }
    }
    /// Clicks and the wheel inside the opened Detail pane.
    fn detail_mouse(&mut self, event: MouseEvent, point: Position) -> Action {
        let Some(pane) = &mut self.detail else {
            return Action::None;
        };
        if event.kind == MouseEventKind::Down(MouseButton::Left) {
            if let Some((_, button)) = self
                .hits
                .buttons
                .iter()
                .find(|(rect, _)| contains(*rect, point))
            {
                return match button {
                    Button::Close => {
                        if let Some(review) = &mut pane.review
                            && review.busy()
                        {
                            return Action::Review(review.control(Control::Cancel));
                        }
                        self.leave_detail();
                        Action::None
                    }
                    Button::SessionDetails => {
                        pane.show_details = !pane.show_details;
                        pane.focus = if pane.show_details {
                            DetailArea::Summary
                        } else {
                            DetailArea::Evidence
                        };
                        Action::None
                    }
                    Button::SessionTop | Button::SessionBottom => {
                        if let Some(live) = &mut pane.live {
                            pane.focus = DetailArea::Evidence;
                            live.movement(if *button == Button::SessionTop {
                                crate::agent::session::document::Move::Top
                            } else {
                                crate::agent::session::document::Move::Bottom
                            });
                        }
                        Action::None
                    }
                    Button::Review(control) => {
                        pane.review.as_mut().map_or(Action::None, |review| {
                            Action::Review(review.control(*control))
                        })
                    }
                };
            }
            if let Some((_, id)) = self
                .hits
                .session_groups
                .iter()
                .find(|(rect, _)| contains(*rect, point))
                && let Some(live) = &mut pane.live
            {
                pane.focus = DetailArea::Evidence;
                live.toggle(*id);
                return Action::None;
            }
            // The shared component: focus an area, select a row; a click on the selected tool
            // of the focused Tools pane runs it.
            if let Some(review) = &mut pane.review {
                return Action::Review(review.click(&self.hits.review, point));
            } else if contains(self.hits.areas[0], point) {
                pane.focus = DetailArea::Summary;
            } else if contains(self.hits.areas[1], point) {
                pane.focus = DetailArea::Evidence;
            }
        }
        let delta = match event.kind {
            MouseEventKind::ScrollDown => WHEEL_ROWS,
            MouseEventKind::ScrollUp => -WHEEL_ROWS,
            _ => return Action::None,
        };
        if contains(self.hits.review.instruction, point)
            && let Some(review) = &mut pane.review
        {
            review.scroll_instruction(delta);
            return Action::None;
        }
        let index = if contains(self.hits.areas[0], point) {
            0
        } else if contains(self.hits.areas[1], point) {
            1
        } else {
            return Action::None;
        };
        if let Some(review) = &mut pane.review {
            review.scroll_single(delta, index == 1);
        } else if index == 1
            && let Some(live) = &mut pane.live
        {
            pane.focus = DetailArea::Evidence;
            live.movement(if delta > 0 {
                crate::agent::session::document::Move::Down(delta as usize)
            } else {
                crate::agent::session::document::Move::Up(delta.unsigned_abs() as usize)
            });
        } else {
            pane.scroll[index] = pane.scroll[index].saturating_add_signed(delta);
        }
        Action::None
    }
}
