//! Terminal ownership and the asynchronous event/job driver.
use super::{Action, Monitor, REFRESH, SPIN, input, review};
use crossterm::event::{Event, EventStream, KeyEventKind};
use futures_util::StreamExt;
use std::{future::Future, path::PathBuf, pin::Pin};
use tokio_util::sync::CancellationToken;

/// Restore every input protocol when leaving, including errors, signals and panic unwinding.
struct InputGuard;
impl Drop for InputGuard {
    fn drop(&mut self) {
        let _ = input::protocols(false, false);
    }
}

pub(crate) async fn suspend<T>(
    terminal: &mut ratatui::DefaultTerminal,
    child: impl Future<Output = T>,
) -> Result<T, String> {
    input::protocols(false, false)?;
    terminal.show_cursor().map_err(|error| error.to_string())?;
    ratatui::try_restore().map_err(|error| error.to_string())?;
    let result = child.await;
    crossterm::terminal::enable_raw_mode().map_err(|error| error.to_string())?;
    crossterm::execute!(std::io::stdout(), crossterm::terminal::EnterAlternateScreen)
        .map_err(|error| error.to_string())?;
    repaint(terminal)?;
    Ok(result)
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
    let mut pending: Option<Pin<Box<dyn Future<Output = review::Outcome>>>> = None;
    let mut action = Action::Refresh;
    let mut session_job: Option<
        tokio::task::JoinHandle<(super::session::Job, crate::agent::session::live::Window)>,
    > = None;
    let mut session_probe = tokio::time::interval(std::time::Duration::from_secs(5));
    session_probe.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        match action {
            Action::Quit => {
                if let Some(review) = monitor
                    .detail
                    .as_ref()
                    .and_then(|pane| pane.review.as_ref())
                {
                    review.cancel_single();
                }
                if let Some(job) = pending.take() {
                    let _ = job.await;
                }
                return Ok(());
            }
            Action::Refresh | Action::Review(review::Action::Refresh) => {
                monitor.refresh().await;
                tick.reset();
            }
            Action::OpenDetail => monitor.open_detail().await,
            Action::Capture(capture) => input::protocols(capture, true)?,
            Action::Review(review::Action::Start(job)) => {
                if let Some(review) = monitor.review_mut() {
                    pending = Some(Box::pin(review.start(job)));
                }
            }
            Action::Review(review::Action::Edit(_)) => {
                return Err("Monitor never invokes an external editor.".into());
            }
            _ => {}
        }
        monitor.sync_peek().await;
        terminal
            .draw(|frame| monitor.draw(frame))
            .map_err(|error| error.to_string())?;
        // A Detail or peek switch drops the obsolete job's result. There is at most one bounded
        // job; render only marks geometry dirty and never performs I/O or spawns tasks.
        if session_job.is_none()
            && let Some(job) = monitor.live_mut().and_then(super::session::Live::job)
        {
            session_job = Some(tokio::task::spawn_blocking(move || {
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
                Some(Ok(Event::Paste(text))) => { monitor.paste(&text); Action::None }
                Some(Ok(Event::Resize(_, _))) => { monitor.hits = input::Hits::default(); Action::None }
                Some(Ok(_)) => Action::None,
                Some(Err(error)) => return Err(error.to_string()),
                None => Action::Quit,
            },
            outcome = async { session_job.as_mut().expect("active session job").await }, if session_job.is_some() => {
                session_job = None;
                match outcome {
                    Ok((job, window)) => {
                        if let Some(live) = monitor.live_mut()
                            && live.serial == job.serial && live.source.reference == job.reader.source.reference
                        { live.finish(job, window); }
                    }
                    Err(error) => return Err(format!("Session reader job failed: {error}")),
                }
                Action::None
            }
            _ = session_probe.tick() => {
                if let Some(live) = monitor.live_mut() { live.invalidate(); }
                Action::None
            },
            outcome = async { match &mut pending { Some(job) => Some(job.await), None => None } }, if pending.is_some() => {
                pending = None;
                // Publishing may have emitted a fail-open warning on stderr.
                repaint(terminal)?;
                match (outcome, monitor.review_mut()) {
                    (Some(outcome), Some(review)) => Action::Review(review.finish_single(outcome)),
                    _ => Action::Refresh,
                }
            }
            _ = spin.tick(), if pending.is_some() => Action::None,
            _ = tick.tick() => Action::None,
            change = changes.next() => match change {
                crate::changes::Change::StateInvalidated | crate::changes::Change::Resync => Action::Refresh,
                crate::changes::Change::SessionInvalidated(id) => {
                    if let Some(live) = monitor.live_mut() && live.source.reference.session_id == id { live.invalidate(); }
                    Action::None
                }
            },
        };
    }
}
