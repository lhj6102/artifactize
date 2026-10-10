//! Windows owner-only DACLs, Job Objects, and console modes and events.
//! Pinned file access lives in artifactize-tools.

mod process;
mod security;

use std::{future::Future, io, path::Path, process::ExitStatus, ptr};

use tokio::signal::windows::{ctrl_break, ctrl_c, ctrl_close};
use windows_sys::Win32::{
    Foundation::{HANDLE, INVALID_HANDLE_VALUE},
    System::{
        Console::{
            CONSOLE_MODE, ENABLE_ECHO_INPUT, GetConsoleMode, GetStdHandle, STD_INPUT_HANDLE,
            SetConsoleMode,
        },
        SystemInformation::{ComputerNameDnsHostname, GetComputerNameExW},
    },
};

pub(crate) use process::{Child, process_start_time, spawn_detached, spawn_gated};
pub(crate) use security::{
    create_private_dir, create_private_dir_all, is_owner_only, is_private_dir, is_private_file,
    private_options, private_pipe, private_tempdir_in, restrict_file, user_identity,
};

/// NTFS persists a rename through its journal, and Windows cannot flush a directory handle
/// opened for reading, so there is nothing to sync.
pub(crate) fn sync_dir(_path: &Path) -> io::Result<()> {
    Ok(())
}

/// Windows has no execute bit. `std::process::Command` runs a path as given or, without an
/// extension, with `.exe` appended, so either existing as a file counts.
pub(crate) fn is_executable(path: &Path) -> bool {
    let file = |path: &Path| path.metadata().is_ok_and(|m| m.is_file());
    file(path) || (path.extension().is_none() && file(&path.with_extension("exe")))
}

/// The editor a review opens fields in when `EDITOR` is unset.
pub(crate) const DEFAULT_EDITOR: &str = "notepad";

/// Run `$EDITOR file` through cmd, not a Unix shell: `EDITOR` may hold arguments, such as
/// `code --wait`, or name a `.cmd` shim, which only cmd finds. `/s` strips only the outer
/// quotes, so the file stays one quoted word; `/d` skips AutoRun commands.
pub(crate) fn editor(editor: &str, file: &Path) -> tokio::process::Command {
    let mut command = tokio::process::Command::new("cmd");
    command.raw_arg(format!("/d /s /c \"{editor} \"{}\"\"", file.display()));
    command
}

/// Windows processes end with an exit code, never a signal.
pub(crate) fn exit_signal(_status: &ExitStatus) -> Option<i32> {
    None
}

pub(crate) fn host_name() -> Option<String> {
    let mut size = 0;
    // SAFETY: a null buffer only asks for the size, including the terminating NUL.
    unsafe { GetComputerNameExW(ComputerNameDnsHostname, ptr::null_mut(), &mut size) };
    let mut buffer = vec![0_u16; size as usize];
    // SAFETY: the buffer holds `size` UTF-16 units.
    if unsafe { GetComputerNameExW(ComputerNameDnsHostname, buffer.as_mut_ptr(), &mut size) } == 0 {
        return None;
    }
    buffer.truncate(size as usize);
    Some(String::from_utf16_lossy(&buffer))
}

/// Register for Ctrl-C, Ctrl-Break and console close now; the future completes at the first.
pub(crate) fn stop_requested() -> io::Result<impl Future<Output = ()> + Send + 'static> {
    let mut interrupt = ctrl_c()?;
    let mut brk = ctrl_break()?;
    let mut close = ctrl_close()?;
    Ok(async move {
        tokio::select! {
            _ = interrupt.recv() => {},
            _ = brk.recv() => {},
            _ = close.recv() => {},
        }
    })
}

/// Turns console echo off until dropped, so a pasted secret is not shown.
pub(crate) struct HiddenInput {
    console: HANDLE,
    mode: CONSOLE_MODE,
}

impl HiddenInput {
    pub fn new() -> io::Result<Self> {
        // SAFETY: GetStdHandle only reads the process's standard handle table.
        let console = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
        if console.is_null() || console == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let mut mode = 0;
        // SAFETY: a standard handle and a valid out pointer.
        if unsafe { GetConsoleMode(console, &mut mode) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the same console input handle.
        if unsafe { SetConsoleMode(console, mode & !ENABLE_ECHO_INPUT) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { console, mode })
    }
}

impl Drop for HiddenInput {
    fn drop(&mut self) {
        // SAFETY: restores the mode read in `new` on the same handle.
        unsafe { SetConsoleMode(self.console, self.mode) };
    }
}

#[cfg(test)]
mod tests {
    use std::process::Stdio;

    use super::*;

    #[tokio::test]
    async fn the_editor_runs_through_cmd_and_gets_the_file_as_one_word() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("fields file.json");
        std::fs::write(&file, "{}").unwrap();
        let edited = directory.path().join("edited.json");
        std::fs::write(&edited, r#"{"approved":true}"#).unwrap();
        // An editor command line with arguments of its own, which copies over what it opens.
        let command_line = format!("copy /y \"{}\"", edited.display());
        let status = editor(&command_line, &file)
            .stdout(Stdio::null())
            .status()
            .await
            .unwrap();
        assert!(status.success());
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            r#"{"approved":true}"#
        );
    }
}
