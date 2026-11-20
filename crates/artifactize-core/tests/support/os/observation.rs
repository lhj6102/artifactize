//! Read-only OS process observations shared by fixture barriers and OS test helpers.
/// Whether a process has not exited. A PID that no longer names a process, or names one
/// this user may not open, counts as exited.
#[cfg(windows)]
pub fn running(pid: u32) -> bool {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, WAIT_TIMEOUT},
        System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
    };
    // SAFETY: the handle is checked, queried once without waiting, and closed.
    unsafe {
        let process = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if process.is_null() {
            return false;
        }
        // A signalled process has exited even if its actual exit code is 259
        // (STILL_ACTIVE); the exit-code value alone cannot establish liveness.
        let active = WaitForSingleObject(process, 0) == WAIT_TIMEOUT;
        CloseHandle(process);
        active
    }
}

/// A process entry still present, including a zombie waiting for its parent to reap it.
#[cfg(unix)]
pub fn exists(pid: u32) -> bool {
    #[cfg(not(target_os = "macos"))]
    {
        std::path::Path::new(&format!("/proc/{pid}")).exists()
    }
    #[cfg(target_os = "macos")]
    {
        bsd_info(pid).is_some()
    }
}

/// Whether a Unix process has not exited; zombies are present but no longer running.
#[cfg(unix)]
pub fn running(pid: u32) -> bool {
    #[cfg(not(target_os = "macos"))]
    {
        std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
            stat.rsplit_once(')')
                .and_then(|(_, fields)| fields.split_whitespace().next())
                .is_some_and(|state| !matches!(state, "Z" | "X" | "x"))
        })
    }
    #[cfg(target_os = "macos")]
    {
        bsd_info(pid).is_some_and(|info| {
            info.pbi_status != libc::SZOMB && info.pbi_flags & PROC_FLAG_INEXIT == 0
        })
    }
}

// Darwin proc_info.h: an INEXIT process cannot execute user code even before SZOMB.
#[cfg(target_os = "macos")]
const PROC_FLAG_INEXIT: u32 = 4;

#[cfg(target_os = "macos")]
fn bsd_info(pid: u32) -> Option<libc::proc_bsdinfo> {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::uninit();
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
    // SAFETY: the writable output buffer has the exact size required by PROC_PIDTBSDINFO.
    let read = unsafe {
        libc::proc_pidinfo(
            i32::try_from(pid).ok()?,
            libc::PROC_PIDTBSDINFO,
            1,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    // SAFETY: only a complete successful result is read.
    (read == size).then(|| unsafe { info.assume_init() })
}

/// A process's start time as artifactize records it: field 22 of `/proc/PID/stat` on Linux,
/// microseconds since the epoch on macOS, and the creation time in 100 ns units since 1601 on Windows.
pub fn start_time(pid: u32) -> u64 {
    observed_start_time(pid).expect("the owned process has a start time")
}

/// A read-only lifetime check of a saved identity, not a reconciliation or cleanup action.
/// A process disappearing between the liveness and birth-time reads is an ordinary exit.
pub fn same_process_running(pid: u32, start_time: u64) -> bool {
    running(pid) && observed_start_time(pid) == Some(start_time)
}

// After the parenthesized comm field, index 0 is stat field 3; birth time is field 22.
#[cfg(all(unix, not(target_os = "macos")))]
const START_TIME_INDEX: usize = 19;

/// Observe this PID's native parent, without assigning ownership or guarding PID reuse.
/// Callers that need a lifetime identity also capture and compare the process birth time.
pub fn parent_pid(pid: u32) -> Option<u32> {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // After the parenthesized comm field, index 0 is state and index 1 is parent PID.
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        stat.rsplit_once(')')?
            .1
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()
    }
    #[cfg(target_os = "macos")]
    {
        bsd_info(pid).map(|info| info.pbi_ppid)
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
            System::Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
        };
        // SAFETY: the snapshot is checked and closed on every path; the sized entry buffer
        // is writable for enumeration, and only the entry matching the requested PID is read.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return None;
            }
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            let mut found = Process32FirstW(snapshot, &mut entry);
            let parent = loop {
                if found == 0 {
                    break None;
                }
                if entry.th32ProcessID == pid {
                    break Some(entry.th32ParentProcessID);
                }
                found = Process32NextW(snapshot, &mut entry);
            };
            CloseHandle(snapshot);
            parent
        }
    }
}

/// Read a birth identity without panicking if an observed process has already exited.
pub fn observed_start_time(pid: u32) -> Option<u64> {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        stat.rsplit_once(')')?
            .1
            .split_whitespace()
            .nth(START_TIME_INDEX)?
            .parse()
            .ok()
    }
    #[cfg(target_os = "macos")]
    {
        let info = bsd_info(pid)?;
        Some(info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec)
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Foundation::{CloseHandle, FILETIME},
            System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
        };
        // SAFETY: the handle is checked, queried once, and closed on success or failure.
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return None;
            }
            let mut creation = FILETIME::default();
            let (mut exit, mut kernel, mut user) = (creation, creation, creation);
            let queried =
                GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user);
            CloseHandle(process);
            (queried != 0).then_some(
                (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime),
            )
        }
    }
}

/// Stop an observed fixture owner and wait for that exact birth identity to exit.
pub fn stop_owned(
    pid: u32,
    birth: u64,
    group: bool,
    bound: std::time::Duration,
) -> std::io::Result<()> {
    if !same_process_running(pid, birth) {
        return Ok(());
    }
    signal_owned(pid, birth, group)?;
    let deadline = std::time::Instant::now() + bound;
    // Poll a native read-only process observation; the interval is not a test assertion.
    const EXIT_POLL: std::time::Duration = std::time::Duration::from_millis(10);
    while same_process_running(pid, birth) {
        if std::time::Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("fixture process {pid} did not exit"),
            ));
        }
        std::thread::sleep(EXIT_POLL);
    }
    Ok(())
}

#[cfg(unix)]
fn signal_owned(pid: u32, birth: u64, group: bool) -> std::io::Result<()> {
    let id = i32::try_from(pid)
        .ok()
        .filter(|id| *id > 0)
        .ok_or(std::io::ErrorKind::InvalidInput)?;
    if !same_process_running(pid, birth) {
        return Ok(());
    }
    // SAFETY: The positive PID's birth identity was just rechecked. A negative PID
    // refers to the process group of an owned Eval child, never the caller's group.
    if unsafe { libc::kill(if group { -id } else { id }, libc::SIGKILL) } == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if !same_process_running(pid, birth) {
        Ok(())
    } else {
        Err(error)
    }
}

#[cfg(windows)]
fn signal_owned(pid: u32, birth: u64, _group: bool) -> std::io::Result<()> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::{
        Foundation::FILETIME,
        System::Threading::{
            GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
            TerminateProcess,
        },
    };
    // SAFETY: OpenProcess takes a PID, with no caller pointers. Only a valid, owned handle
    // is wrapped below and closed on return.
    let process = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE,
            0,
            pid,
        )
    };
    if process.is_null() {
        let error = std::io::Error::last_os_error();
        return if same_process_running(pid, birth) {
            Err(error)
        } else {
            Ok(())
        };
    }
    // SAFETY: the successful OpenProcess returned a unique owned handle.
    let process = unsafe { OwnedHandle::from_raw_handle(process) };
    let mut created = FILETIME::default();
    let (mut exit, mut kernel, mut user) = (created, created, created);
    // SAFETY: the owned handle is live, and all four out-pointers are valid.
    if unsafe {
        GetProcessTimes(
            process.as_raw_handle(),
            &mut created,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    if (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime) != birth {
        return Ok(());
    }
    // SAFETY: only the handle with the matching birth time is stopped. The executor's
    // Job Objects own its runtime descendants.
    if unsafe { TerminateProcess(process.as_raw_handle(), 1) } == 0
        && same_process_running(pid, birth)
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
