//! Open one path or URL with the desktop's default application, without a shell.

use std::{ffi::OsStr, io};

/// The platform layer chooses the desktop's own opener. The target is one argument, never a
/// shell command or command line.
pub async fn open(target: &OsStr) -> io::Result<()> {
    crate::platform::open_desktop(target).await
}
