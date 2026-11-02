use super::Foreground;
use portable_pty::{Child, MasterPty};
use std::io;

// Keep the PTY child and descendants in a kill-on-close Job.
pub(crate) use crate::platform::windows::process::TerminalJob as Processes;

pub(crate) fn foreground(_master: &dyn MasterPty, _shell_pid: Option<u32>) -> Option<Foreground> {
    // ConPTY exposes no foreground group. OSC 7 supplies cwd; command stays unknown.
    None
}

impl Processes {
    pub fn new(child: &dyn Child) -> io::Result<Self> {
        Self::attach(child)
    }
}
