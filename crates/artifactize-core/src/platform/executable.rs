//! Native executable identity for launching another copy of this process.

use std::{io, path::PathBuf};

/// The running executable's native path, with filesystem aliases resolved for relaunch.
pub fn current_executable() -> io::Result<PathBuf> {
    super::canonicalize(&std::env::current_exe()?)
}
