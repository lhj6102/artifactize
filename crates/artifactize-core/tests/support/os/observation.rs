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
