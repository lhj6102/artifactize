//! Thin Git and terminal commands. Core owns host lifetime, PTYs, restore and transport.
use crate::{App, Failure, failure::joined};
use artifactize_core::{
    repository::{self, Commit, CreatedWorktree},
    terminal::{self, Attachment, RestoreChoice, TerminalFrame},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::Arc,
};
use tauri::{State, ipc::Channel};
use tokio::sync::Mutex;

#[derive(Default)]
pub(crate) struct Terminals {
    // Client addresses are worktree paths, never branch keys. Core resolves the stable Git
    // identity once at attach; keystrokes require no filesystem scan or Git subprocess.
    attached: Arc<Mutex<BTreeMap<PathBuf, Attachment>>>,
    /// Worktrees whose terminal ends with the app: the system keeps their hosts in the Job
    /// the app runs in, so the app ends them itself as it closes rather than whenever that
    /// Job does.
    ending: Arc<Mutex<BTreeSet<PathBuf>>>,
}
impl Terminals {
    pub fn close(&self) -> impl Future<Output = ()> + use<> {
        let attached = self.attached.clone();
        async move {
            attached.lock().await.clear();
        }
    }
    /// End the terminals that end with the app, their programs included, through core's
    /// owner-checked shutdown. Persistent terminals keep running.
    pub fn end(&self) -> impl Future<Output = ()> + use<> {
        let ending = self.ending.clone();
        async move {
            let mut stops = tokio::task::JoinSet::new();
            for worktree in std::mem::take(&mut *ending.lock().await) {
                stops.spawn(async move { terminal::stop(&worktree).await });
            }
            while let Some(stopped) = stops.join_next().await {
                // A panic resumes here; a cancelled stop has nothing left to report.
                if let Ok(Err(error)) = artifactize_core::task::joined(stopped) {
                    eprintln!("cannot end a terminal that ends with the app: {error}");
                }
            }
        }
    }
    async fn get(&self, worktree: PathBuf) -> Result<Attachment, Failure> {
        self.attached
            .lock()
            .await
            .get(&worktree)
            .cloned()
            .ok_or_else(|| Failure::Missing("Terminal is not attached".into()))
    }
}
#[tauri::command]
#[specta::specta]
pub async fn git_history(worktree: PathBuf) -> Result<Vec<Commit>, Failure> {
    joined(tokio::task::spawn_blocking(move || repository::history(&worktree)).await)?
        .map_err(Failure::Read)
}
#[tauri::command]
#[specta::specta]
pub async fn worktree_create(
    worktree: PathBuf,
    commit: String,
) -> Result<CreatedWorktree, Failure> {
    joined(
        tokio::task::spawn_blocking(move || repository::create_worktree(&worktree, &commit)).await,
    )?
    .map_err(Failure::Action)
}
#[tauri::command]
#[specta::specta]
pub async fn worktree_remove(worktree: PathBuf) -> Result<(), Failure> {
    joined(tokio::task::spawn_blocking(move || repository::remove_worktree(&worktree)).await)?
        .map_err(Failure::Action)
}
#[tauri::command]
#[specta::specta]
pub async fn terminal_open(
    app: State<'_, App>,
    worktree: PathBuf,
    cols: u16,
    rows: u16,
    frames: Channel<TerminalFrame>,
) -> Result<(), Failure> {
    let mut attached = app.terminals.attached.lock().await;
    let (terminal, mut output) = terminal::attach(&worktree, cols, rows)
        .await
        .map_err(Failure::Action)?;
    if !terminal.persistent() {
        app.terminals.ending.lock().await.insert(worktree.clone());
    }
    attached.insert(worktree, terminal);
    tokio::spawn(async move {
        while let Some(frame) = output.recv().await {
            if frames.send(frame).is_err() {
                break;
            }
        }
    });
    Ok(())
}
#[tauri::command]
#[specta::specta]
pub async fn terminal_input(
    app: State<'_, App>,
    worktree: PathBuf,
    data: Vec<u8>,
) -> Result<(), Failure> {
    app.terminals
        .get(worktree)
        .await?
        .input(data)
        .await
        .map_err(Failure::Action)
}
#[tauri::command]
#[specta::specta]
pub async fn terminal_resize(
    app: State<'_, App>,
    worktree: PathBuf,
    cols: u16,
    rows: u16,
) -> Result<(), Failure> {
    app.terminals
        .get(worktree)
        .await?
        .resize(cols, rows)
        .await
        .map_err(Failure::Action)
}
#[tauri::command]
#[specta::specta]
pub async fn terminal_close(app: State<'_, App>, worktree: PathBuf) -> Result<(), Failure> {
    let terminal = app.terminals.attached.lock().await.remove(&worktree);
    if let Some(terminal) = terminal {
        terminal.detach().await.map_err(Failure::Action)?;
    }
    Ok(())
}
#[tauri::command]
#[specta::specta]
pub async fn terminal_restore(
    app: State<'_, App>,
    worktree: PathBuf,
    choice: RestoreChoice,
) -> Result<(), Failure> {
    app.terminals
        .get(worktree)
        .await?
        .restore(choice)
        .await
        .map_err(Failure::Action)
}
