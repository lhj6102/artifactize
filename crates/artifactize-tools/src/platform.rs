//! The crate's platform layer: every operating-system call and `cfg` branch lives here,
//! behind one portable interface. `files` holds the pinned, no-follow file access; the rest
//! is program lookup details, process trees for the standalone command, and the desktop
//! opener.
//!
//! Kept as is (#201): process-wrap 10.0.1's Windows JobObject assigns its job after spawning;
//! it does not offer creation-time assignment. Keep our suspended-spawn/assignment policy
//! and owned SuspendedChild resumption instead of substituting a different tree lifecycle.
//! `which` reads PATHEXT from the current process rather than the child's environment, and
//! appends extensions even to names with a non-PATHEXT extension; our lookup does neither.
//! `open` tries PowerShell before xdg-open on WSL; our WSL policy is xdg-open only.

pub mod files;

/// How a finished process ended, for a message: its exit code, or on Unix the signal that
/// ended it, in the system's own wording.
pub(crate) fn exit_description(status: &std::process::ExitStatus) -> String {
    status.to_string()
}
/// Whether a native process exited successfully, including Unix signal termination.
pub(crate) fn succeeded(status: &std::process::ExitStatus) -> bool {
    status.success()
}
pub(crate) mod process;
pub mod program;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
use unix as os;
#[cfg(windows)]
use windows as os;

pub(crate) use os::{USES_PATHEXT, is_executable, open_desktop, spawn_tree};
