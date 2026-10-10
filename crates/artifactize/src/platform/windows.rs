//! Windows owner-only DACLs, Job Objects, and console modes and events.
//! Pinned file access lives in artifactize-tools.

pub(crate) mod ipc;
mod process;
mod security;

use std::{
    fs::File, future::Future, io, os::windows::io::AsRawHandle, path::Path, process::ExitStatus,
    ptr,
};

use tokio::signal::windows::{ctrl_break, ctrl_c, ctrl_close};
use windows_sys::Win32::{
    Foundation::{HANDLE, INVALID_HANDLE_VALUE},
    Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle},
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
    create_private_dir, create_private_dir_all, is_private_dir, is_private_file, private_options,
    private_pipe, private_tempdir_in, restrict_file, user_identity,
};

/// NTFS persists a rename through its journal, and Windows cannot flush a directory handle
/// opened for reading, so there is nothing to sync.
pub(crate) fn sync_dir(_path: &Path) -> io::Result<()> {
    Ok(())
}

/// A file's identity on its volume: the volume serial number and the file index.
pub(crate) fn file_identity(file: &File) -> io::Result<super::FileIdentity> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: an open file handle and a correctly sized writable output.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(super::FileIdentity {
        volume: u64::from(info.dwVolumeSerialNumber),
        index: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    })
}

/// Whether an open file or directory belongs to the current user and grants access to
/// nobody else; see `security::is_owner_only`.
pub(crate) fn is_owner_only(file: &File) -> io::Result<bool> {
    security::is_owner_only(file.as_raw_handle())
}

/// Windows compares environment variable names without regard to case.
pub(crate) const ENV_NAMES_IGNORE_CASE: bool = true;

/// The variable naming the user's home directory, which Unix-style tools read on Windows too.
pub(crate) const HOME_VARIABLE: &str = "HOME";

/// Variables Windows programs, and Unix-style tools on Windows, read their home from.
pub(crate) const HOME_VARIABLES: &[&str] = &[HOME_VARIABLE, "USERPROFILE", "APPDATA"];

/// Variables programs read their cache directory from.
pub(crate) const CACHE_VARIABLES: &[&str] = &["XDG_CACHE_HOME", "LOCALAPPDATA"];

/// Variables programs read their temporary directory from.
pub(crate) const TEMP_VARIABLES: &[&str] = &["TMPDIR", "TMP", "TEMP"];

/// System variables many Windows programs cannot start without.
pub(crate) const SYSTEM_VARIABLES: &[&str] = &["SystemRoot", "ComSpec", "PATHEXT"];

/// Variables naming a per-user state directory, in order; artifactize keeps its state in an
/// `artifactize` folder below the first one set, and otherwise below `HOME`.
pub(crate) const STATE_VARIABLES: &[&str] = &["XDG_STATE_HOME", "LOCALAPPDATA"];

/// The variables naming the signed-in user, in order; Windows sets `USERNAME`, not `USER`.
pub(crate) const USER_VARIABLES: &[&str] = &["USER", "USERNAME"];

/// A path from the bytes a tool such as git prints, which are UTF-8 on Windows.
pub(crate) fn path_from_bytes(bytes: &[u8]) -> std::path::PathBuf {
    String::from_utf8_lossy(bytes).into_owned().into()
}

/// Windows programs end a line with CRLF, as Python's print does there.
pub(crate) const CRLF_LINE_ENDINGS: bool = true;

/// Whether a terminal that cannot hide input is still read, visibly, after a warning:
/// mintty (Git Bash) hands Windows programs a pipe that only looks like a terminal, with no
/// console echo to turn off.
pub(crate) const VISIBLE_INPUT_FALLBACK: bool = true;

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

/// The absolute form of `path`; Windows has no aliased system roots of the macOS kind, and
/// 8.3 names are resolved by canonicalization.
pub(crate) fn resolve_system_aliases(path: &Path) -> io::Result<std::path::PathBuf> {
    std::path::absolute(path)
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
