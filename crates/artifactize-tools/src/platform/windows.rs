//! Windows program lookup details and the ShellExecute desktop opener.

use std::{
    ffi::OsStr,
    io, mem,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
    ptr,
};

use windows_sys::Win32::{
    Foundation::INVALID_HANDLE_VALUE,
    System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    },
    System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_BASIC_LIMIT_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, SetInformationJobObject,
    },
    System::Threading::{CREATE_SUSPENDED, OpenThread, ResumeThread, THREAD_SUSPEND_RESUME},
    UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
};

/// An extensionless program name is tried with each `PATHEXT` suffix.
pub(crate) const USES_PATHEXT: bool = true;

/// Windows has no execute bit; any regular file is a candidate.
pub(crate) fn is_executable(path: &Path) -> bool {
    path.metadata().is_ok_and(|metadata| metadata.is_file())
}

/// Variables, besides `PATH` and the locale, that Windows programs need to start and to find
/// their home, data and temporary directories.
pub const SYSTEM_VARIABLES: &[&str] = &[
    "HOME",
    "SystemRoot",
    "ComSpec",
    "PATHEXT",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "TEMP",
    "TMP",
];

/// A started program in a kill-on-close Job Object, so that its children die with it.
pub(crate) struct Tree {
    pub(crate) child: tokio::process::Child,
    job: Option<OwnedHandle>,
}

/// Start `command` suspended, put it in a new kill-on-close job, then let it run: every
/// process it starts is in the job from its first instruction.
pub(crate) fn spawn_tree(command: &mut tokio::process::Command) -> io::Result<Tree> {
    // SAFETY: no attributes and no name create a private, non-inheritable job.
    let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreateJobObjectW returned a new handle that nothing else owns.
    let job = unsafe { OwnedHandle::from_raw_handle(handle) };
    let limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
        BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION {
            LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            ..Default::default()
        },
        ..Default::default()
    };
    // SAFETY: the information class matches the structure and its size.
    if unsafe {
        SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            ptr::from_ref(&limits).cast(),
            mem::size_of_val(&limits) as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    command.creation_flags(CREATE_SUSPENDED);
    let mut suspended = SuspendedChild(command.spawn()?);
    let admitted = (|| {
        let process = suspended.0.raw_handle().ok_or(io::ErrorKind::NotFound)?;
        // SAFETY: a live job handle and the handle of the suspended child just started.
        if unsafe { AssignProcessToJobObject(job.as_raw_handle(), process) } == 0 {
            return Err(io::Error::last_os_error());
        }
        resume(&suspended)
    })();
    if let Err(error) = admitted {
        // A child that never joined the job is killed directly; it never ran.
        let _ = suspended.0.start_kill();
        return Err(error);
    }
    Ok(Tree {
        child: suspended.0,
        job: Some(job),
    })
}

/// A child created with CREATE_SUSPENDED by this launcher. Owning the child
/// keeps its process identity live until job admission and resumption finish.
struct SuspendedChild(tokio::process::Child);

/// ResumeThread uses DWORD_MAX as its error sentinel, not as a suspend count.
const RESUME_THREAD_FAILED: u32 = u32::MAX;

/// Resume the primary thread of the child this launcher created suspended.
fn resume(child: &SuspendedChild) -> io::Result<()> {
    let pid = child.0.id().ok_or(io::ErrorKind::NotFound)?;
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
            if unsafe { ResumeThread(thread.as_raw_handle()) } == RESUME_THREAD_FAILED {
                return Err(io::Error::last_os_error());
            }
            resumed = true;
        }
        // SAFETY: as for Thread32First.
        more = unsafe { Thread32Next(snapshot.as_raw_handle(), &mut entry) } != 0;
    }
    if resumed {
        Ok(())
    } else {
        Err(io::Error::other(
            "the suspended child has no thread to resume",
        ))
    }
}

impl Tree {
    /// Close the job, which kills every process still in it.
    pub(crate) fn kill(&mut self) {
        self.job.take();
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Hand the target to ShellExecute's default verb on a blocking thread.
pub(crate) async fn open_desktop(target: &OsStr) -> io::Result<()> {
    let target = target.to_owned();
    match tokio::task::spawn_blocking(move || shell_execute(&target)).await {
        Ok(result) => result,
        Err(error) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
        Err(error) => Err(io::Error::other(error)),
    }
}

/// ShellExecuteW returns an error code at or below 32; greater values mean a successful
/// desktop handoff, not a handle that this process owns.
const SHELL_EXECUTE_ERROR_MAX: isize = 32;

fn shell_execute(target: &OsStr) -> io::Result<()> {
    let mut target: Vec<_> = target.encode_wide().collect();
    if target.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Open target contains NUL.",
        ));
    }
    target.push(0);
    // SAFETY: target is NUL-terminated and remains alive for the call. Null operation
    // selects the default verb; no parameters or directory are supplied.
    let result = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            ptr::null(),
            target.as_ptr(),
            ptr::null(),
            ptr::null(),
            SW_SHOWNORMAL,
        )
    } as isize;
    // ShellExecute returns a value greater than 32 on success, not an owned handle.
    if result <= SHELL_EXECUTE_ERROR_MAX {
        return Err(io::Error::other(format!(
            "ShellExecute could not open the target (code {result})."
        )));
    }
    Ok(())
}
