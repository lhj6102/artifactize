//! Windows program lookup details and the ShellExecute desktop opener.

use std::{ffi::OsStr, io, os::windows::ffi::OsStrExt, path::Path, ptr};

use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};

/// An extensionless program name is tried with each `PATHEXT` suffix.
pub(crate) const USES_PATHEXT: bool = true;

/// Windows has no execute bit; any regular file is a candidate.
pub(crate) fn is_executable(path: &Path) -> bool {
    path.metadata().is_ok_and(|metadata| metadata.is_file())
}

/// Hand the target to ShellExecute's default verb on a blocking thread.
pub(crate) async fn open_desktop(target: &OsStr) -> io::Result<()> {
    let target = target.to_owned();
    tokio::task::spawn_blocking(move || shell_execute(&target))
        .await
        .map_err(io::Error::other)?
}

fn shell_execute(target: &OsStr) -> io::Result<()> {
    let mut target: Vec<_> = target.encode_wide().collect();
    if target.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Open target contains NUL.",
        ));
    }
    target.push(0);
    // SAFETY: target is NUL-terminated and remains alive for the call. Null operation
    // selects the default verb; no parameters or directory are supplied.
    let result = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            ptr::null(),
            target.as_ptr(),
            ptr::null(),
            ptr::null(),
            SW_SHOWNORMAL,
        )
    } as isize;
    // ShellExecute returns a value greater than 32 on success, not an owned handle.
    if result <= 32 {
        return Err(io::Error::other(format!(
            "ShellExecute could not open the target (code {result})."
        )));
    }
    Ok(())
}
