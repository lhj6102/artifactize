//! Claim, tool, submission and release jobs for the standalone review and embedded Detail.
use super::{
    Action, CLAIM_FIRST, Focus, Job, Mode, NOT_CLAIMED, Outcome, Review, Shown, Submission, log,
};
use crate::{config::ToolName, store::HumanClaim, types::RequestId};
use std::{
    future::Future,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

pub(super) struct Busy {
    pub(super) label: String,
    pub(super) since: Instant,
    pub(super) cancel: CancellationToken,
}

impl Drop for Busy {
    fn drop(&mut self) {
        // A terminal error or unwinding must cancel an active Human tool too.
        self.cancel.cancel();
    }
}

/// Resolved command lines by tool name; a tool that cannot be resolved carries its error.
impl Review {
    /// Run the selected tool at once; the run itself checks the claim and resolves the command.
    pub(super) fn run_selected(&mut self) -> Action {
        let Some(tool) = self.tools().into_iter().nth(self.tool) else {
            return self.notify("This request declares no Human tools.", true);
        };
        self.run(tool.name)
    }

    /// The open request when this reviewer holds its claim. Jobs never claim implicitly: a
    /// claim is always the explicit `c`.
    fn owned_request(&self) -> Result<RequestId, String> {
        match self.actionable()? {
            (id, false) => Ok(id),
            (_, true) => Err(CLAIM_FIRST.into()),
        }
    }

    pub(super) fn release(&mut self) -> Action {
        match (self.actionable(), self.mine()) {
            (Ok((id, _)), true) => Action::Start(Job::Release {
                ids: vec![id],
                quit: false,
            }),
            (Ok(_), false) => self.notify(NOT_CLAIMED, true),
            (Err(error), _) => self.notify(error, true),
        }
    }

    pub(super) fn run(&mut self, tool: ToolName) -> Action {
        match self.owned_request() {
            Ok(id) => Action::Start(Job::Run {
                id,
                tool,
                claim: false,
            }),
            Err(error) => self.notify(error, true),
        }
    }

    pub(super) fn submit(&mut self, result: Submission) -> Action {
        match self.owned_request() {
            Ok(id) => Action::Start(Job::Submit {
                id,
                result,
                claim: false,
            }),
            Err(error) => {
                if let Mode::Form(form) = &mut self.mode {
                    form.error = Some(error);
                }
                Action::None
            }
        }
    }

    /// Time since this job started, using the monotonic time sampled for this frame.
    pub(crate) fn busy_elapsed_at(&self, now: Instant) -> Duration {
        self.busy.as_ref().map_or(Duration::ZERO, |busy| {
            now.saturating_duration_since(busy.since)
        })
    }

    /// Mark the screen busy and return the job's future; feed its outcome to `finish`.
    pub fn start(&mut self, job: Job) -> impl Future<Output = Outcome> + 'static {
        let cancel = CancellationToken::new();
        let label = match &job {
            Job::Claim { .. } => "Claiming the review".into(),
            Job::Run { tool, .. } => format!("Running {tool}"),
            Job::Submit { result, .. } => format!("Submitting {}", result.verdict().as_str()),
            Job::Release { .. } => "Releasing the claim".into(),
        };
        self.busy = Some(Busy {
            label,
            since: Instant::now(),
            cancel: cancel.clone(),
        });
        let state = self.state.clone();
        let reviewer = self.reviewer.clone();
        let sink = self.sink.clone();
        async move { job.run(state, reviewer, cancel, sink).await }
    }

    /// The next Tool log event of this review's runs, for the screen's event loop.
    pub(crate) async fn log_event(&mut self) -> log::Event {
        self.events
            .recv()
            .await
            .expect("the review holds its own sink")
    }

    /// Take a Tool log event in, and any others already waiting. Events of a request this
    /// review no longer shows are dropped.
    pub(crate) fn logged(&mut self, event: log::Event) {
        if self.open.as_ref() == event.key.request() {
            self.log.apply(event);
        }
        self.drain_log();
    }

    /// Take in the Tool log events already waiting.
    pub(crate) fn drain_log(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            if self.open.as_ref() == event.key.request() {
                self.log.apply(event);
            }
        }
    }

    /// Remember a claim this session took on the open request.
    fn took(&mut self, claim: Option<HumanClaim>) {
        if let (Some(_), Some(id)) = (claim, &self.open)
            && !self.taken.contains(id)
        {
            self.taken.push(id.clone());
        }
    }

    /// Apply a job's outcome.
    pub fn finish(&mut self, outcome: Outcome) -> Action {
        // The job may finish before the driver receives its last log events.
        self.drain_log();
        self.busy = None;
        let notice = outcome.notice();
        match outcome {
            Outcome::SavedLocally { request, .. } => {
                self.taken.retain(|id| id != &request.id);
                if let Some(view) = self.shown.request_mut() {
                    view.request = std::sync::Arc::new(*request);
                    view.claim = None;
                }
                self.mode = Mode::Request;
                self.notice = Some(notice);
                Action::Refresh
            }
            Outcome::Claimed { result } => {
                if let Ok(claim) = result {
                    self.took(Some(claim.clone()));
                    if let Some(view) = self.shown.request_mut() {
                        view.claim = Some(claim);
                    }
                    if !matches!(self.mode, Mode::Form(_)) {
                        self.mode = Mode::Request;
                    }
                }
                self.notice = Some(notice);
                Action::Refresh
            }
            Outcome::Ran { claimed, .. } => {
                self.took(claimed);
                // Declarations may have changed; show the command the next run resolves.
                self.commands = None;
                self.notice = Some(notice);
                Action::Refresh
            }
            Outcome::Submitted {
                id,
                claimed,
                result,
            } => {
                self.took(claimed);
                match result {
                    Ok(_) => {
                        self.taken.retain(|taken| *taken != id);
                        self.notice = Some(notice);
                        self.open = None;
                        self.shown = Shown::Nothing;
                        self.log.clear();
                        self.mode = Mode::Request;
                        self.focus = Focus::List;
                        self.submitted = true;
                    }
                    Err(error) => {
                        if let Mode::Form(form) = &mut self.mode {
                            form.error = Some(error);
                        } else {
                            self.notice = Some(notice);
                        }
                    }
                }
                Action::Refresh
            }
            Outcome::Released { ids, quit, failed } => {
                self.taken.retain(|taken| !ids.contains(taken));
                if failed.is_empty() {
                    if quit {
                        return Action::Quit;
                    }
                } else {
                    self.taken.extend(failed.into_iter().map(|(id, _)| id));
                }
                self.notice = Some(notice);
                Action::Refresh
            }
        }
    }
}
