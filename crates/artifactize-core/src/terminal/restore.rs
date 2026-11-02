//! A confirmed argv passes through the interactive shell without becoming shell source.
use super::TerminalCommand;
use crate::platform;
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
};

/// The confirmed argv and the directory it was remembered in. Shell startup files may change
/// the fresh shell's directory, so the trampoline runs the command where it was confirmed.
#[derive(Serialize, Deserialize)]
struct Confirmed {
    command: TerminalCommand,
    #[serde(with = "crate::platform::path_serde")]
    cwd: PathBuf,
}

pub(super) fn input(
    launch: &mut portable_pty::CommandBuilder,
    command: &TerminalCommand,
    cwd: &Path,
) -> io::Result<Vec<u8>> {
    launch.env(
        platform::terminal::RESTORE_EXECUTABLE,
        std::env::current_exe()?,
    );
    let confirmed = Confirmed {
        command: command.clone(),
        cwd: cwd.to_owned(),
    };
    launch.env(
        platform::terminal::RESTORE_COMMAND,
        serde_json::to_string(&confirmed)?,
    );
    Ok(platform::terminal::restore_input(&launch.get_shell()))
}

/// Hidden CLI entry point, invoked by the user's fresh shell only after restore confirmation.
/// On Unix this replaces the shell's child, preserving its foreground group and signals.
pub fn run_confirmed() -> Result<u8, String> {
    let payload = platform::environment::var_text(platform::terminal::RESTORE_COMMAND)
        .ok_or("No confirmed terminal command")?;
    let Confirmed {
        command: restored,
        cwd,
    } = serde_json::from_str(&payload).map_err(|e| e.to_string())?;
    let program =
        platform::program::resolve(restored.program.as_ref(), &cwd).map_err(|e| e.to_string())?;
    let mut command = std::process::Command::new(program);
    command
        .args(restored.args)
        .current_dir(cwd)
        .env_clear()
        .envs(platform::environment::for_command());
    platform::terminal::run_confirmed(command).map_err(|e| e.to_string())
}
