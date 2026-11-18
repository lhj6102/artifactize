//! The read-only Detail of a request the search found that is not a Human review: the
//! monitor's Detail sections of the eval in its Run, its saved runtime evidence, or its Agent
//! session read by the monitor's session reader. Nothing here claims, runs or submits.
use crate::{
    agent::session::{document::Move, live},
    config::ProfileKind,
    monitor::{self, Detail, Evidence, EvidenceStamp, SessionView, Target, session::Live},
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

const SESSION_TITLE: &str = "Agent session";

pub(crate) struct Record {
    pub view: RequestView,
    detail: Detail,
    content: Content,
    stamp: EvidenceStamp,
    technical: bool,
    pending: bool,
    scroll: u16,
}

enum Content {
    Evidence(Evidence),
    Session { live: Box<Live>, view: SessionView },
}

impl Record {
    /// Read the Detail of `view` in its Run. A `previous` Detail of the same request keeps its
    /// toggles, scroll and session reader while the request's session facts hold.
    pub async fn load(
        state: &Path,
        view: RequestView,
        previous: Option<Self>,
        serial: crate::monitor::session::ReaderId,
    ) -> Result<Self, String> {
        let (run, requests) = store::read_run_requests(state, &view.request.run_id).await?;
        let target = Target::Eval(view.request.eval_id.clone());
        let detail = monitor::detail(&run, &requests, &target, OffsetDateTime::now_utc());
        let stamp = EvidenceStamp::new(&view);
        let previous = previous.filter(|previous| previous.view.request.id == view.request.id);
        let (technical, pending, scroll) =
            previous.as_ref().map_or((false, false, 0), |previous| {
                (previous.technical, previous.pending, previous.scroll)
            });
        let content = match previous {
            Some(previous) if previous.stamp == stamp => previous.content,
            previous => {
                Content::load(
                    state,
                    &view,
                    previous.map(|previous| previous.content),
                    serial,
                )
                .await
            }
        };
        Ok(Self {
            view,
            detail,
            content,
            stamp,
            technical,
            pending,
            scroll,
        })
    }

    /// The Agent session, read by the monitor's incremental reader.
    pub(super) fn live_mut(&mut self) -> Option<&mut Live> {
        match &mut self.content {
            Content::Session { live, .. } => Some(live),
            Content::Evidence(_) => None,
        }
    }

    /// Keys of the read-only Detail; `false` leaves the key to the caller (Esc, q).
    pub fn key(&mut self, key: KeyEvent) -> bool {
        if let Content::Session { live, view } = &mut self.content {
            if key.code == KeyCode::Char('d') {
                *view = match *view {
                    SessionView::Transcript => SessionView::Sections,
                    SessionView::Sections => SessionView::Transcript,
                };
                return true;
            }
            if *view == SessionView::Transcript {
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
                    _ => return false,
                }
            }
        }
        match key.code {
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
        match &self.content {
            Content::Session {
                view: SessionView::Transcript,
                ..
            } => "↑↓ PgUp/PgDn scroll · Home/End · Space tools · d details · Esc back",
            Content::Session {
                view: SessionView::Sections,
                ..
            } => "↑↓ scroll · t technical · w pending · d transcript · Esc back",
            Content::Evidence(_) => "↑↓ PgUp/PgDn scroll · t technical · w pending · Esc back",
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
        let (title, text) = match &mut self.content {
            Content::Session {
                live,
                view: SessionView::Transcript,
            } => {
                let [rows, footer] =
                    Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(inner);
                live.geometry(usize::from(rows.width), usize::from(rows.height));
                frame.render_widget(Paragraph::new(monitor::transcript_lines(live)), rows);
                frame.render_widget(
                    Line::from(format!("{} · Space tools · d details", live.indicator()))
                        .dark_gray(),
                    footer,
                );
                return;
            }
            Content::Evidence(evidence) if evidence.is_empty() => {
                frame.render_widget(sections(), inner);
                return;
            }
            Content::Evidence(evidence) => (evidence.title.as_str(), evidence.text.as_str()),
            Content::Session {
                view: SessionView::Sections,
                ..
            } => (SESSION_TITLE, ""),
        };
        let [top, bottom] =
            Layout::vertical([Constraint::Percentage(SECTIONS_SHARE), Constraint::Fill(1)])
                .areas(inner);
        frame.render_widget(sections(), top);
        frame.render_widget(
            Paragraph::new(text)
                .wrap(Wrap { trim: false })
                .block(Block::bordered().title(format!(" {title} "))),
            bottom,
        );
    }
}

impl Content {
    /// Restore the current reader and mode only when there is still a live session.
    async fn load(
        state: &Path,
        request: &RequestView,
        previous: Option<Self>,
        serial: crate::monitor::session::ReaderId,
    ) -> Self {
        if request.request.profile.kind() != ProfileKind::Agent {
            return Self::Evidence(monitor::evidence(state, request));
        }
        let text = match live::resolve(state, request).await {
            Ok(live::Resolution::Local(source)) => {
                return match previous {
                    Some(Self::Session { mut live, view }) => {
                        if !live.refresh(&source) {
                            *live = Live::new(serial, source);
                        }
                        Self::Session { live, view }
                    }
                    _ => Self::Session {
                        live: Box::new(Live::new(serial, source)),
                        view: SessionView::Transcript,
                    },
                };
            }
            Ok(live::Resolution::Unavailable(text)) => text,
            Err(error) => format!("Conversation unavailable: {error}"),
        };
        Self::Evidence(Evidence {
            title: SESSION_TITLE.into(),
            text,
        })
    }
}
