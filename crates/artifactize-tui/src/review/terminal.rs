//! The standalone review's terminal loop: draw, read events, run jobs, show the Tool log.
use super::{Action, Outcome, REFRESH, Review, SPIN};
use crate::{
    platform::terminal::{self, Event},
    types::RequestId,
};
use futures_util::{FutureExt, StreamExt};
use std::{path::PathBuf, time::Instant};
use time::OffsetDateTime;
use tokio_util::sync::CancellationToken;

impl Review {
    pub fn draw(&mut self, frame: &mut ratatui::Frame) {
        let now = OffsetDateTime::now_utc();
        let elapsed = self.busy_elapsed_at(Instant::now());
        self.draw_at(frame, now, elapsed);
    }
}

/// Run the review UI until q, a signal, or a submission that leaves nothing waiting.
pub async fn run(
    state: PathBuf,
    repo: Option<PathBuf>,
    reviewer: crate::types::ReviewerId,
    open: Option<RequestId>,
    cancellation: CancellationToken,
) -> Result<(), String> {
    let mut display =
        terminal::init().map_err(|error| format!("cannot start the review: {error}"))?;
    let input = terminal::Input;
    let result = async {
        let review = Review::new(state, repo, reviewer, open);
        input.capture(review.mouse_capture)?;
        drive(&mut display, &input, review, cancellation).await
    }
    .await;
    drop(input);
    drop(display);
    terminal::restore()?;
    result
}

async fn drive(
    terminal: &mut ratatui::DefaultTerminal,
    input: &terminal::Input,
    mut review: Review,
    cancellation: CancellationToken,
) -> Result<(), String> {
    let mut changes = crate::changes::Subscription::new(&review.state).await;
    let mut events = terminal::events();
    let mut tick = tokio::time::interval(REFRESH);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut spin = tokio::time::interval(SPIN);
    let mut jobs = crate::jobs::Jobs::default();
    let mut action = Action::Refresh;
    let mut session_probe = tokio::time::interval(crate::monitor::SESSION_PROBE);
    session_probe.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let result = std::panic::AssertUnwindSafe(async {
        loop {
            loop {
                action = match action {
                    Action::None => break,
                    Action::Quit => return Ok(()),
                    Action::Refresh => {
                        tick.reset();
                        review.refresh().await
                    }
                    Action::Start(job) => {
                        jobs.action = Some(Box::pin(review.start(job)));
                        Action::None
                    }
                    Action::Capture(capture) => {
                        input.capture(capture)?;
                        Action::None
                    }
                };
            }
            review.drain_log();
            terminal
                .draw(|frame| review.draw(frame))
                .map_err(|e| e.to_string())?;
            if jobs.session.is_none()
                && let Some(job) = review
                    .record_live()
                    .and_then(crate::monitor::session::Live::job)
            {
                jobs.session = Some(tokio::task::spawn_blocking(move || {
                    let mut job = job;
                    let window = job.reader.step_expanded(
                        job.width,
                        job.height,
                        job.position,
                        &job.expanded,
                    );
                    (job, window)
                }));
            }
            action = tokio::select! {
                _ = cancellation.cancelled() => Action::Quit,
                outcome = async { jobs.session.as_mut().expect("active session job").await },
                    if jobs.session.is_some() =>
                {
                    jobs.session = None;
                    let (job, window) = crate::task::joined(outcome)
                        .map_err(|error| format!("Session reader job failed: {error}"))?;
                    if let Some(live) = review.record_live()
                        && live.serial == job.serial
                        && live.source.reference == job.reader.source.reference
                    {
                        live.finish(job, window);
                    }
                    Action::None
                }
                _ = session_probe.tick() => {
                    if let Some(live) = review.record_live() {
                        live.invalidate();
                    }
                    Action::None
                }
                outcome = async { jobs.action.as_mut().expect("pending job").await },
                    if jobs.action.is_some() =>
                {
                    jobs.action = None;
                    if matches!(outcome, Outcome::Submitted { .. }) {
                        // Remote and cache warnings may have reached stderr; repaint every cell.
                        terminal::repaint(terminal)?;
                    }
                    review.finish(outcome)
                }
                event = review.log_event() => {
                    review.logged(event);
                    Action::None
                }
                _ = spin.tick(), if review.busy() => Action::None,
                _ = tick.tick() => Action::None,
                change = changes.next() => match change {
                    crate::changes::Change::StateInvalidated
                    | crate::changes::Change::RunInvalidated(_)
                    | crate::changes::Change::Resync => {
                        Action::Refresh
                    }
                    crate::changes::Change::SessionInvalidated(id) => {
                        if let Some(live) = review.record_live()
                            && live.source.reference.session_id == id
                        {
                            live.invalidate();
                        }
                        Action::None
                    }
                },
                event = events.next() => match event {
                    Some(Ok(Event::Key(key))) => review.key(key),
                    Some(Ok(Event::Mouse(mouse))) => review.mouse(mouse),
                    Some(Ok(Event::Paste(text))) => review.paste(&text),
                    Some(Ok(Event::Resize)) => Action::None,
                    Some(Err(error)) => return Err(error.to_string()),
                    None => Action::Quit,
                },
            };
        }
    })
    .catch_unwind()
    .await;
    review.cancel_single();
    jobs.finish(result, changes).await
}
