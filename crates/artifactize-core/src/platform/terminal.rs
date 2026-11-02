//! Native foreground-process observation and terminal process ownership.
use crate::terminal::TerminalCommand;
use std::path::PathBuf;
mod pty;
pub(crate) use portable_pty::PtySize as Size;
pub(crate) use pty::{Output, Pty};
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;
#[cfg(unix)]
pub(crate) use unix::{Modes, Processes, STOP_BEFORE_REMOVE, foreground, wait_host};
#[cfg(windows)]
pub(crate) use windows::{Modes, Processes, STOP_BEFORE_REMOVE, foreground, wait_host};

pub(crate) struct Foreground {
    pub cwd: Option<PathBuf>,
    pub command: Option<TerminalCommand>,
    pub shell: bool,
}

/// Windows keeps the shell's current directory open. Stop and join its host before Git
/// removes that directory; retain the election lock until Git finishes removing it.
pub(crate) fn before_remove(worktree: &std::path::Path) -> Result<Option<std::fs::File>, String> {
    if !STOP_BEFORE_REMOVE {
        return Ok(None);
    }
    let worktree = worktree.to_owned();
    let thread = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        runtime
            .block_on(crate::terminal::stop_for_removal(&worktree))
            .map_err(|e| e.to_string())
    });
    match thread.join() {
        Ok(result) => result.map(Some),
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

pub(crate) fn host_identity() -> std::io::Result<crate::process::ChildIdentity> {
    crate::process::child_identity(std::process::id())
}
