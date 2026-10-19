//! The standalone review's terminal loop: draw, read events, run jobs, show the Tool log.
use super::{Action, Outcome, REFRESH, Review, SPIN};
use crate::types::RequestId;
use crossterm::event::{Event, EventStream, KeyEventKind};
use futures_util::{FutureExt, StreamExt};
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

/// Run the review UI until q, a signal, or a submission that leaves nothing waiting.
pub async fn run(
    state: PathBuf,
    repo: Option<PathBuf>,
    reviewer: crate::types::ReviewerId,
    open: Option<RequestId>,
    cancellation: CancellationToken,
) -> Result<(), String> {
    // ratatui's panic hook restores the terminal before a panic message is printed.
    let mut terminal = ratatui::try_init().map_err(|e| {
        let _ = crossterm::terminal::disable_raw_mode();
        format!("cannot start the review: {e}")
    })?;
    // Bracketed paste, like monitor: a pasted block reaches a field whole, never as keys.
    let guard = crate::monitor::InputGuard;
    let result = async {
        let review = Review::new(state, repo, reviewer, open);
        crate::monitor::input::protocols(review.mouse_capture, true)?;
        drive(&mut terminal, review, cancellation).await
    }
    .await;
    drop(guard);
    drop(terminal);
    ratatui::try_restore().map_err(|e| e.to_string())?;
    result
}

async fn drive(
    terminal: &mut ratatui::DefaultTerminal,
    mut review: Review,
    cancellation: CancellationToken,
) -> Result<(), String> {
    let mut changes = crate::changes::Subscription::new(&review.state).await;
    let mut events = EventStream::new();
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
                        crate::monitor::input::protocols(capture, true)?;
                        Action::None
                    }
                };
            }
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
                        crate::monitor::repaint(terminal)?;
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
                    crate::changes::Change::StateInvalidated | crate::changes::Change::Resync => {
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
                    Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => review.key(key),
                    Some(Ok(Event::Mouse(mouse))) => review.mouse(mouse),
                    Some(Ok(Event::Paste(text))) => review.paste(&text),
                    Some(Ok(_)) => Action::None,
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
