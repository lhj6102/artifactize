//! Unix program lookup details and the desktop opener.

use std::{
    ffi::OsStr, io, os::unix::fs::PermissionsExt, path::Path, process::Stdio, time::Duration,
};

/// Unix names programs exactly; extensions carry no meaning for lookup.
pub(crate) const USES_PATHEXT: bool = false;

/// Any execute bit is the declared-command preflight rule.
const EXECUTE_BITS: u32 = 0o111;

pub(crate) fn is_executable(path: &Path) -> bool {
    path.metadata().is_ok_and(|metadata| {
        metadata.is_file() && metadata.permissions().mode() & EXECUTE_BITS != 0
    })
}

/// Desktop handoff must not hold a sign-in or Human tool indefinitely.
const OPEN_TIMEOUT: Duration = Duration::from_secs(3);

/// The desktop's own opener program: `open` on macOS; `xdg-open` elsewhere, WSL included.
#[cfg(target_os = "macos")]
const OPENER: &str = "open";
#[cfg(not(target_os = "macos"))]
const OPENER: &str = "xdg-open";

/// Run the desktop opener with the target as its one argument, never through a shell.
pub(crate) async fn open_desktop(target: &OsStr) -> io::Result<()> {
    let cwd = std::env::current_dir()?;
    let program = crate::program::resolve(OsStr::new(OPENER), &cwd)?;
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
