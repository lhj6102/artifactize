//! Process groups, the pre-exec admission gate, and OS process start times.

use std::{
    io::{self, Read, Write},
    net::Shutdown,
    os::{fd::AsRawFd, unix::net::UnixStream},
    process::ExitStatus,
    sync::{Arc, OnceLock, mpsc},
};

use process_wrap::tokio::{ChildWrapper, CommandWrap, ProcessGroup};
use tokio::{
    io::unix::AsyncFd,
    process::{ChildStderr, ChildStdin, ChildStdout, Command},
    task::JoinHandle,
};

/// The child's process group, killed when dropped.
pub(crate) struct Child {
    child: Box<dyn ChildWrapper>,
    group: Option<crate::process::ChildIdentity>,
}

impl Child {
    pub fn stdin(&mut self) -> &mut Option<ChildStdin> {
        self.child.stdin()
    }

    pub fn stdout(&mut self) -> &mut Option<ChildStdout> {
        self.child.stdout()
    }

    pub fn stderr(&mut self) -> &mut Option<ChildStderr> {
        self.child.stderr()
    }

    /// Wait for the leader, then reap the rest of its group.
    pub async fn wait(&mut self) -> io::Result<ExitStatus> {
        self.child.wait().await
    }

    /// Session liveness is the leader's, not its descendants'.
    pub async fn wait_leader(&mut self) -> io::Result<ExitStatus> {
        self.child.inner_mut().wait().await
    }

    pub fn try_wait_leader(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.inner_mut().try_wait()
    }

    /// Ask the group to stop with SIGTERM.
    pub fn interrupt(&self) -> io::Result<()> {
        if let Some(group) = self.group {
            signal_saved_group(group, libc::SIGTERM)
        } else {
            Ok(())
        }
    }

    /// SIGKILL the group; a group that is already gone is not an error.
    pub fn kill(&mut self) -> io::Result<()> {
        if let Some(group) = self.group {
            kill_saved_group(group)?;
            // No later wait, cleanup or Drop may signal this numeric PGID again.
            self.group = None;
        }
        Ok(())
    }

    pub async fn cleanup(&mut self) -> io::Result<ExitStatus> {
        self.kill()?;
        self.wait().await
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
        let _ = self.kill();
    }
}

/// Spawn `command` as the leader of a new process group, held before exec until admitted.
pub(crate) fn spawn_gated(command: Command) -> io::Result<(Gate, JoinHandle<io::Result<Child>>)> {
    let mut command = CommandWrap::from(command);
    command.wrap(ProcessGroup::leader());
    spawn(command)
}

/// Spawn `command` as the leader of a new process group, running at once. Without the gate's
/// `pre_exec` std starts it with `posix_spawn`, whose cost does not grow with this process's
/// memory as `fork`'s copy of its page tables does.
pub(crate) async fn spawn_now(command: Command) -> io::Result<Child> {
    let mut command = CommandWrap::from(command);
    command.wrap(ProcessGroup::leader());
    let child = command.spawn()?;
    // Unreaped until waited for, the leader keeps its /proc entry even if it already exited.
    let pid = child
        .id()
        .ok_or_else(|| io::Error::other("the child was reaped before its identity was read"))?;
    Ok(Child {
        group: Some(crate::process::child_identity(pid)?),
        child,
    })
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

pub(crate) fn spawn_run(command: Command) -> io::Result<tokio::process::Child> {
    spawn_detached(command)
}

/// A desktop handoff whose output pipes a Tool log drains. Like [`spawn_detached`] it gets
/// its own session; it also ignores SIGPIPE, so that once nobody reads the pipes any more (the
/// review's process exited) a write fails with EPIPE instead of killing the program.
pub(crate) fn spawn_watched(mut command: Command) -> io::Result<tokio::process::Child> {
    // SAFETY: the forked child calls only setsid and signal, which are async-signal-safe.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            if libc::signal(libc::SIGPIPE, libc::SIG_IGN) == libc::SIG_ERR {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command.spawn()
}

/// Unix programs open no console window; a host probe runs as it is.
pub(crate) fn hide_console_window(_command: &mut std::process::Command) {}

/// The index of `starttime` (field 22 of /proc/PID/stat, see proc(5)) among the fields after
/// the command name. The name, field 2, is cut off at its closing parenthesis because it may
/// contain spaces, so the remaining fields start at field 3: 22 - 3 = 19.
#[cfg(not(target_os = "macos"))]
const START_TIME_AFTER_NAME: usize = 22 - 3;

/// Field 22 of /proc/PID/stat: the start time in clock ticks since boot.
#[cfg(not(target_os = "macos"))]
pub(crate) fn process_start_time(pid: u32) -> io::Result<u64> {
    process_stat(pid).map(|(_, start)| start)
}

#[cfg(not(target_os = "macos"))]
fn process_stat(pid: u32) -> io::Result<(char, u64)> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let mut fields = stat
        .rsplit_once(')')
        .ok_or_else(|| io::Error::other("invalid process stat"))?
        .1
        .split_whitespace();
    let state = fields
        .next()
        .and_then(|field| field.chars().next())
        .ok_or_else(|| io::Error::other("invalid process state"))?;
    let start = fields
        .nth(START_TIME_AFTER_NAME - 1)
        .and_then(|field| field.parse().ok())
        .ok_or_else(|| io::Error::other("invalid child process start time"))?;
    Ok((state, start))
}

/// An unreaped zombie still has an identity, but cannot own live execution.
#[cfg(not(target_os = "macos"))]
pub(crate) fn running_process_start_time(pid: u32) -> io::Result<u64> {
    let (state, start) = process_stat(pid)?;
    if matches!(state, 'Z' | 'X' | 'x') {
        Err(io::ErrorKind::NotFound.into())
    } else {
        Ok(start)
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn running_process_start_time(pid: u32) -> io::Result<u64> {
    let pid = i32::try_from(pid).map_err(|_| io::ErrorKind::InvalidInput)?;
    let info = process_info(pid, 1)?;
    if info.pbi_status == libc::SZOMB || info.pbi_flags & PROC_FLAG_INEXIT != 0 {
        Err(io::ErrorKind::NotFound.into())
    } else {
        process_birth_time(info)
    }
}

/// macOS exposes the kernel's process birth timestamp, in microseconds since the epoch.
/// Keep its full precision: seconds alone cannot distinguish rapidly reused PIDs.
#[cfg(target_os = "macos")]
pub(crate) fn process_start_time(pid: u32) -> io::Result<u64> {
    let pid = i32::try_from(pid).map_err(|_| io::ErrorKind::InvalidInput)?;
    if pid <= 0 {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    process_birth_time(process_info(pid, 1)?)
}

#[cfg(target_os = "macos")]
fn process_birth_time(info: libc::proc_bsdinfo) -> io::Result<u64> {
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

pub(crate) struct Gate {
    socket: Arc<AsyncFd<UnixStream>>,
    identity: OnceLock<crate::process::ChildIdentity>,
    admitted: mpsc::Sender<crate::process::ChildIdentity>,
}

impl Gate {
    pub async fn pid(&self) -> io::Result<u32> {
        let mut bytes = [0; 4];
        let mut offset = 0;
        while offset < bytes.len() {
            let mut ready = self.socket.readable().await?;
            match ready.try_io(|socket| socket.get_ref().read(&mut bytes[offset..])) {
                Ok(Ok(0)) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(Ok(count)) => offset += count,
                Ok(Err(error)) => return Err(error),
                Err(_) => continue,
            }
        }
        let pid = u32::from_ne_bytes(bytes);
        let identity = crate::process::child_identity(pid)?;
        let _ = self.identity.set(identity);
        Ok(pid)
    }

    pub fn admit(&self) -> io::Result<()> {
        let identity = self.identity.get().ok_or(io::ErrorKind::InvalidInput)?;
        self.socket.get_ref().write_all(&[GATE_ADMIT])?;
        // A native spawn is not proof of admission: a pre_exec child killed by
        // SIGPIPE also closes std's exec-error pipe without reporting an errno.
        // Publish the saved, inert identity only after opening the gate; the
        // spawning thread waits for this handoff or for Gate drop to cancel it.
        self.admitted
            .send(*identity)
            .map_err(|_| io::ErrorKind::BrokenPipe.into())
    }
}

impl Drop for Gate {
    fn drop(&mut self) {
        // shutdown also disconnects the copy held alive by the spawning thread.
        let _ = self.socket.get_ref().shutdown(Shutdown::Both);
    }
}

fn spawn(mut command: CommandWrap) -> io::Result<(Gate, JoinHandle<io::Result<Child>>)> {
    let (parent, child) = UnixStream::pair()?;
    parent.set_nonblocking(true)?;
    let (admitted, admission) = mpsc::channel();
    let gate = Gate {
        socket: Arc::new(AsyncFd::new(parent)?),
        identity: OnceLock::new(),
        admitted,
    };
    let parent = gate.socket.clone();
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
        let spawned = command.spawn();
        // Close the parent's captured child endpoint before awaiting admission.
        // If pre_exec died without reporting an errno, Gate::pid must see EOF
        // rather than wait on this copy of the socket for its own cancellation.
        drop(command);
        let mut child = spawned?;
        match admission.recv() {
            Ok(group) => Ok(Child {
                child,
                group: Some(group),
            }),
            Err(_) => {
                // With no admission byte, a successful native spawn means the
                // pre_exec child died and closed the exec-error pipe. It never
                // executed user code or forked descendants. Reap its owned handle,
                // without inventing an identity or signalling a numeric PGID.
                tokio::runtime::Handle::current().block_on(child.inner_mut().wait())?;
                Err(io::Error::from_raw_os_error(libc::ECANCELED))
            }
        }
    });
    Ok((gate, spawning))
}

/// Detached Run workers own a session, including preparation commands that have no request
/// row yet. Recover all its groups, not just the worker's group. The saved start identity
/// excludes a reused session leader; group IDs remain reserved while members refer to them.
pub(crate) fn kill_saved_session(leader: crate::process::ChildIdentity) -> io::Result<()> {
    match process_start_time(leader.pid.get()) {
        Ok(start) if start != leader.start_time => return Ok(()),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let session = i32::try_from(leader.pid.get()).map_err(|_| io::ErrorKind::InvalidInput)?;
    let mut groups = std::collections::BTreeMap::new();
    for pid in process_ids()? {
        if pid <= 0 {
            continue;
        }
        let Ok(member_start) = process_start_time(pid as u32) else {
            continue;
        };
        // SAFETY: these read-only queries take a PID; vanished members return -1.
        let (sid, group) = unsafe { (libc::getsid(pid), libc::getpgid(pid)) };
        if sid != session || group <= 0 {
            continue;
        }
        let start = match process_start_time(group as u32) {
            Ok(start) => start,
            // An orphaned group has no leader, but its members still reserve its ID.
            Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
            Err(error) => return Err(error),
        };
        if process_start_time(pid as u32).ok() == Some(member_start) {
            groups.insert(group, start);
        }
    }
    for (group, start_time) in groups {
        if process_start_time(leader.pid.get()).is_ok_and(|start| start != leader.start_time) {
            return Ok(());
        }
        // Reuse identity checks and Darwin's zombie-only group handling too.
        kill_saved_group(crate::process::ChildIdentity {
            pid: crate::process::ProcessId::new(group as u32),
            start_time,
        })?;
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn process_ids() -> io::Result<Vec<i32>> {
    Ok(std::fs::read_dir("/proc")?
        .filter_map(|entry| entry.ok()?.file_name().to_str()?.parse().ok())
        .collect())
}
#[cfg(target_os = "macos")]
fn process_ids() -> io::Result<Vec<i32>> {
    // Bound the same kernel enumeration cost as the existing group-cleanup query.
    const MAX_PID_BYTES: i32 = 16 * 1024 * 1024;
    // SAFETY: a null buffer asks libproc for the PID count without writing records.
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if count <= 0 {
        return Err(io::Error::last_os_error());
    }
    let bytes = count
        .checked_mul(std::mem::size_of::<i32>() as i32)
        .ok_or_else(|| io::Error::other("process enumeration size overflow"))?;
    if bytes > MAX_PID_BYTES {
        return Err(io::Error::other("process enumeration exceeds 16 MiB"));
    }
    let mut pids = vec![0; count as usize];
    // SAFETY: the aligned PID buffer has the queried byte capacity; truncation is rejected.
    let read = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
    if read < 0 {
        return Err(io::Error::last_os_error());
    }
    if read >= count {
        return Err(io::Error::other("incomplete process enumeration"));
    }
    pids.truncate(read as usize);
    Ok(pids)
}

/// Recover only the saved group, never a reused leader PID. An absent leader can
/// still have live descendants; Darwin's zombie-only EPERM follows ordinary cleanup.
pub(crate) fn kill_saved_group(leader: crate::process::ChildIdentity) -> io::Result<()> {
    signal_saved_group(leader, libc::SIGKILL)
}

fn signal_saved_group(leader: crate::process::ChildIdentity, signal: i32) -> io::Result<()> {
    match process_start_time(leader.pid.get()) {
        Ok(start) if start != leader.start_time => return Ok(()),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let pgid = i32::try_from(leader.pid.get()).map_err(|_| io::ErrorKind::InvalidInput)?;
    // SAFETY: the nonzero saved PID names a group owned by the dead artifactize owner;
    // the preceding identity check excludes a known reused leader.
    if signal_group(pgid, signal) == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    if error.raw_os_error() == Some(libc::EPERM) && process_group_exited(leader.pid.get())? {
        return Ok(());
    }
    Err(error)
}

// Observe actual OS-boundary signal attempts, not private Child fields. The
// test catches an extra syscall even when the kernel answers ESRCH successfully.
#[cfg(test)]
thread_local! { static GROUP_SIGNALS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

fn signal_group(pgid: i32, signal: i32) -> i32 {
    #[cfg(test)]
    GROUP_SIGNALS.with(|count| count.set(count.get() + 1));
    // SAFETY: the caller identity-checked a nonzero, owned group before signalling.
    unsafe { libc::killpg(pgid, signal) }
}

#[cfg(test)]
mod tests {
    use std::{process::Stdio, time::Duration};

    use process_wrap::tokio::ProcessGroup;

    use super::*;
    use crate::test_os::BeforeIdentity;

    // Darwin killpg excludes zombies and returns EPERM for a zombie-only group.
    // Other Unix kernels already accept this signal; their cleanup tests cover it.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn killing_a_zombie_only_group_succeeds_without_reaping_it_first() {
        let mut command = Command::new(crate::test_os::true_program());
        command.env_clear().stdin(Stdio::null());
        let (gate, spawning) = spawn_gated(command).unwrap();
        let pid = gate.pid().await.unwrap();
        assert!(!process_group_exited(pid).unwrap());
        gate.admit().unwrap();
        drop(gate);
        let mut child = spawning.await.unwrap().unwrap();
        tokio::task::spawn_blocking(move || crate::test_os::wait_for_zombie(pid))
            .await
            .unwrap();
        assert!(process_group_exited(pid).unwrap());
        // Exercise the real EPERM path, not an injected error or a delay.
        assert_eq!(
            child.child.start_kill().unwrap_err().raw_os_error(),
            Some(libc::EPERM)
        );
        child.kill().unwrap();
        assert!(child.wait().await.unwrap().success());
        assert!(process_group_exited(pid).unwrap());
        child.kill().unwrap();
    }

    #[tokio::test]
    async fn a_cleaned_group_no_longer_owns_a_signal_target() {
        let mut command = Command::new(crate::test_os::true_program());
        command.env_clear().stdin(Stdio::null());
        let (gate, spawning) = spawn_gated(command).unwrap();
        gate.pid().await.unwrap();
        gate.admit().unwrap();
        drop(gate);
        let mut child = spawning.await.unwrap().unwrap();
        assert!(child.wait_leader().await.unwrap().success());
        GROUP_SIGNALS.with(|count| count.set(0));
        child.cleanup().await.unwrap();
        assert_eq!(GROUP_SIGNALS.with(std::cell::Cell::get), 1);
        child.interrupt().unwrap();
        child.cleanup().await.unwrap();
        drop(child);
        assert_eq!(
            GROUP_SIGNALS.with(std::cell::Cell::get),
            1,
            "a cleaned group must never be signalled again"
        );
    }

    #[tokio::test]
    async fn disconnect_before_identity_publication_cancels_and_reaps_native_child() {
        let mut command = Command::new(crate::test_os::true_program());
        command.env_clear().stdin(Stdio::null());
        let mut checkpoint = BeforeIdentity::hold(&mut command);
        let (gate, spawning) = spawn_gated(command).unwrap();
        let pid = checkpoint.pid().await;
        // The native child exists, but it cannot publish its PID to the gate until
        // released. Its PID write now meets a disconnected socket and SIGPIPE.
        drop(gate);
        checkpoint.release().await;
        let error = spawning
            .await
            .expect("pre-admission cancellation must not panic")
            .err()
            .expect("an unadmitted native child must not be returned");
        assert_eq!(error.raw_os_error(), Some(libc::ECANCELED));
        assert!(crate::test_os::gone(pid), "the native child must be reaped");
    }

    #[tokio::test]
    async fn native_death_before_identity_publication_closes_pid_observation_and_reaps() {
        let mut command = Command::new(crate::test_os::true_program());
        command.env_clear().stdin(Stdio::null());
        let mut checkpoint = BeforeIdentity::hold(&mut command);
        let (gate, spawning) = spawn_gated(command).unwrap();
        let pid = checkpoint.pid().await;
        crate::test_os::kill(pid);
        // The parent's captured child endpoint must be closed after native spawn
        // returns, or PID observation would never learn that this child died.
        let error = gate.pid().await.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
        drop(gate);
        let error = spawning
            .await
            .expect("pre-admission death must not panic")
            .err()
            .expect("an unadmitted native child must not be returned");
        assert_eq!(error.raw_os_error(), Some(libc::ECANCELED));
        assert!(crate::test_os::gone(pid), "the native child must be reaped");
    }

    #[tokio::test]
    async fn parent_disconnect_before_admission_prevents_exec_and_reaps() {
        let mut command = CommandWrap::with_new(crate::test_os::true_program(), |command| {
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
