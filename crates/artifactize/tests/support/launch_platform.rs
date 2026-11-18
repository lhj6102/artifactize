//! Observe the detach contract of the launch probe through the shared OS fixtures.

use crate::support::os;

#[cfg(unix)]
pub fn is_detached(pid: u32) -> bool {
    os::leads_group(pid) && os::leads_session(pid)
}

// A Windows handoff may stay in the enclosing Job if it forbids breakaway.
// The observable guarantee there is that the child outlives the launch host.
#[cfg(windows)]
pub fn is_detached(pid: u32) -> bool {
    os::running(pid)
}
