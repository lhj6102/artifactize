//! Unix desktop handoff and WSL opener status policy.

use std::{
    ffi::OsStr,
    io,
    process::{ExitStatus, Stdio},
    time::Duration,
};

/// The desktop's own opener program: `open` on macOS; `xdg-open` elsewhere, WSL included.
#[cfg(target_os = "macos")]
const OPENER: &str = "open";
#[cfg(not(target_os = "macos"))]
const OPENER: &str = "xdg-open";

/// Run the desktop opener with the target as its one argument, never through a shell.
pub(crate) async fn open_desktop(target: &OsStr, deadline: Duration) -> io::Result<()> {
    let cwd = std::env::current_dir()?;
    let program = crate::program::resolve(OsStr::new(OPENER), &cwd)?;
    let mut command = tokio::process::Command::new(program);
    command
        .arg(target)
        .env_clear()
        .envs(crate::environment::user())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    // The opener runs as a group of its own, so a timeout stops it with what it started.
    let mut tree = super::spawn_tree(&mut command)?;
    let status = match tokio::time::timeout(deadline, tree.child.wait()).await {
        Ok(status) => status?,
        Err(_) => {
            tree.kill()?;
            tree.child.wait().await?;
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Desktop opener timed out.",
            ));
        }
    };
    if !opened(&status, is_wsl()) {
        return Err(io::Error::other(format!(
            "Desktop opener exited with {status}."
        )));
    }
    Ok(())
}

/// On WSL, xdg-utils opens files with `explorer.exe`, which exits 1 even after opening the
/// file, and xdg-open then reports "the action failed" with this status (#210).
const WSL_EXPLORER_STATUS: i32 = 4;

/// Whether the opener's exit status means the target was handed to the desktop.
fn opened(status: &ExitStatus, wsl: bool) -> bool {
    status.success() || (wsl && status.code() == Some(WSL_EXPLORER_STATUS))
}

/// Whether this is WSL, by the test xdg-open itself uses: `microsoft` in `/proc/version`.
fn is_wsl() -> bool {
    cfg!(target_os = "linux")
        && std::fs::read_to_string("/proc/version")
            .is_ok_and(|version| version.contains("microsoft"))
}

// The opener's exit status exists only where an opener process runs: Unix, WSL included.
// Windows hands the target to ShellExecute instead.
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;

    /// A Unix wait status holds a normal exit's code in bits 8–15.
    const EXIT_CODE_SHIFT: i32 = 8;

    /// An exit status with `code`, in the wait-status encoding `from_raw` takes.
    fn exited(code: i32) -> ExitStatus {
        ExitStatus::from_raw(code << EXIT_CODE_SHIFT)
    }

    #[test]
    fn wsl_explorer_status_counts_as_opened_only_on_wsl() {
        assert!(opened(&exited(0), false));
        assert!(opened(&exited(0), true));
        assert!(opened(&exited(4), true));
        assert!(!opened(&exited(4), false));
        for code in [1, 2, 3] {
            assert!(!opened(&exited(code), true));
        }
    }
}
