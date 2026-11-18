//! Opened evidence and live Agent sessions, plus Human drafts kept between Detail visits.
use super::{Detail, DetailArea, Focus, Monitor, Pane, Target, evidence, layout, model, session};
use crate::{
    config::ProfileKind,
    review::Review,
    store::RequestView,
    types::{RequestId, RequestStatus, RunId},
};
use time::OffsetDateTime;

/// What an opened Detail holds besides its sections: a Human request is a review, an Agent
/// request with a saved conversation is a session, anything else only its evidence.
pub(super) enum Content {
    Evidence,
    Review(Box<Review>),
    Session {
        live: Box<session::Live>,
        view: SessionView,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SessionView {
    Transcript,
    Sections,
}

impl Content {
    pub(super) fn session(live: session::Live) -> Self {
        Self::Session {
            live: Box::new(live),
            view: SessionView::Transcript,
        }
    }

    pub(super) fn session_view(&self) -> Option<SessionView> {
        match self {
            Self::Session { view, .. } => Some(*view),
            _ => None,
        }
    }

    pub(super) fn review(&self) -> Option<&Review> {
        match self {
            Self::Review(review) => Some(review.as_ref()),
            _ => None,
        }
    }
    pub(super) fn review_mut(&mut self) -> Option<&mut Review> {
        match self {
            Self::Review(review) => Some(review.as_mut()),
            _ => None,
        }
    }
    pub(super) fn live(&self) -> Option<&session::Live> {
        match self {
            Self::Session { live, .. } => Some(live.as_ref()),
            _ => None,
        }
    }
    pub(super) fn live_mut(&mut self) -> Option<&mut session::Live> {
        match self {
            Self::Session { live, .. } => Some(live.as_mut()),
            _ => None,
        }
    }
    /// The conversation shown, or none: a session that cannot be read leaves only evidence.
    fn set_live(&mut self, live: Option<session::Live>) {
        match (self, live) {
            (Self::Session { live: current, .. }, Some(live)) => **current = live,
            (content, Some(live)) => *content = Self::session(live),
            (content, None) => *content = Self::Evidence,
        }
    }
    fn take_review(&mut self) -> Option<Review> {
        match std::mem::replace(self, Self::Evidence) {
            Self::Review(review) => Some(*review),
            other => {
                *self = other;
                None
            }
        }
    }
}

/// The opened Detail of one tree node: sections, evidence, an Agent session or a Human review.
pub struct DetailPane {
    pub(super) run_id: RunId,
    pub(super) target: Target,
    pub(super) request: Option<RequestId>,
    pub(super) evidence: evidence::Evidence,
    pub(super) evidence_stamp: Option<evidence::EvidenceStamp>,
    /// Snapshot remains attached to this Detail if the selected Run is pruned or shifts pages.
    pub(super) detail: Detail,
    /// What the Detail shows beyond its sections.
    pub(super) content: Content,
    pub(super) focus: DetailArea,
    pub(super) scroll: [u16; 2],
    /// The folded Technical section is shown.
    pub(super) technical: bool,
    /// The pending evals under `Waits for` are shown.
    pub(super) pending: bool,
}

impl DetailPane {
    pub(super) fn toggle_session_view(&mut self) {
        if let Content::Session { view, .. } = &mut self.content {
            (*view, self.focus) = match view {
                SessionView::Transcript => (SessionView::Sections, DetailArea::Summary),
                SessionView::Sections => (SessionView::Transcript, DetailArea::Evidence),
            };
        }
    }
}

/// The tree-focused peek of a running Agent: only its last transcript line is read.
pub(super) struct Peek {
    pub(super) request: RequestId,
    /// The request's session facts when it was resolved; a change resolves it again.
    pub(super) stamp: evidence::EvidenceStamp,
    pub(super) live: Option<session::Live>,
    /// Why there is no session to read; a refresh resolves it again.
    pub(super) text: String,
}

impl Monitor {
    pub(super) async fn refresh_detail(&mut self, changed: bool, now: OffsetDateTime) {
        // A peek without a session (unavailable or failed) resolves again after a refresh.
        self.peek = self.peek.take().filter(|peek| peek.live.is_some());
        if let Some(live) = self.peek.as_mut().and_then(|peek| peek.live.as_mut()) {
            live.invalidate();
        }
        if let Focus::Detail(pane) = &mut self.focus {
            if let Some((run, requests)) = &self.run
                && changed
                && run.run.id == pane.run_id
                && pane
                    .request
                    .as_ref()
                    .is_none_or(|id| requests.iter().any(|view| &view.request.id == id))
            {
                pane.detail = model::detail(run, requests, &pane.target, now);
                if pane.content.review().is_none()
                    && let Some(view) = pane
                        .request
                        .as_ref()
                        .and_then(|id| requests.iter().find(|view| &view.request.id == id))
                {
                    let stamp = evidence::EvidenceStamp::new(view);
                    if pane.evidence_stamp.as_ref() != Some(&stamp)
                        || pane.content.live().is_none()
                            && view.request.profile.kind() == ProfileKind::Agent
                    {
                        Self::load_evidence(
                            &self.state,
                            self.origin.identity(),
                            self.serial,
                            pane,
                            view,
                        )
                        .await;
                        pane.evidence_stamp = Some(stamp);
                    }
                }
            }
            if changed && let Some(review) = pane.content.review_mut() {
                let _ = review.refresh().await;
            }
        }
    }
    pub async fn open_detail(&mut self) {
        let Some(target) = self.target() else {
            return;
        };
        let Some((run, requests)) = &self.run else {
            return;
        };
        let view = match &target {
            Target::Eval(eval) => requests
                .iter()
                .find(|view| &view.request.eval_id == eval)
                .cloned(),
            _ => None,
        };
        let saved_detail = model::detail(run, requests, &target, OffsetDateTime::now_utc());
        self.serial = self.serial.next();
        let mut pane = DetailPane {
            run_id: run.run.id.clone(),
            target,
            detail: saved_detail,
            request: view.as_ref().map(|view| view.request.id.clone()),
            evidence: evidence::Evidence::default(),
            evidence_stamp: view.as_ref().map(evidence::EvidenceStamp::new),
            // Sections first; an Agent session moves focus to its transcript.
            focus: DetailArea::Summary,
            scroll: [0; 2],
            // The peek's session keeps its reader and position.
            content: self
                .peek
                .take()
                .filter(|peek| view.as_ref().map(|view| &view.request.id) == Some(&peek.request))
                .and_then(|peek| peek.live)
                .map_or(Content::Evidence, Content::session),
            technical: false,
            pending: false,
        };
        if let Some(view) = view {
            if view.request.profile.kind() == ProfileKind::Human {
                let resolved = if view.request.status().is_human_wait() {
                    evidence::original(&self.state, &view).await
                } else {
                    Ok(view)
                };
                match resolved {
                    Ok(view) => match crate::human::default_reviewer() {
                        Ok(reviewer) => {
                            let mut review =
                                self.reviews.remove(&view.request.id).unwrap_or_else(|| {
                                    Review::new(
                                        self.state.clone(),
                                        None,
                                        reviewer,
                                        Some(view.request.id.clone()),
                                    )
                                });
                            pane.request = Some(view.request.id.clone());
                            review.load_single(view);
                            review.resolve_commands().await;
                            pane.content = Content::Review(Box::new(review));
                        }
                        Err(error) => self.notice = Some(error),
                    },
                    Err(error) => self.notice = Some(error),
                }
            } else {
                Self::load_evidence(
                    &self.state,
                    self.origin.identity(),
                    self.serial,
                    &mut pane,
                    &view,
                )
                .await;
            }
        }
        self.focus = Focus::Detail(Box::new(pane));
    }
    async fn load_evidence(
        state: &std::path::Path,
        identity: Option<&crate::types::StateId>,
        serial: crate::monitor::session::ReaderId,
        pane: &mut DetailPane,
        view: &RequestView,
    ) {
        use crate::agent::session::live::{self, Resolution};
        if view.request.profile.kind() != ProfileKind::Agent {
            pane.evidence = evidence::evidence(state, view);
            return;
        }
        pane.evidence.title = "Agent session".into();
        match live::resolve_cached(state, view, identity).await {
            Ok(Resolution::Local(source)) => {
                let refreshed = pane
                    .content
                    .live_mut()
                    .is_some_and(|live| live.refresh(&source));
                if !refreshed {
                    pane.content
                        .set_live(Some(session::Live::new(serial, source)));
                }
                pane.evidence.text = "Loading session…".into();
            }
            Ok(Resolution::Unavailable(text)) => {
                pane.content.set_live(None);
                pane.evidence.text = text;
            }
            Err(error) => {
                pane.content.set_live(None);
                pane.evidence.text = format!("Conversation unavailable: {error}");
            }
        }
    }
    /// Close the Detail and step back to the tree; Human drafts survive for the next open.
    pub(super) fn leave_detail(&mut self) {
        if !matches!(self.focus, Focus::Detail(_)) {
            return;
        }
        if let Focus::Detail(mut pane) = std::mem::replace(&mut self.focus, Focus::Artifacts)
            && let (Some(id), Some(review)) = (pane.request, pane.content.take_review())
        {
            self.reviews.insert(id, review);
        }
    }
    /// Load the peek's Agent session when the tree selects a running Agent and the peek shows.
    pub async fn sync_peek(&mut self) {
        use crate::agent::session::live::{self, Resolution};
        let visible = matches!(self.focus, Focus::Artifacts)
            && layout::plan(self.size, Pane::Artifacts).density(Pane::Detail)
                == layout::Density::Preview;
        let view = visible
            .then(|| self.selected_request())
            .flatten()
            .filter(|view| {
                view.request.profile.kind() == ProfileKind::Agent
                    && view.request.status() == RequestStatus::Running
            })
            .cloned();
        let Some(view) = view else {
            self.peek = None;
            return;
        };
        let stamp = evidence::EvidenceStamp::new(&view);
        // A resolved session is read incrementally; only a new request or new session facts
        // resolve it again, so drawing never queries the state.
        if let Some(peek) = &self.peek
            && peek.request == view.request.id
            && peek.stamp == stamp
        {
            return;
        }
        let kept = self
            .peek
            .take()
            .filter(|peek| peek.request == view.request.id)
            .and_then(|peek| peek.live);
        let (live, text) =
            match live::resolve_cached(&self.state, &view, self.origin.identity()).await {
                Ok(Resolution::Local(source)) => match kept {
                    Some(mut live) if live.source.reference == source.reference => {
                        live.source = source;
                        if let Some(reader) = &mut live.reader {
                            reader.source = live.source.clone();
                        }
                        live.invalidate();
                        (Some(live), String::new())
                    }
                    _ => {
                        self.serial = self.serial.next();
                        (Some(session::Live::new(self.serial, source)), String::new())
                    }
                },
                Ok(Resolution::Unavailable(text)) => (None, text),
                Err(error) => (None, format!("Conversation unavailable: {error}")),
            };
        self.peek = Some(Peek {
            request: view.request.id.clone(),
            stamp,
            live,
            text,
        });
    }
    /// The Agent session the driver reads for: the open Detail's, or else the peek's.
    pub(super) fn live_mut(&mut self) -> Option<&mut session::Live> {
        match &mut self.focus {
            Focus::Detail(pane) => pane.content.live_mut(),
            _ => self.peek.as_mut().and_then(|peek| peek.live.as_mut()),
        }
    }
    pub(super) fn review_mut(&mut self) -> Option<&mut Review> {
        match &mut self.focus {
            Focus::Detail(pane) => pane.content.review_mut(),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(super) fn detail_mut(&mut self) -> Option<&mut DetailPane> {
        match &mut self.focus {
            Focus::Detail(pane) => Some(pane),
            _ => None,
        }
    }

    pub(super) fn detail(&self) -> Option<&DetailPane> {
        match &self.focus {
            Focus::Detail(pane) => Some(pane),
            _ => None,
        }
    }
    /// A Human review that is working or editing keeps focus in Detail.
    pub(super) fn locked(&self) -> bool {
        self.detail()
            .and_then(|pane| pane.content.review())
            .is_some_and(owns_keys)
    }
}

/// A Human review takes every key while it works, or while it edits a request
/// that still waits; a request settled elsewhere leaves its form behind without keys.
fn owns_keys(review: &Review) -> bool {
    review.busy() || !review.settled() && review.editing()
}
