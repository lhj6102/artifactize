//! Unix executable lookup and process groups.

use std::{io, os::unix::fs::PermissionsExt, path::Path};

mod desktop;
pub(crate) use desktop::open_desktop;

/// Resolves when the terminal hangs up (`SIGHUP`) or the process is asked to terminate
/// (`SIGTERM`); a signal whose handler cannot be installed is never awaited.
pub(crate) async fn hung_up() {
    use tokio::signal::unix::{SignalKind, signal};
    async fn delivered(kind: SignalKind) {
        match signal(kind) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    }
    tokio::select! {
        () = delivered(SignalKind::hangup()) => {}
        () = delivered(SignalKind::terminate()) => {}
    }
}

/// Unix names programs exactly; extensions carry no meaning for lookup.
pub(crate) const USES_PATHEXT: bool = false;

/// Any execute bit is the declared-command preflight rule.
const EXECUTE_BITS: u32 = 0o111;

pub(crate) fn is_executable(path: &Path) -> bool {
    path.metadata().is_ok_and(|metadata| {
        metadata.is_file() && metadata.permissions().mode() & EXECUTE_BITS != 0
    })
}

/// A started program in a process group of its own, so that its children can be killed
/// with it.
pub(crate) struct Tree {
    pub(crate) child: tokio::process::Child,
    group: Option<GroupId>,
}

/// A process group created by this launcher, never an arbitrary process identifier.
struct GroupId(rustix::process::Pid);

/// A group ID of zero selects the child's own PID as its new group ID.
const CHILD_PID_AS_GROUP: i32 = 0;

/// Start `command` as the leader of a new process group.
pub(crate) fn spawn_tree(command: &mut tokio::process::Command) -> io::Result<Tree> {
    command.process_group(CHILD_PID_AS_GROUP);
    let child = command.spawn()?;
    let group = child.id().map(|pid| {
        let pid = i32::try_from(pid).expect("Unix process IDs fit pid_t");
        GroupId(rustix::process::Pid::from_raw(pid).expect("a child's process ID is positive"))
    });
    Ok(Tree { child, group })
}

impl Tree {
    /// Kill every process left in the group; a group that has already gone is fine, any
    /// other failure is the caller's.
    pub(crate) fn kill(&mut self) -> std::io::Result<()> {
        if let Some(group) = self.group.take() {
            match rustix::process::kill_process_group(group.0, rustix::process::Signal::KILL) {
                Ok(()) | Err(rustix::io::Errno::SRCH) => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        // A drop has no caller to report to; the launcher has already killed and reported.
        let _ = self.kill();
    }
}
