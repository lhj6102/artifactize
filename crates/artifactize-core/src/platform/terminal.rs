//! Native foreground-process observation and terminal process ownership.
use crate::terminal::TerminalCommand;
use std::path::PathBuf;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;
#[cfg(unix)]
pub(crate) use unix::{Processes, foreground};
#[cfg(windows)]
pub(crate) use windows::{Processes, foreground};

pub(crate) struct Foreground {
    pub cwd: Option<PathBuf>,
    pub command: Option<TerminalCommand>,
    pub shell: bool,
}
