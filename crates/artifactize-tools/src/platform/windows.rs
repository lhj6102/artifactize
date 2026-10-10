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
    System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_BASIC_LIMIT_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, SetInformationJobObject,
    },
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

/// Start `command` and put it in a new kill-on-close job. A child it starts before joining
/// is not in the job; a help program starts none that early.
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
    let child = command.spawn()?;
    let process = child.raw_handle().ok_or(io::ErrorKind::NotFound)?;
    // SAFETY: a live job handle and the handle of the child just started.
    if unsafe { AssignProcessToJobObject(job.as_raw_handle(), process) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(Tree {
        child,
        job: Some(job),
    })
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
    if result <= 32 {
        return Err(io::Error::other(format!(
            "ShellExecute could not open the target (code {result})."
        )));
    }
    Ok(())
}
