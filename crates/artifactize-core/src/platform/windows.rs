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
    Foundation::{ERROR_ACCESS_DENIED, ERROR_SHARING_VIOLATION, HANDLE, INVALID_HANDLE_VALUE},
    Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle},
    System::{
        Console::{
            CONSOLE_MODE, ENABLE_ECHO_INPUT, GetConsoleMode, GetStdHandle, STD_INPUT_HANDLE,
            SetConsoleMode,
        },
        SystemInformation::{ComputerNameDnsHostname, GetComputerNameExW},
    },
};

pub(crate) use process::{
    Child, kill_saved_group, kill_saved_session, process_start_time, running_process_start_time,
    spawn_detached, spawn_gated, spawn_run, spawn_watched,
};
pub(crate) use security::{
    PrivateTempDir, create_private_dir, create_private_dir_all, is_private_dir, is_private_file,
    private_options, private_pipe, private_tempdir_in, restrict_file, user_identity,
};

/// Owner death closes every Job handle, but descendant termination and handle
/// release are asynchronous. Recovery cannot query those now-closed Jobs, so it
/// waits briefly for their file sharing dispositions to clear, with a fixed bound.
pub(crate) fn remove_abandoned_directory(path: &Path) -> io::Result<()> {
    const RECOVERY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
    const SHARING_INTERVAL: std::time::Duration = std::time::Duration::from_millis(10);
    let deadline = std::time::Instant::now() + RECOVERY_TIMEOUT;
    loop {
        match std::fs::remove_dir_all(path) {
            Err(error)
                if error.raw_os_error().is_some_and(|code| {
                    code == ERROR_ACCESS_DENIED as i32 || code == ERROR_SHARING_VIOLATION as i32
                }) && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(SHARING_INTERVAL)
            }
            result => return result,
        }
    }
}

/// Whether `error` is Windows transiently refusing a metadata or open probe on a file that
/// another handle is still finalizing: emptying or deleting a WAL `-wal`/`-shm` sidecar, for
/// one, leaves the name `ERROR_ACCESS_DENIED` until that handle closes, even though no one
/// holds an incompatible lock. The condition clears on its own, so callers retry briefly
/// rather than surface it; any other error (a real permission problem, a missing file) is not
/// this and must not be retried as if it were.
pub(crate) fn transient_file_access(error: &io::Error) -> bool {
    error.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32)
}

/// How a private directory and file are described in messages.
pub(crate) const PRIVATE_DIRECTORY: &str = "owner-only (a protected DACL granting only this user)";
pub(crate) const PRIVATE_FILE: &str =
    "readable by their owner only (a DACL granting only this user)";

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

/// Variables programs read their temporary directory from.
pub(crate) const TEMP_VARIABLES: &[&str] = &["TMPDIR", "TMP", "TEMP"];

/// Variables naming a per-user state directory, in order; artifactize keeps its state in an
/// `artifactize` folder below the first one set, and otherwise below `HOME`.
pub(crate) const STATE_VARIABLES: &[&str] = &["XDG_STATE_HOME", "LOCALAPPDATA"];

/// The variables naming the signed-in user, in order; Windows sets `USERNAME`, not `USER`.
pub(crate) const USER_VARIABLES: &[&str] = &["USER", "USERNAME"];

/// The user's home directory: `HOME` when set, as Unix-style tools set it, else the
/// profile folder.
pub(crate) fn home_directory() -> Option<std::path::PathBuf> {
    [HOME_VARIABLE, "USERPROFILE"]
        .into_iter()
        .find_map(|name| std::env::var_os(name).filter(|path| !path.is_empty()))
        .map(Into::into)
}

/// A path from the bytes a tool such as git prints, which are UTF-8 on Windows.
pub(crate) fn path_from_bytes(bytes: &[u8]) -> std::path::PathBuf {
    String::from_utf8_lossy(bytes).into_owned().into()
}

/// Windows programs end a line with CRLF, as Python's print does there.
pub(crate) const CRLF_LINE_ENDINGS: bool = true;

/// Whether a terminal that cannot hide input is still read, visibly, after a warning:
/// mintty (Git Bash) hands Windows programs a pipe that only looks like a terminal, with no
/// console echo to turn off.
pub const VISIBLE_INPUT_FALLBACK: bool = true;

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
pub fn stop_requested() -> io::Result<impl Future<Output = ()> + Send + 'static> {
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
pub struct HiddenInput {
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
    use super::*;

    #[test]
    fn transient_file_access_is_only_error_access_denied() {
        assert!(transient_file_access(&io::Error::from_raw_os_error(
            ERROR_ACCESS_DENIED as i32
        )));
        for other in [
            2,  /* ERROR_FILE_NOT_FOUND */
            32, /* ERROR_SHARING_VIOLATION */
        ] {
            assert!(!transient_file_access(&io::Error::from_raw_os_error(other)));
        }
        assert!(!transient_file_access(&io::Error::new(
            io::ErrorKind::PermissionDenied,
            "no raw code"
        )));
    }

    // Only Windows has delete-sharing modes; Unix open handles do not block unlinking.
    #[test]
    fn delete_sharing_observation_never_removes_the_owned_file() {
        let root = crate::test_os::tempdir();
        let path = root.path().join("held-file");
        std::fs::write(&path, "owned content").unwrap();
        let holder = crate::test_os::hold_without_delete_sharing(&path);
        crate::test_os::assert_delete_blocked(&path);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "owned content");
        drop(holder);
        // A regressed fixture must fail the assertion without destroying its evidence.
        assert!(std::panic::catch_unwind(|| crate::test_os::assert_delete_blocked(&path)).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "owned content");
    }
}
