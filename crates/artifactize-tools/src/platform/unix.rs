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

/// Variables, besides `PATH` and the locale, that programs need to start: the home and
/// temporary directories.
pub const SYSTEM_VARIABLES: &[&str] = &["HOME", "TMPDIR"];

/// A started program in a process group of its own, so that its children can be killed
/// with it.
pub(crate) struct Tree {
    pub(crate) child: tokio::process::Child,
    group: Option<libc::pid_t>,
}

/// Start `command` as the leader of a new process group.
pub(crate) fn spawn_tree(command: &mut tokio::process::Command) -> io::Result<Tree> {
    command.process_group(0);
    let child = command.spawn()?;
    let group = child
        .id()
        .map(|pid| libc::pid_t::try_from(pid).expect("Unix process IDs fit pid_t"));
    Ok(Tree { child, group })
}

impl Tree {
    /// Kill every process left in the group; a group that has already gone is fine.
    pub(crate) fn kill(&mut self) {
        if let Some(group) = self.group.take() {
            // SAFETY: a plain signal to the process group this tree started.
            unsafe { libc::killpg(group, libc::SIGKILL) };
        }
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        self.kill();
    }
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
