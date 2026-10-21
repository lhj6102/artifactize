//! Terminal ownership and the asynchronous event/job driver.
use super::{Action, Monitor, REFRESH, SESSION_PROBE, SPIN, input, review};
use crossterm::event::{Event, EventStream, KeyEventKind};
use futures_util::{FutureExt, StreamExt};
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

/// Restore every input protocol when leaving, including errors, signals and panic unwinding.
pub(crate) struct InputGuard;
impl Drop for InputGuard {
    fn drop(&mut self) {
        let _ = input::protocols(false, false);
    }
}

/// What the next snapshot read must cover. Wanting more never lowers what is already wanted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Wanted {
    /// No read is due.
    Nothing,
    /// A read that may reuse the snapshot on screen: navigation only moved within it.
    Changes,
    /// A read of everything.
    Everything,
    /// A read of everything after hints may have been missed; Git is asked again too.
    Resynced,
}

impl Wanted {
    fn at_least(&mut self, wanted: Self) {
        *self = (*self).max(wanted);
    }
}

pub(crate) fn repaint(terminal: &mut ratatui::DefaultTerminal) -> Result<(), String> {
    crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::Clear(crossterm::terminal::ClearType::All)
    )
    .map_err(|error| error.to_string())?;
    terminal.swap_buffers();
    Ok(())
}
pub async fn run(
    state: PathBuf,
    repo: Option<PathBuf>,
    cancellation: CancellationToken,
) -> Result<(), String> {
    let mut terminal = ratatui::try_init().map_err(|error| {
        let _ = crossterm::terminal::disable_raw_mode();
        format!("cannot start the monitor: {error}")
    })?;
    let guard = InputGuard;
    let result = async {
        input::protocols(true, true)?;
        watch(&mut terminal, Monitor::new(state, repo), cancellation).await
    }
    .await;
    drop(guard);
    drop(terminal);
    ratatui::try_restore().map_err(|error| error.to_string())?;
    result
}
async fn watch(
    terminal: &mut ratatui::DefaultTerminal,
    mut monitor: Monitor,
    cancellation: CancellationToken,
) -> Result<(), String> {
    let mut changes = crate::changes::Subscription::new(&monitor.state).await;
    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(REFRESH);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut spin = tokio::time::interval(SPIN);
    let mut jobs = crate::jobs::Jobs::default();
    let mut action = Action::Refresh;
    let mut wanted = Wanted::Nothing;
    let mut session_probe = tokio::time::interval(SESSION_PROBE);
    session_probe.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let result = std::panic::AssertUnwindSafe(async {
    loop {
        match action {
            Action::Quit => return Ok(()),
            Action::Resync => {
                wanted.at_least(Wanted::Resynced);
                tick.reset();
            }
            Action::Refresh | Action::Review(review::Action::Refresh) => {
                wanted.at_least(if std::mem::take(&mut monitor.navigation) {
                    Wanted::Changes
                } else {
                    Wanted::Everything
                });
                tick.reset();
            }
            Action::OpenDetail => monitor.open_detail().await,
            Action::Capture(capture) => input::protocols(capture, true)?,
            Action::Review(review::Action::Start(job)) => {
                if let Some(review) = monitor.review_mut() {
                    jobs.action = Some(Box::pin(review.start(job)));
                }
            }
            _ => {}
        }
        if wanted > Wanted::Nothing && jobs.snapshot.is_none() {
            jobs.snapshot =
                monitor.start_refresh(wanted >= Wanted::Everything, wanted == Wanted::Resynced);
            wanted = Wanted::Nothing;
        }
        monitor.sync_peek().await;
        terminal
            .draw(|frame| monitor.draw(frame))
            .map_err(|error| error.to_string())?;
        // A Detail or peek switch drops the obsolete job's result. There is at most one bounded
        // job; render only marks geometry dirty and never performs I/O or spawns tasks.
        if jobs.session.is_none()
            && let Some(job) = monitor.live_mut().and_then(super::session::Live::job)
        {
            jobs.session = Some(tokio::task::spawn_blocking(move || {
                let mut job = job;
                let window =
                    job.reader
                        .step_expanded(job.width, job.height, job.position, &job.expanded);
                (job, window)
            }));
        }
        action = tokio::select! {
            biased;
            _ = cancellation.cancelled() => Action::Quit,
            event = events.next() => match event {
                Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => monitor.key(key),
                Some(Ok(Event::Mouse(mouse))) => monitor.mouse(mouse),
                Some(Ok(Event::Paste(text))) => {
                    monitor.paste(&text);
                    Action::None
                }
                Some(Ok(Event::Resize(_, _))) => {
                    monitor.hits = input::Hits::default();
                    Action::None
                }
                Some(Ok(_)) => Action::None,
                Some(Err(error)) => return Err(error.to_string()),
                None => Action::Quit,
            },
            outcome = async { jobs.snapshot.as_mut().expect("active snapshot job").await },
                if jobs.snapshot.is_some() =>
            {
                jobs.snapshot = None;
                let completed = crate::task::joined(outcome).map_err(|error| error.to_string())?;
                if !monitor.finish_refresh(completed).await {
                    wanted.at_least(Wanted::Everything);
                }
                Action::None
            }
            outcome = async { jobs.session.as_mut().expect("active session job").await },
                if jobs.session.is_some() =>
            {
                jobs.session = None;
                match crate::task::joined(outcome) {
                    Ok((job, window)) => {
                        if let Some(live) = monitor.live_mut()
                            && live.serial == job.serial
                            && live.source.reference == job.reader.source.reference
                        {
                            live.finish(job, window);
                        }
                    }
                    Err(error) => return Err(format!("Session reader job failed: {error}")),
                }
                Action::None
            }
            _ = session_probe.tick() => {
                if let Some(live) = monitor.live_mut() {
                    live.invalidate();
                }
                Action::None
            },
            outcome = async {
                match &mut jobs.action {
                    Some(job) => Some(job.await),
                    None => None,
                }
            }, if jobs.action.is_some() => {
                jobs.action = None;
                // Publishing may have emitted a fail-open warning on stderr.
                repaint(terminal)?;
                match (outcome, monitor.review_mut()) {
                    (Some(outcome), Some(review)) => Action::Review(review.finish_single(outcome)),
                    _ => Action::Refresh,
                }
            }
            event = async {
                match monitor.review_mut() {
                    Some(review) => review.log_event().await,
                    None => std::future::pending().await,
                }
            } => {
                if let Some(review) = monitor.review_mut() {
                    review.logged(event);
                }
                Action::None
            }
            _ = spin.tick(), if jobs.action.is_some() => Action::None,
            _ = tick.tick() => Action::None,
            change = changes.next() => match change {
                crate::changes::Change::StateInvalidated => {
                    wanted.at_least(Wanted::Everything);
                    Action::None
                }
                // The list is read again, and the shown Run only when it is this one.
                crate::changes::Change::RunInvalidated(_) => {
                    monitor.changed.add(&change);
                    wanted.at_least(Wanted::Changes);
                    Action::None
                }
                crate::changes::Change::Resync => {
                    wanted.at_least(Wanted::Resynced);
                    Action::None
                }
                crate::changes::Change::SessionInvalidated(id) => {
                    if let Some(live) = monitor.live_mut()
                        && live.source.reference.session_id == id
                    {
                        live.invalidate();
                    }
                    Action::None
                }
            },
        };
    }
    })
    .catch_unwind()
    .await;
    if let Some(review) = monitor.review_mut() {
        review.cancel_single();
    }
    for review in monitor.reviews.values() {
        review.cancel_single();
    }
    jobs.finish(result, changes).await
}
