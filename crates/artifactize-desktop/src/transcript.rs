//! An Agent request's live transcript, read by the monitor's own session reader. The frontend
//! sizes the window in text columns and rows of its monospace pane, like a terminal would.
use crate::ids::TranscriptHandle;
use crate::{App, Failure};
use artifactize_core::{
    agent::session::{document::Move, live, transcript::BlockId},
    monitor::{EvidenceStamp, session::Live},
    store,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, atomic::AtomicU32},
};

/// Open transcripts by handle; each is read by at most one job at a time.
#[derive(Default)]
pub struct Transcripts {
    open: Mutex<BTreeMap<TranscriptHandle, Entry>>,
    next: AtomicU32,
}

/// An open reader and the request's session facts it was resolved for.
struct Entry {
    live: Arc<tokio::sync::Mutex<Live>>,
    stamp: EvidenceStamp,
}

/// The session behind a request, or why there is none to read.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Opened {
    Local {
        handle: TranscriptHandle,
        /// The session id, which change hints name.
        session: crate::ids::SessionKey,
    },
    /// Why the conversation cannot be shown.
    Unavailable { text: String },
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

/// Resolve request `id`'s session and open a reader for it. With the `handle` already open
/// for it, resolve again only when the request's session facts changed, as the terminal
/// monitor does: a queued request gains its session once it runs. An unchanged session keeps
/// its reader and position; another session replaces it.
#[tauri::command]
#[specta::specta]
pub async fn transcript_open(
    app: tauri::State<'_, App>,
    id: crate::ids::RequestKey,
    handle: Option<TranscriptHandle>,
) -> Result<Opened, Failure> {
    let view = store::read_request(&app.launch.state, &id.into_inner())
        .await
        .map_err(Failure::Read)?;
    let stamp = EvidenceStamp::new(&view);
    let current = handle.and_then(|handle| {
        let open = app.transcripts.open.lock().expect("transcripts lock");
        open.get(&handle)
            .map(|entry| (handle, entry.live.clone(), entry.stamp == stamp))
    });
    if let Some((handle, live, true)) = &current {
        let session = live.lock().await.source.reference.session_id.clone().into();
        return Ok(Opened::Local {
            handle: *handle,
            session,
        });
    }
    let resolved = live::resolve(&app.launch.state, &view).await;
    if let (Some((handle, live, _)), Ok(live::Resolution::Local(source))) = (&current, &resolved) {
        // The same session keeps its reader, position, follow state and expanded groups, as in
        // the terminal monitor: wait for a window job to finish with it rather than replace it.
        let mut reader = live.lock().await;
        if reader.refresh(source) {
            drop(reader);
            let mut open = app.transcripts.open.lock().expect("transcripts lock");
            // Closed meanwhile: a new reader opens below instead.
            if let Some(entry) = open.get_mut(handle) {
                entry.stamp = stamp;
                return Ok(Opened::Local {
                    handle: *handle,
                    session: source.reference.session_id.clone().into(),
                });
            }
        }
    }
    let mut open = app.transcripts.open.lock().expect("transcripts lock");
    // Another session, or none: the old reader goes.
    if let Some((handle, _, _)) = &current {
        open.remove(handle);
    }
    Ok(match resolved {
        Ok(live::Resolution::Local(source)) => {
            let handle = TranscriptHandle::allocate(&app.transcripts.next);
            let session = source.reference.session_id.clone().into();
            let live = Live::new(handle.number().into(), source);
            open.insert(
                handle,
                Entry {
                    live: Arc::new(tokio::sync::Mutex::new(live)),
                    stamp,
                },
            );
            Opened::Local { handle, session }
        }
        Ok(live::Resolution::Unavailable(text)) => Opened::Unavailable { text },
        Err(error) => Opened::Unavailable {
            text: format!("Conversation unavailable: {error}"),
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
    handle: TranscriptHandle,
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
        .map(|entry| entry.live.clone())
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
        let job = tokio::task::spawn_blocking(move || {
            let mut job = job;
            let window =
                job.reader
                    .step_expanded(job.width, job.height, job.position, &job.expanded);
            (job, window)
        })
        .await;
        let (job, window) = crate::failure::joined(job)?;
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
        following: live.scroll.mode == artifactize_core::agent::session::document::Mode::Following,
        loading: window.loading,
    })
}

/// Close transcript `handle`; its reader removes its temporary files.
#[tauri::command]
#[specta::specta]
pub fn transcript_close(app: tauri::State<'_, App>, handle: TranscriptHandle) {
    app.transcripts
        .open
        .lock()
        .expect("transcripts lock")
        .remove(&handle);
}
