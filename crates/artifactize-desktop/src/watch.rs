//! Live updates: the state's invalidation hub, as the terminal monitor subscribes to it.
//! Hints only say what to read again; the frontend reads through the commands.
use artifactize_core::changes::{Change, Subscription};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::PathBuf;
use tauri_specta::Event;

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
/// reconciles missed commits and database replacement every few seconds.
pub fn spawn<R: tauri::Runtime>(app: tauri::AppHandle<R>, state: PathBuf) {
    tauri::async_runtime::spawn(async move {
        let mut changes = Subscription::new(&state).await;
        loop {
            let sent = match changes.next().await {
                Change::StateInvalidated => StateChanged { resync: false }.emit(&app),
                Change::Resync => StateChanged { resync: true }.emit(&app),
                Change::SessionInvalidated(id) => SessionChanged {
                    session: id.clone().into(),
                }
                .emit(&app),
            };
            // The app is closing; nobody listens any more.
            if sent.is_err() {
                return;
            }
        }
    });
}
