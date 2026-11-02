//! Native foreground-process observation and terminal process ownership.
use crate::terminal::TerminalCommand;
use std::path::PathBuf;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;
#[cfg(unix)]
pub(crate) use unix::{Processes, foreground, restore_input, run_confirmed};
#[cfg(windows)]
pub(crate) use windows::{Processes, foreground, restore_input, run_confirmed};

pub(crate) struct Foreground {
    pub cwd: Option<PathBuf>,
    pub command: Option<TerminalCommand>,
    pub shell: bool,
}

/// The prompt invokes this executable; the restored program and argv stay in an encoded
/// payload and are never interpreted as shell source.
pub(crate) const RESTORE_EXECUTABLE: &str = "ARTIFACTIZE_TERMINAL_EXECUTABLE";
pub(crate) const RESTORE_COMMAND: &str = "ARTIFACTIZE_TERMINAL_COMMAND";
