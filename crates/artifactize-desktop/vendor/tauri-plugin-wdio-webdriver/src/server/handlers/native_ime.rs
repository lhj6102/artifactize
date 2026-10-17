//! Vendor WebDriver input-method endpoint; never an application IPC command.
use crate::{
    platform::native_ime,
    server::{
        response::{WebDriverErrorResponse, WebDriverResponse, WebDriverResult},
        AppState,
    },
};
use axum::{
    extract::{Path, State},
    Json,
};
use std::{sync::Arc, time::Duration};
use tauri::Runtime;

pub async fn input<R: Runtime + 'static>(
    State(state): State<Arc<AppState<R>>>,
    Path(id): Path<String>,
    Json(input): Json<native_ime::Input>,
) -> WebDriverResult {
    let sessions = state.sessions.read().await;
    let session = sessions.get(&id)?;
    let window = session.current_window.clone();
    let timeouts = session.timeouts.clone();
    let frames = session.frame_context.clone();
    let timeout = Duration::from_millis(timeouts.script_ms);
    drop(sessions);
    let executor = state.get_executor_for_window(&window, timeouts, frames)?;
    tokio::time::timeout(timeout, native_ime::apply(executor.window(), input))
        .await
        .map_err(|_| WebDriverErrorResponse::script_timeout())??;
    Ok(WebDriverResponse::success(serde_json::Value::Null))
}
