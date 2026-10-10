//! Open one path or URL with the desktop's default application, without a shell.

use std::{ffi::OsStr, io};

/// Linux, including WSL, uses only `xdg-open`; macOS uses `open`; Windows uses
/// ShellExecute. The target is one argument, never a shell command or command line.
pub async fn open(target: &OsStr) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::{process::Stdio, time::Duration};
        // Desktop handoff must not hold a sign-in or Human tool indefinitely.
        const OPEN_TIMEOUT: Duration = Duration::from_secs(3);
        #[cfg(target_os = "macos")]
        const PROGRAM: &str = "open";
        #[cfg(not(target_os = "macos"))]
        const PROGRAM: &str = "xdg-open";
        let cwd = std::env::current_dir()?;
        let program = crate::program::resolve(OsStr::new(PROGRAM), &cwd)?;
        let mut command = tokio::process::Command::new(program);
        command
            .arg(target)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let status = tokio::time::timeout(OPEN_TIMEOUT, command.status())
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "Desktop opener timed out."))??;
        if !status.success() {
            return Err(io::Error::other(format!(
                "Desktop opener exited with {status}."
            )));
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        let target = target.to_owned();
        tokio::task::spawn_blocking(move || shell_execute(&target))
            .await
            .map_err(io::Error::other)?
    }
}

#[cfg(windows)]
fn shell_execute(target: &OsStr) -> io::Result<()> {
    use std::{os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};

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
