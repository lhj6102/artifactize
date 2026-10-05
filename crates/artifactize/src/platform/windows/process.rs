//! Job Objects: a child starts suspended in a new console process group, joins a
//! kill-on-close job, and its primary thread resumes only when the gate admits it.

use std::{
    io, mem,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    process::ExitStatus,
    ptr,
    sync::atomic::{AtomicBool, Ordering},
};

use tokio::{
    process::{ChildStderr, ChildStdin, ChildStdout, Command},
    sync::oneshot,
    task::JoinHandle,
};
use windows_sys::Win32::{
    Foundation::{
        ERROR_ACCESS_DENIED, ERROR_CANCELLED, ERROR_INVALID_PARAMETER, FILETIME,
        INVALID_HANDLE_VALUE, STILL_ACTIVE,
    },
    System::{
        Console::{CTRL_BREAK_EVENT, GenerateConsoleCtrlEvent},
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
        },
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_BASIC_LIMIT_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
        },
        Threading::{
            CREATE_NEW_PROCESS_GROUP, CREATE_SUSPENDED, DETACHED_PROCESS, GetExitCodeProcess,
            GetProcessTimes, OpenProcess, OpenThread, PROCESS_QUERY_LIMITED_INFORMATION,
            ResumeThread, THREAD_SUSPEND_RESUME,
        },
    },
};

/// The child and its job; every process in the job is killed when this drops.
pub(crate) struct Child {
    child: tokio::process::Child,
    job: Job,
}

impl Child {
    pub fn stdin(&mut self) -> &mut Option<ChildStdin> {
        &mut self.child.stdin
    }

    pub fn stdout(&mut self) -> &mut Option<ChildStdout> {
        &mut self.child.stdout
    }

    pub fn stderr(&mut self) -> &mut Option<ChildStderr> {
        &mut self.child.stderr
    }

    pub async fn wait(&mut self) -> io::Result<ExitStatus> {
        self.child.wait().await
    }

    /// Ask the child's console process group to stop with Ctrl-Break, the closest to SIGTERM.
    /// This fails when the child shares no console with artifactize; the job is killed after
    /// the grace period either way.
    pub fn interrupt(&self) -> io::Result<()> {
        // Group 0 would mean every process on this console, artifactize included.
        let Some(group) = self.child.id().filter(|&pid| pid != 0) else {
            return Ok(());
        };
        // SAFETY: a nonzero group id reaches only the child's own process group.
        if unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, group) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Terminate every process in the job.
    pub fn kill(&mut self) -> io::Result<()> {
        self.job.terminate()
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.job.terminate();
    }
}

/// A job whose processes all die when its last handle closes, artifactize's included.
struct Job(OwnedHandle);

impl Job {
    fn new() -> io::Result<Self> {
        // SAFETY: no attributes and no name create a private, non-inheritable job.
        let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: CreateJobObjectW returned a new handle that nothing else owns.
        let job = Self(unsafe { OwnedHandle::from_raw_handle(handle) });
        let limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
            BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION {
                LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                ..Default::default()
            },
            ..Default::default()
        };
        // SAFETY: the information class matches the structure and its size.
        let ok = unsafe {
            SetInformationJobObject(
                job.0.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                mem::size_of_val(&limits) as u32,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }

    fn assign(&self, child: &tokio::process::Child) -> io::Result<()> {
        let process = child
            .raw_handle()
            .ok_or_else(|| io::Error::other("the child was reaped before joining its job"))?;
        // SAFETY: both handles are open.
        if unsafe { AssignProcessToJobObject(self.0.as_raw_handle(), process) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn terminate(&self) -> io::Result<()> {
        // SAFETY: the job handle is open.
        if unsafe { TerminateJobObject(self.0.as_raw_handle(), 1) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

/// Holds a suspended child until admitted. Dropping it unadmitted kills the child before it
/// runs any code.
pub(crate) struct Gate {
    pid: Option<u32>,
    admitted: AtomicBool,
    outcome: Option<oneshot::Sender<bool>>,
}

impl Gate {
    pub async fn pid(&self) -> io::Result<u32> {
        self.pid.ok_or_else(|| io::ErrorKind::UnexpectedEof.into())
    }

    /// Resume the child's primary thread: only now does it start to run.
    pub fn admit(&self) -> io::Result<()> {
        resume(self.pid.ok_or(io::ErrorKind::UnexpectedEof)?)?;
        self.admitted.store(true, Ordering::Release);
        Ok(())
    }
}

impl Drop for Gate {
    fn drop(&mut self) {
        if let Some(outcome) = self.outcome.take() {
            let _ = outcome.send(*self.admitted.get_mut());
        }
    }
}

/// Spawn `command` suspended, in a new console process group and a kill-on-close job. The
/// task yields the child once the gate admits it, or kills it if the gate drops first.
pub(crate) fn spawn_gated(
    mut command: Command,
) -> io::Result<(Gate, JoinHandle<io::Result<Child>>)> {
    let job = Job::new()?;
    command.creation_flags(CREATE_SUSPENDED | CREATE_NEW_PROCESS_GROUP);
    let child = command
        .spawn()
        .and_then(|mut child| match job.assign(&child) {
            Ok(()) => Ok(child),
            Err(error) => {
                // Still suspended, so nothing ran; kill it whatever its kill_on_drop says.
                let _ = child.start_kill();
                Err(error)
            }
        });
    let (outcome, admission) = oneshot::channel();
    let gate = Gate {
        pid: child.as_ref().ok().and_then(tokio::process::Child::id),
        admitted: AtomicBool::new(false),
        outcome: Some(outcome),
    };
    let spawning = tokio::spawn(async move {
        let mut child = child?;
        if !matches!(admission.await, Ok(true)) {
            let _ = job.terminate();
            let _ = child.wait().await;
            return Err(io::Error::from_raw_os_error(ERROR_CANCELLED as i32));
        }
        Ok(Child { child, job })
    });
    Ok((gate, spawning))
}

/// Resume every thread of a process created suspended; it has exactly one. Its handle is
/// held by the spawning task, so the PID cannot be reused meanwhile.
fn resume(pid: u32) -> io::Result<()> {
    // SAFETY: a snapshot of all threads; the result is checked and then owned.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreateToolhelp32Snapshot returned a new handle that nothing else owns.
    let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot) };
    let mut entry = THREADENTRY32 {
        dwSize: mem::size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    let mut resumed = false;
    // SAFETY: an open snapshot and an entry whose dwSize is set.
    let mut more = unsafe { Thread32First(snapshot.as_raw_handle(), &mut entry) } != 0;
    while more {
        if entry.th32OwnerProcessID == pid {
            // SAFETY: the result is checked and then owned.
            let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            if thread.is_null() {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: OpenThread returned a new handle that nothing else owns.
            let thread = unsafe { OwnedHandle::from_raw_handle(thread) };
            // SAFETY: an open thread handle with THREAD_SUSPEND_RESUME.
            if unsafe { ResumeThread(thread.as_raw_handle()) } == u32::MAX {
                return Err(io::Error::last_os_error());
            }
            resumed = true;
        }
        // SAFETY: as for Thread32First.
        more = unsafe { Thread32Next(snapshot.as_raw_handle(), &mut entry) } != 0;
    }
    if !resumed {
        return Err(io::Error::other(
            "the suspended child has no thread to resume",
        ));
    }
    Ok(())
}

/// Leave artifactize's console and process group, for a desktop handoff that outlives it.
pub(crate) fn detach(command: &mut Command) {
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
}

/// The creation time, in 100 ns intervals since 1601. A process that has exited counts as
/// not found, as does one this user may not query: artifactize records only its own
/// processes and children, so such a PID was reused by another user's or a system process.
pub(crate) fn process_start_time(pid: u32) -> io::Result<u64> {
    // SAFETY: the result is checked and then owned.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        let error = io::Error::last_os_error();
        return Err(match error.raw_os_error().map(|code| code as u32) {
            Some(ERROR_INVALID_PARAMETER | ERROR_ACCESS_DENIED) => io::ErrorKind::NotFound.into(),
            _ => error,
        });
    }
    // SAFETY: OpenProcess returned a new handle that nothing else owns.
    let process = unsafe { OwnedHandle::from_raw_handle(process) };
    let mut code = 0;
    // SAFETY: an open process handle and a valid out pointer.
    if unsafe { GetExitCodeProcess(process.as_raw_handle(), &mut code) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // A process that exits with code 259 itself is indistinguishable and counts as running.
    if code != STILL_ACTIVE as u32 {
        return Err(io::ErrorKind::NotFound.into());
    }
    let mut creation = FILETIME::default();
    let (mut exit, mut kernel, mut user) = (creation, creation, creation);
    // SAFETY: an open process handle and valid out pointers.
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
    Ok((u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime))
}
