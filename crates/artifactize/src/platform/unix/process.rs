//! Process groups, the pre-exec admission gate, and OS process start times.

use std::{
    io::{self, Read, Write},
    net::Shutdown,
    os::{fd::AsRawFd, unix::net::UnixStream},
    process::ExitStatus,
    sync::Arc,
};

use process_wrap::tokio::{ChildWrapper, CommandWrap, ProcessGroup};
use tokio::{
    io::unix::AsyncFd,
    process::{ChildStderr, ChildStdin, ChildStdout, Command},
    task::JoinHandle,
};

/// The child's process group, killed when dropped.
pub(crate) struct Child(Box<dyn ChildWrapper>, #[cfg(target_os = "macos")] u32);

impl Child {
    pub fn stdin(&mut self) -> &mut Option<ChildStdin> {
        self.0.stdin()
    }

    pub fn stdout(&mut self) -> &mut Option<ChildStdout> {
        self.0.stdout()
    }

    pub fn stderr(&mut self) -> &mut Option<ChildStderr> {
        self.0.stderr()
    }

    /// Wait for the leader, then reap the rest of its group.
    pub async fn wait(&mut self) -> io::Result<ExitStatus> {
        self.0.wait().await
    }

    /// Ask the group to stop with SIGTERM.
    pub fn interrupt(&self) -> io::Result<()> {
        self.0.signal(libc::SIGTERM)
    }

    /// SIGKILL the group; a group that is already gone is not an error.
    pub fn kill(&mut self) -> io::Result<()> {
        match self.0.start_kill() {
            Err(error) if error.raw_os_error() == Some(libc::ESRCH) => Ok(()),
            #[cfg(target_os = "macos")]
            Err(error) if error.raw_os_error() == Some(libc::EPERM) => {
                // Darwin can report EPERM while the last members are exiting or
                // zombies, before the group disappears. Suppress it only when the
                // kernel confirms no member can still execute user code.
                if process_group_exited(self.1)? {
                    Ok(())
                } else {
                    Err(error)
                }
            }
            result => result,
        }
    }
}

/// Darwin proc_info.h: a process committed to exit() can still report SRUN
/// before becoming SZOMB. It can no longer execute user code or fork descendants.
#[cfg(target_os = "macos")]
const PROC_FLAG_INEXIT: u32 = 4;

#[cfg(target_os = "macos")]
fn process_group_exited(pgid: u32) -> io::Result<bool> {
    let pgid = i32::try_from(pgid).map_err(|_| io::ErrorKind::InvalidInput)?;
    // libproc returns PID counts, not bytes, including spare capacity on a size query.
    // SAFETY: a null buffer asks for capacity and does not read or write records.
    let capacity = unsafe { libc::proc_listpgrppids(pgid, std::ptr::null_mut(), 0) };
    if capacity <= 0 {
        return Err(io::Error::last_os_error());
    }
    let bytes = capacity
        .checked_mul(std::mem::size_of::<i32>() as i32)
        .ok_or_else(|| io::Error::other("process group information size overflow"))?;
    // Bound kernel-reported allocations and reject a truncated list rather than
    // overlooking a live descendant. No signalling or enumeration retries.
    const MAX_GROUP_INFO_BYTES: i32 = 16 * 1024 * 1024;
    if bytes > MAX_GROUP_INFO_BYTES {
        return Err(io::Error::other("process group information exceeds 16 MiB"));
    }
    let mut pids = vec![0_i32; capacity as usize];
    // libproc also returns zero on error. Clear this thread's errno before the call
    // so an empty group is distinguishable from a failed enumeration.
    // SAFETY: __error returns this thread's writable errno; the aligned PID buffer
    // has the queried byte capacity. Both calls execute on the same thread.
    let (read, error) = unsafe {
        *libc::__error() = 0;
        let read = libc::proc_listpgrppids(pgid, pids.as_mut_ptr().cast(), bytes);
        (read, io::Error::last_os_error())
    };
    if read < 0 || (read == 0 && error.raw_os_error() != Some(0)) {
        return Err(error);
    }
    if read >= capacity {
        return Err(io::Error::other("incomplete process group information"));
    }
    for pid in pids.into_iter().take(read as usize) {
        // Include zombies explicitly. ESRCH means the listed member was reaped
        // during inspection; every other lookup error must remain an error.
        match process_info(pid, 1) {
            Ok(info)
                if info.pbi_pgid == pgid as u32
                    && info.pbi_status != libc::SZOMB
                    && info.pbi_flags & PROC_FLAG_INEXIT == 0 =>
            {
                return Ok(false);
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(true)
}

impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.start_kill();
    }
}

/// Spawn `command` as the leader of a new process group, held before exec until admitted.
pub(crate) fn spawn_gated(command: Command) -> io::Result<(Gate, JoinHandle<io::Result<Child>>)> {
    let mut command = CommandWrap::from(command);
    command.wrap(ProcessGroup::leader());
    spawn(command)
}

/// Spawn in a session of its own, for a desktop handoff that outlives artifactize.
pub(crate) fn spawn_detached(mut command: Command) -> io::Result<tokio::process::Child> {
    // SAFETY: the forked child calls only setsid, which is async-signal-safe.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command.spawn()
}

/// The index of `starttime` (field 22 of /proc/PID/stat, see proc(5)) among the fields after
/// the command name. The name, field 2, is cut off at its closing parenthesis because it may
/// contain spaces, so the remaining fields start at field 3: 22 - 3 = 19.
#[cfg(not(target_os = "macos"))]
const START_TIME_AFTER_NAME: usize = 22 - 3;

/// Field 22 of /proc/PID/stat: the start time in clock ticks since boot.
#[cfg(not(target_os = "macos"))]
pub(crate) fn process_start_time(pid: u32) -> io::Result<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
    stat.rsplit_once(')')
        .and_then(|(_, fields)| fields.split_whitespace().nth(START_TIME_AFTER_NAME))
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| io::Error::other("invalid child process start time"))
}

/// macOS exposes the kernel's process birth timestamp, in microseconds since the epoch.
/// Keep its full precision: seconds alone cannot distinguish rapidly reused PIDs.
#[cfg(target_os = "macos")]
pub(crate) fn process_start_time(pid: u32) -> io::Result<u64> {
    let pid = i32::try_from(pid).map_err(|_| io::ErrorKind::InvalidInput)?;
    if pid <= 0 {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let info = process_info(pid, 0)?;
    const MICROSECONDS_PER_SECOND: u64 = 1_000_000;
    info.pbi_start_tvsec
        .checked_mul(MICROSECONDS_PER_SECOND)
        .and_then(|seconds| seconds.checked_add(info.pbi_start_tvusec))
        .ok_or_else(|| io::Error::other("invalid child process start time"))
}

#[cfg(target_os = "macos")]
fn process_info(pid: i32, include_zombies: u64) -> io::Result<libc::proc_bsdinfo> {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::uninit();
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
    // SAFETY: a writable buffer of the exact size required by PROC_PIDTBSDINFO. The
    // initialized fields are read only if the kernel reports the complete struct.
    let read = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            include_zombies,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if read <= 0 {
        let error = io::Error::last_os_error();
        // proc_pidinfo reports ESRCH for a departed PID; liveness treats this like
        // Linux procfs ENOENT, not a supervision error.
        return Err(if error.raw_os_error() == Some(libc::ESRCH) {
            io::ErrorKind::NotFound.into()
        } else {
            error
        });
    }
    if read != size {
        return Err(io::Error::other("incomplete child process information"));
    }
    // SAFETY: proc_pidinfo wrote the entire struct.
    let info = unsafe { info.assume_init() };
    if info.pbi_pid != pid as u32 {
        return Err(io::Error::other("invalid child process identity"));
    }
    Ok(info)
}

/// The one byte the parent writes on the gate socket to admit the held child past
/// `pre_exec`. Its value carries no meaning beyond "admitted"; the handshake only checks
/// that this exact byte, not some other write, arrived.
const GATE_ADMIT: u8 = 1;

pub(crate) struct Gate(Arc<AsyncFd<UnixStream>>);

impl Gate {
    pub async fn pid(&self) -> io::Result<u32> {
        let mut bytes = [0; 4];
        let mut offset = 0;
        while offset < bytes.len() {
            let mut ready = self.0.readable().await?;
            match ready.try_io(|socket| socket.get_ref().read(&mut bytes[offset..])) {
                Ok(Ok(0)) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(Ok(count)) => offset += count,
                Ok(Err(error)) => return Err(error),
                Err(_) => continue,
            }
        }
        Ok(u32::from_ne_bytes(bytes))
    }

    pub fn admit(&self) -> io::Result<()> {
        self.0.get_ref().write_all(&[GATE_ADMIT])
    }
}

impl Drop for Gate {
    fn drop(&mut self) {
        // shutdown also disconnects the copy held alive by the spawning thread.
        let _ = self.0.get_ref().shutdown(Shutdown::Both);
    }
}

fn spawn(mut command: CommandWrap) -> io::Result<(Gate, JoinHandle<io::Result<Child>>)> {
    let (parent, child) = UnixStream::pair()?;
    parent.set_nonblocking(true)?;
    let gate = Gate(Arc::new(AsyncFd::new(parent)?));
    let parent = gate.0.clone();
    // SAFETY: the forked child uses only async-signal-safe syscalls and stack data.
    // Both descriptors stay alive until spawn completes; CLOEXEC closes the child
    // endpoint on exec. Closing the inherited parent endpoint makes EOF meaningful.
    unsafe {
        command.command_mut().pre_exec(move || {
            libc::close(parent.get_ref().as_raw_fd());
            let pid = (libc::getpid() as u32).to_ne_bytes();
            let fd = child.as_raw_fd();
            let mut offset = 0;
            while offset < pid.len() {
                let written = libc::write(fd, pid[offset..].as_ptr().cast(), pid.len() - offset);
                if written < 0 {
                    let error = io::Error::last_os_error();
                    if error.raw_os_error() == Some(libc::EINTR) {
                        continue;
                    }
                    return Err(error);
                }
                if written == 0 {
                    return Err(io::Error::from_raw_os_error(libc::EPIPE));
                }
                offset += written as usize;
            }
            let mut admission = 0_u8;
            loop {
                match libc::read(fd, (&mut admission as *mut u8).cast(), 1) {
                    1 if admission == GATE_ADMIT => return Ok(()),
                    -1 => {
                        let error = io::Error::last_os_error();
                        if error.raw_os_error() != Some(libc::EINTR) {
                            return Err(error);
                        }
                    }
                    _ => return Err(io::Error::from_raw_os_error(libc::ECANCELED)),
                }
            }
        });
    }
    // std spawn waits for exec's error pipe, so it cannot run on the task that
    // opens the gate. Returning a guarded child also covers a dropped join handle.
    let spawning = tokio::task::spawn_blocking(move || {
        command.spawn().map(|child| {
            #[cfg(target_os = "macos")]
            let pid = child.id().expect("new child has a PID");
            Child(
                child,
                #[cfg(target_os = "macos")]
                pid,
            )
        })
    });
    Ok((gate, spawning))
}

#[cfg(test)]
mod tests {
    use std::{process::Stdio, time::Duration};

    use process_wrap::tokio::ProcessGroup;

    use super::*;

    // Darwin killpg excludes zombies and returns EPERM for a zombie-only group.
    // Other Unix kernels already accept this signal; their cleanup tests cover it.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn killing_a_zombie_only_group_succeeds_without_reaping_it_first() {
        let mut command = Command::new(crate::test_os::bin("/bin/true"));
        command.env_clear().stdin(Stdio::null());
        let (gate, spawning) = spawn_gated(command).unwrap();
        let pid = gate.pid().await.unwrap();
        assert!(!process_group_exited(pid).unwrap());
        gate.admit().unwrap();
        drop(gate);
        let mut child = spawning.await.unwrap().unwrap();
        tokio::task::spawn_blocking(move || {
            let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::uninit();
            // SAFETY: waitid writes a full siginfo_t for our child; WNOWAIT
            // synchronizes on exit while deliberately keeping the zombie unreaped.
            let result = unsafe {
                libc::waitid(
                    libc::P_PID,
                    pid,
                    info.as_mut_ptr(),
                    libc::WEXITED | libc::WNOWAIT,
                )
            };
            assert_eq!(result, 0, "{}", io::Error::last_os_error());
        })
        .await
        .unwrap();
        assert!(process_group_exited(pid).unwrap());
        // Exercise the real EPERM path, not an injected error or a delay.
        assert_eq!(
            child.0.start_kill().unwrap_err().raw_os_error(),
            Some(libc::EPERM)
        );
        child.kill().unwrap();
        assert!(child.wait().await.unwrap().success());
        assert!(process_group_exited(pid).unwrap());
        child.kill().unwrap();
    }

    #[tokio::test]
    async fn parent_disconnect_before_admission_prevents_exec_and_reaps() {
        let mut command = CommandWrap::with_new(crate::test_os::bin("/bin/true"), |command| {
            command.env_clear().stdin(Stdio::null());
        });
        command.wrap(ProcessGroup::leader());
        let (gate, spawning) = spawn(command).unwrap();
        let pid = gate.pid().await.unwrap();
        assert!(process_start_time(pid).is_ok());
        drop(gate);
        let error = tokio::time::timeout(Duration::from_secs(2), spawning)
            .await
            .unwrap()
            .unwrap()
            .err()
            .expect("the child must not exec");
        assert_eq!(error.raw_os_error(), Some(libc::ECANCELED));
        assert!(process_start_time(pid).is_err());
    }
}
