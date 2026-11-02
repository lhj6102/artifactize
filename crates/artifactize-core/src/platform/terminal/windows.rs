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

pub(crate) fn restore_input(shell: &str) -> Vec<u8> {
    let program = std::path::Path::new(shell)
        .file_stem()
        .and_then(|name| name.to_str());
    let executable = match program {
        Some("pwsh" | "powershell") => "& $env:ARTIFACTIZE_TERMINAL_EXECUTABLE",
        Some("nu") => "^$env.ARTIFACTIZE_TERMINAL_EXECUTABLE",
        _ => "\"%ARTIFACTIZE_TERMINAL_EXECUTABLE%\"",
    };
    format!("{executable} __terminal-command\r").into_bytes()
}

pub(crate) fn run_confirmed(mut command: std::process::Command) -> io::Result<u8> {
    let status = command.status()?;
    Ok(status.code().unwrap_or(1) as u8)
}
