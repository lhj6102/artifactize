//! The crate's platform layer: every operating-system call and `cfg` branch lives here,
//! behind one portable interface. `files` holds the pinned, no-follow file access; the rest
//! is program lookup details, process trees for the standalone command, and the desktop
//! opener.

pub mod files;
pub mod program;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
use unix as os;
#[cfg(windows)]
use windows as os;

pub(crate) use os::{SYSTEM_VARIABLES, USES_PATHEXT, is_executable, open_desktop, spawn_tree};
