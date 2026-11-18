//! Shared process outcomes for standalone and host launchers.

use std::process::ExitStatus;

/// How a finished process ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessEnd {
    /// It exited with this code.
    Exited(i32),
    /// It was ended without an exit code: on Unix by a signal, when known.
    Signaled(Option<i32>),
}

/// The reason a process ended and the host's wording for diagnostics.
#[derive(Debug)]
pub struct ProcessOutcome {
    pub end: ProcessEnd,
    description: String,
}

impl ProcessOutcome {
    /// Translate native wait status at the platform boundary.
    pub fn from_native(status: ExitStatus) -> Self {
        Self {
            end: match status.code() {
                Some(code) => ProcessEnd::Exited(code),
                None => ProcessEnd::Signaled(exit_signal(&status)),
            },
            description: status.to_string(),
        }
    }

    pub fn code(&self) -> Option<i32> {
        match self.end {
            ProcessEnd::Exited(code) => Some(code),
            ProcessEnd::Signaled(_) => None,
        }
    }

    pub fn success(&self) -> bool {
        self.code() == Some(0)
    }
}

impl std::fmt::Display for ProcessOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.description)
    }
}

#[cfg(unix)]
fn exit_signal(status: &ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    status.signal()
}

#[cfg(windows)]
fn exit_signal(_status: &ExitStatus) -> Option<i32> {
    // Windows processes never end with a signal.
    None
}
