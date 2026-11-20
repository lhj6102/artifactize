//! The crate's platform layer: every operating-system call and `cfg` branch lives here,
//! behind one portable interface. `files` holds the pinned, no-follow file access; the rest
//! is program lookup details, the environment of started programs (without what an AppImage
//! adds), process trees for the standalone command, and the desktop opener.
//!
//! Kept as is (#201): process-wrap 10.0.1's Windows JobObject assigns its job after spawning;
//! it does not offer creation-time assignment. Keep our suspended-spawn/assignment policy
//! and owned SuspendedChild resumption instead of substituting a different tree lifecycle.
//! `which` reads PATHEXT from the current process rather than the child's environment, and
//! appends extensions even to names with a non-PATHEXT extension; our lookup does neither.
//! `open` tries PowerShell before xdg-open on WSL; our WSL policy is xdg-open only.

pub mod files;

#[cfg(target_os = "linux")]
mod appimage;
pub mod environment;
pub(crate) mod outcome;
pub(crate) mod process;
pub mod program;
pub mod text;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
use unix as os;
#[cfg(windows)]
use windows as os;

pub(crate) use os::{USES_PATHEXT, is_executable, open_desktop, spawn_tree};

/// Resolves when the terminal interrupts this process: Ctrl+C, as Unix and Windows deliver it,
/// or, on Windows, its console closing.
pub(crate) async fn interrupted() {
    #[cfg(windows)]
    {
        let close = async {
            match tokio::signal::windows::ctrl_close() {
                Ok(mut close) => {
                    close.recv().await;
                }
                Err(_) => std::future::pending::<()>().await,
            }
        };
        tokio::select! {
            () = interrupt() => {}
            () = close => {}
        }
    }
    #[cfg(not(windows))]
    interrupt().await;
}

async fn interrupt() {
    if tokio::signal::ctrl_c().await.is_err() {
        // Without a handler there is no interrupt to wait for.
        std::future::pending::<()>().await;
    }
}
