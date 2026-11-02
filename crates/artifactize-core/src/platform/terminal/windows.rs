use super::Foreground;
use portable_pty::{Child, MasterPty};
use std::io;

// Keep the PTY child and descendants in a kill-on-close Job.
pub(crate) use crate::platform::windows::process::TerminalJob as Processes;

pub(crate) fn foreground(_master: &dyn MasterPty, _shell_pid: Option<u32>) -> Option<Foreground> {
    // ConPTY exposes no foreground group. OSC 7 supplies cwd; command stays unknown.
    None
}

impl Processes {
    pub fn new(child: &dyn Child) -> io::Result<Self> {
        Self::attach(child)
    }
}

pub(crate) const STOP_BEFORE_REMOVE: bool = true;

pub(crate) fn wait_host(host: crate::process::ChildIdentity) -> io::Result<()> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::{
        Foundation::{ERROR_INVALID_PARAMETER, FILETIME, WAIT_OBJECT_0},
        System::Threading::{
            GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
            WaitForSingleObject,
        },
    };
    // Library callers can serve a terminal in their own process. Its completed service and
    // released election lock are sufficient; waiting for one's own process would deadlock.
    if host.pid.get() == std::process::id() {
        return Ok(());
    }
    // SAFETY: a read/synchronize-only process handle, checked below and then owned.
    let process = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            host.pid.get(),
        )
    };
    if process.is_null() {
        let error = io::Error::last_os_error();
        return if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
            Ok(())
        } else {
            Err(error)
        };
    }
    // SAFETY: OpenProcess returned a new handle owned only here.
    let process = unsafe { OwnedHandle::from_raw_handle(process) };
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: the handle is open and all time outputs are writable structures.
    if unsafe {
        GetProcessTimes(
            process.as_raw_handle(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let start = (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime);
    if start != host.start_time {
        return Ok(());
    }
    // A stopped host only needs to release its runtime and process handles before removal.
    const HOST_EXIT_TIMEOUT_MS: u32 = 30_000;
    // SAFETY: the synchronize-capable handle remains open throughout this bounded wait.
    if unsafe { WaitForSingleObject(process.as_raw_handle(), HOST_EXIT_TIMEOUT_MS) }
        == WAIT_OBJECT_0
    {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "Terminal host did not exit before worktree removal",
        ))
    }
}
