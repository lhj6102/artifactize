//! Live updates: the state's invalidation hub, as the terminal monitor subscribes to it, and
//! a rescan of which worktrees exist, which no state change reports. Hints only say what to
//! read again: the next load reads what they named, and the frontend reads through the
//! commands.
use crate::App;
use artifactize_core::changes::{Change, Subscription};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{path::PathBuf, time::Duration};
use tauri::Manager;
use tauri_specta::Event;

/// How often Git is asked which worktrees exist: a worktree added or removed in another
/// process reaches the Pane within this.
const RESCAN: Duration = Duration::from_secs(2);

/// The state changed, or (`resync`) hints may have been missed: read it afresh.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct StateChanged {
    pub resync: bool,
}

/// An Agent session file grew; only its open transcript reads again.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct SessionChanged {
    pub session: crate::ids::SessionKey,
}

/// Forward the hub's hints to every window until the app exits. The subscription itself
/// reconciles missed commits and database replacement every few seconds. Every [`RESCAN`]
/// the live branches are listed again, and a change is a hint as well.
pub fn spawn<R: tauri::Runtime>(app: tauri::AppHandle<R>, state: PathBuf) {
    let hub = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut changes = Subscription::new(&state).await;
        loop {
            let change = changes.next().await;
            let state = hub.state::<App>();
            state.browse.invalidate(&change);
            if !matches!(change, Change::SessionInvalidated(_)) {
                crate::viewer::refresh_active(&state).await;
            }
            if matches!(
                change,
                Change::RunInvalidated(_) | Change::StateInvalidated | Change::Resync
            ) {
                state.human.review_status_changed();
            }
            let sent = match change {
                Change::StateInvalidated | Change::RunInvalidated(_) => {
                    StateChanged { resync: false }.emit(&hub)
                }
                Change::Resync => StateChanged { resync: true }.emit(&hub),
                Change::DraftInvalidated(id) => {
                    crate::human::draft::HumanDraftChanged { id: id.into() }.emit(&hub)
                }
                Change::SessionInvalidated(id) => SessionChanged { session: id.into() }.emit(&hub),
            };
            // The app is closing; nobody listens any more.
            if sent.is_err() {
                return;
            }
        }
    });
    tauri::async_runtime::spawn(async move {
        let mut ticks = tokio::time::interval(RESCAN);
        ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticks.tick().await;
            // A failed read is the next load's notice; the rescan tries again next tick.
            let app_state = app.state::<App>();
            let _ = app_state.chat.reconcile_interrupted().await;
            let reviews_changed =
                artifactize_core::store::retire_removed_signoffs(&app_state.launch.state).await;
            let branches_changed = crate::browse::rescan(&app_state).await;
            let review_worktrees_changed = app_state.human.review_worktrees_changed();
            if (reviews_changed.unwrap_or(false)
                || branches_changed.unwrap_or(false)
                || review_worktrees_changed)
                && (StateChanged { resync: false }).emit(&app).is_err()
            {
                return;
            }
        }
    });
}
