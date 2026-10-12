//! An Agent request's live transcript, read by the monitor's own session reader. The frontend
//! sizes the window in text columns and rows of its monospace pane, like a terminal would.
use crate::{App, Failure};
use artifactize::{
    agent::session::{document::Move, live, transcript::BlockId},
    monitor::session::Live,
    store,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
};

/// Open transcripts by handle; each is read by at most one job at a time.
#[derive(Default)]
pub struct Transcripts {
    open: Mutex<BTreeMap<u32, Arc<tokio::sync::Mutex<Live>>>>,
    next: AtomicU32,
}

/// The session behind a request, or why there is none to read.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Opened {
    pub handle: Option<u32>,
    /// The session id, which change hints name.
    pub session: Option<String>,
    /// Why the conversation cannot be shown.
    pub text: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Type)]
#[serde(tag = "kind", content = "rows", rename_all = "camelCase")]
pub enum Movement {
    Up(u32),
    Down(u32),
    Top,
    Bottom,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub text: String,
    /// A collapsed tool activity line; toggling it shows names, targets and states.
    pub group: bool,
    /// Under the subdued Thinking label.
    pub thinking: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptView {
    pub rows: Vec<Row>,
    /// Row position, bottom or history, following or paused.
    pub indicator: String,
    pub status: Option<String>,
    pub following: bool,
    /// More indexing or layout is pending; ask again for the rest.
    pub loading: bool,
}

/// Resolve request `id`'s session and open a reader for it.
#[tauri::command]
#[specta::specta]
pub async fn transcript_open(app: tauri::State<'_, App>, id: String) -> Result<Opened, Failure> {
    let view = store::read_request(&app.launch.state, &id)
        .await
        .map_err(Failure::Read)?;
    Ok(match live::resolve(&app.launch.state, &view).await {
        Ok(live::Resolution::Local(source)) => {
            let handle = app.transcripts.next.fetch_add(1, Ordering::Relaxed);
            let session = source.reference.session_id.to_string();
            app.transcripts
                .open
                .lock()
                .expect("transcripts lock")
                .insert(
                    handle,
                    Arc::new(tokio::sync::Mutex::new(Live::new(
                        u64::from(handle),
                        source,
                    ))),
                );
            Opened {
                handle: Some(handle),
                session: Some(session),
                text: None,
            }
        }
        Ok(live::Resolution::Unavailable(text)) => Opened {
            handle: None,
            session: None,
            text: Some(text),
        },
        Err(error) => Opened {
            handle: None,
            session: None,
            text: Some(format!("Conversation unavailable: {error}")),
        },
    })
}

/// The visible rows of transcript `handle` in a `columns` × `rows` pane, after moving and
/// toggling the activity group on `toggle` (a row of the last window). `changed` re-reads
/// the session file after a change hint or probe.
#[tauri::command]
#[specta::specta]
pub async fn transcript_window(
    app: tauri::State<'_, App>,
    handle: u32,
    columns: u32,
    rows: u32,
    movement: Option<Movement>,
    toggle: Option<u32>,
    changed: bool,
) -> Result<TranscriptView, Failure> {
    let live = app
        .transcripts
        .open
        .lock()
        .expect("transcripts lock")
        .get(&handle)
        .cloned()
        .ok_or_else(|| Failure::Missing("The transcript was closed.".into()))?;
    let mut live = live.lock().await;
    live.geometry(columns as usize, rows as usize);
    if changed {
        live.invalidate();
    }
    if let Some(movement) = movement {
        live.movement(match movement {
            Movement::Up(rows) => Move::Up(rows as usize),
            Movement::Down(rows) => Move::Down(rows as usize),
            Movement::Top => Move::Top,
            Movement::Bottom => Move::Bottom,
        });
    }
    if let Some(row) = toggle {
        let group = live
            .window
            .groups
            .iter()
            .find(|(at, _)| *at == row as usize)
            .map(|(_, id)| *id);
        if let Some(BlockId(id)) = group {
            live.toggle(BlockId(id));
        }
    }
    // One bounded job per call; `loading` asks the caller for the next one.
    if let Some(job) = live.job() {
        let (job, window) = tokio::task::spawn_blocking(move || {
            let mut job = job;
            let window =
                job.reader
                    .step_expanded(job.width, job.height, job.position, &job.expanded);
            (job, window)
        })
        .await
        .map_err(|error| Failure::Read(format!("Session reader job failed: {error}")))?;
        live.finish(job, window);
    }
    let window = &live.window;
    Ok(TranscriptView {
        rows: window
            .rows
            .iter()
            .enumerate()
            .map(|(index, text)| Row {
                text: text.clone(),
                group: window.groups.iter().any(|(row, _)| *row == index),
                thinking: window.thinking.contains(&index),
            })
            .collect(),
        indicator: live.indicator(),
        status: window.status.clone(),
        following: live.scroll.mode == artifactize::agent::session::document::Mode::Following,
        loading: window.loading,
    })
}

/// Close transcript `handle`; its reader removes its temporary files.
#[tauri::command]
#[specta::specta]
pub fn transcript_close(app: tauri::State<'_, App>, handle: u32) {
    app.transcripts
        .open
        .lock()
        .expect("transcripts lock")
        .remove(&handle);
}
