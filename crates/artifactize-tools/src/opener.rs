//! Open one path or URL with the desktop's default application, without a shell.

use std::{ffi::OsStr, io, time::Duration};

/// Desktop handoff must not hold a sign-in or Human tool indefinitely.
const OPEN_TIMEOUT: Duration = Duration::from_secs(3);

/// The platform layer chooses the desktop's own opener. The target is one argument, never a
/// shell command or command line.
pub async fn open(target: &OsStr) -> io::Result<()> {
    open_within(target, OPEN_TIMEOUT).await
}

/// [`open`] with the caller's own bound on how long the opener may take to hand off.
pub async fn open_within(target: &OsStr, deadline: Duration) -> io::Result<()> {
    crate::platform::open_desktop(target, deadline).await
}
