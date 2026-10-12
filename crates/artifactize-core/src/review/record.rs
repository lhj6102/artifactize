//! The read-only Detail of a request the search found that is not a Human review: the
//! monitor's Detail sections of the eval in its Run, its saved runtime evidence, or its Agent
//! session read by the monitor's session reader. Nothing here claims, runs or submits.
use crate::{
    agent::session::{document::Move, live},
    config::ProfileKind,
    monitor::{self, Detail, Evidence, EvidenceStamp, Target, session::Live},
    store::{self, RequestView},
};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Stylize,
    text::Line,
    widgets::{Block, Paragraph, Wrap},
};
use std::path::Path;
use time::OffsetDateTime;

/// The share of a record's height its sections take when evidence is shown beside them: half,
/// so a long evidence text and a long list of sections each stay readable.
const SECTIONS_SHARE: u16 = 50;

/// Rows a page key scrolls the sections, as in the Human Detail.
const PAGE: u16 = 10;

pub(crate) struct Record {
    pub view: RequestView,
    detail: Detail,
    evidence: Evidence,
    stamp: EvidenceStamp,
    /// The Agent session, read by the monitor's incremental reader.
    pub live: Option<Live>,
    technical: bool,
    pending: bool,
    /// An Agent's sections instead of its transcript (`d`).
    details: bool,
    scroll: u16,
}

impl Record {
    /// Read the Detail of `view` in its Run. A `previous` Detail of the same request keeps its
    /// toggles, scroll and session reader while the request's session facts hold.
    pub async fn load(
        state: &Path,
        view: RequestView,
        previous: Option<Self>,
        serial: u64,
    ) -> Result<Self, String> {
        let (run, requests) = store::read_run_requests(state, &view.request.run_id).await?;
        let target = Target::Eval(view.request.eval_id.clone());
        let detail = monitor::detail(&run, &requests, &target, OffsetDateTime::now_utc());
        let stamp = EvidenceStamp::new(&view);
        let previous = previous.filter(|previous| previous.view.request.id == view.request.id);
        let (technical, pending, details, scroll) =
            previous
                .as_ref()
                .map_or((false, false, false, 0), |previous| {
                    (
                        previous.technical,
                        previous.pending,
                        previous.details,
                        previous.scroll,
                    )
                });
        let (evidence, live) = match previous {
            Some(previous) if previous.stamp == stamp => (previous.evidence, previous.live),
            previous => {
                evidence(
                    state,
                    &view,
                    previous.and_then(|previous| previous.live),
                    serial,
                )
                .await
            }
        };
        Ok(Self {
            view,
            detail,
            evidence,
            stamp,
            live,
            technical,
            pending,
            details,
            scroll,
        })
    }

    /// Keys of the read-only Detail; `false` leaves the key to the caller (Esc, q).
    pub fn key(&mut self, key: KeyEvent) -> bool {
        if let Some(live) = self.live.as_mut().filter(|_| !self.details) {
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
                return true;
            }
            match key.code {
                KeyCode::Enter | KeyCode::Char(' ') => {
                    live.toggle_visible();
                    return true;
                }
                KeyCode::Char('d') => {
                    self.details = true;
                    return true;
                }
                _ => return false,
            }
        }
        match key.code {
            KeyCode::Char('d') if self.live.is_some() => self.details = false,
            KeyCode::Char('t') => self.technical = !self.technical,
            KeyCode::Char('w') => self.pending = !self.pending,
            KeyCode::Down | KeyCode::Char('j') => self.scroll = self.scroll.saturating_add(1),
            KeyCode::Up | KeyCode::Char('k') => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(PAGE),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(PAGE),
            KeyCode::Home => self.scroll = 0,
            _ => return false,
        }
        true
    }

    /// The keys this Detail answers to, for the footer.
    pub fn hints(&self) -> &'static str {
        match (&self.live, self.details) {
            (Some(_), false) => {
                "↑↓ PgUp/PgDn scroll · Home/End · Space tools · d details · Esc back"
            }
            (Some(_), true) => "↑↓ scroll · t technical · w pending · d transcript · Esc back",
            (None, _) => "↑↓ PgUp/PgDn scroll · t technical · w pending · Esc back",
        }
    }

    pub fn draw(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        let detail = &self.detail;
        let title = if detail.summary.is_empty() {
            format!(" {} · read-only ", monitor::plain(&detail.title))
        } else {
            format!(
                " {} · {} · read-only ",
                monitor::plain(&detail.title),
                monitor::plain(&detail.summary)
            )
        };
        let outer = Block::bordered().title(title);
        let outer = if focused { outer.cyan() } else { outer };
        let inner = outer.inner(area);
        frame.render_widget(outer, area);
        let sections = || {
            Paragraph::new(monitor::section_lines(
                detail,
                self.technical,
                self.pending,
                false,
            ))
            .wrap(Wrap { trim: false })
            .scroll((self.scroll, 0))
        };
        match &mut self.live {
            Some(live) if !self.details => {
                let [rows, footer] =
                    Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(inner);
                live.geometry(usize::from(rows.width), usize::from(rows.height));
                frame.render_widget(Paragraph::new(monitor::transcript_lines(live)), rows);
                frame.render_widget(
                    Line::from(format!("{} · Space tools · d details", live.indicator()))
                        .dark_gray(),
                    footer,
                );
            }
            _ if self.evidence.is_empty() => frame.render_widget(sections(), inner),
            _ => {
                let [left, right] =
                    Layout::vertical([Constraint::Percentage(SECTIONS_SHARE), Constraint::Fill(1)])
                        .areas(inner);
                frame.render_widget(sections(), left);
                frame.render_widget(
                    Paragraph::new(self.evidence.text.as_str())
                        .wrap(Wrap { trim: false })
                        .block(Block::bordered().title(format!(" {} ", self.evidence.title))),
                    right,
                );
            }
        }
    }
}

/// What a request's Detail shows beside its sections: saved runtime logs, or its Agent
/// session (keeping `kept` when it reads the same session), or why there is none.
async fn evidence(
    state: &Path,
    view: &RequestView,
    kept: Option<Live>,
    serial: u64,
) -> (Evidence, Option<Live>) {
    if view.request.profile.kind() != ProfileKind::Agent {
        return (monitor::evidence(state, view), None);
    }
    let title = "Agent session".to_owned();
    match live::resolve(state, view).await {
        Ok(live::Resolution::Local(source)) => {
            let live = match kept {
                Some(mut live) => {
                    if live.refresh(&source) {
                        live
                    } else {
                        Live::new(serial, source)
                    }
                }
                None => Live::new(serial, source),
            };
            (
                Evidence {
                    title,
                    text: String::new(),
                },
                Some(live),
            )
        }
        Ok(live::Resolution::Unavailable(text)) => (Evidence { title, text }, None),
        Err(error) => (
            Evidence {
                title,
                text: format!("Conversation unavailable: {error}"),
            },
            None,
        ),
    }
}
