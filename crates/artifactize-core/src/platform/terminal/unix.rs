use super::Foreground;
use crate::{process::ChildIdentity, terminal::TerminalCommand};
use portable_pty::{Child, MasterPty};
use std::{io, path::PathBuf};

pub(crate) struct Processes(ChildIdentity);
impl Processes {
    pub fn new(child: &dyn Child) -> io::Result<Self> {
        crate::process::child_identity(
            child
                .process_id()
                .ok_or_else(|| io::Error::other("PTY child has no PID"))?,
        )
        .map(Self)
    }
}
impl Processes {
    pub fn stop(&self) -> io::Result<()> {
        crate::platform::kill_saved_session(self.0)
    }
}
impl Drop for Processes {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// The line discipline a new PTY starts with. A program that is killed or exits without
/// restoring its terminal (a shell's line editor turns echo off while it waits) leaves its
/// settings behind; the next program on the same PTY starts from these again.
pub(crate) struct Modes(libc::termios);
impl Modes {
    pub fn initial(master: &dyn MasterPty) -> io::Result<Self> {
        let fd = master
            .as_raw_fd()
            .ok_or_else(|| io::Error::other("PTY master has no descriptor"))?;
        // SAFETY: a zeroed termios is a valid plain C structure that tcgetattr overwrites.
        let mut modes: libc::termios = unsafe { std::mem::zeroed() };
        // SAFETY: the live master owns fd and modes is writable for the call.
        if unsafe { libc::tcgetattr(fd, &mut modes) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(modes))
    }
    pub fn restore(&self, master: &dyn MasterPty) -> io::Result<()> {
        let fd = master
            .as_raw_fd()
            .ok_or_else(|| io::Error::other("PTY master has no descriptor"))?;
        // Apply at once: typed-ahead input stays queued for the next program.
        // SAFETY: the live master owns fd and the saved settings came from tcgetattr.
        if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &self.0) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

/// A detached host has its own session; nothing ends it with the program that started it.
pub(crate) fn ends_with_launcher() -> bool {
    false
}

/// macOS revokes a terminal, flushing its unread input, once the process leading it exits,
/// so each program needs a PTY of its own there. Linux keeps the PTY, and with it what was
/// typed ahead, for the next program.
pub(crate) const PTY_PER_PROGRAM: bool = cfg!(target_os = "macos");

pub(crate) const STOP_BEFORE_REMOVE: bool = false;
pub(crate) fn wait_host(host: ChildIdentity) -> io::Result<()> {
    use std::time::{Duration, Instant};
    // An in-process library host has already released its service lock; it cannot wait
    // for its own containing process. Detached CLI hosts must actually finish exiting.
    if host.pid.get() == std::process::id() {
        return Ok(());
    }
    // Match Windows's bounded process wait after native child cleanup and frame draining.
    const HOST_EXIT_TIMEOUT: Duration = Duration::from_secs(30);
    // A host need not be our child, so observe its identity instead of calling waitpid.
    const EXIT_POLL: Duration = Duration::from_millis(25);
    let deadline = Instant::now() + HOST_EXIT_TIMEOUT;
    while crate::process::is_alive(Some(host))? {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Terminal host did not exit",
            ));
        }
        std::thread::sleep(EXIT_POLL);
    }
    Ok(())
}

pub(crate) fn foreground(master: &dyn MasterPty, shell_pid: Option<u32>) -> Option<Foreground> {
    let fd = master.as_raw_fd()?;
    // SAFETY: the live master owns this descriptor; tcgetpgrp only reads terminal state.
    let pid = unsafe { libc::tcgetpgrp(fd) };
    if pid <= 0 {
        return None;
    }
    let (cwd, argv) = inspect(pid)?;
    let mut args = argv.into_iter();
    let command = args.next().map(|program| TerminalCommand {
        program,
        args: args.collect(),
    });
    let default = portable_pty::CommandBuilder::new_default_prog().get_shell();
    let shell_name = std::path::Path::new(&default).file_name()?.to_str()?;
    let shell = shell_pid == Some(pid as u32)
        && command.as_ref().is_some_and(|command| {
            std::path::Path::new(&command.program)
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.trim_start_matches('-') == shell_name)
        });
    Some(Foreground {
        cwd,
        command,
        shell,
    })
}

#[cfg(target_os = "linux")]
fn inspect(pid: i32) -> Option<(Option<PathBuf>, Vec<String>)> {
    let directory = PathBuf::from(format!("/proc/{pid}"));
    let cwd = std::fs::read_link(directory.join("cwd")).ok();
    let bytes = std::fs::read(directory.join("cmdline")).ok()?;
    // Non-Unicode argv cannot be offered as a faithful rerun in the JSON client.
    let argv = bytes
        .strip_suffix(&[0])?
        .split(|b| *b == 0)
        .map(|s| String::from_utf8(s.to_vec()))
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    Some((cwd, argv))
}

#[cfg(target_os = "macos")]
fn inspect(pid: i32) -> Option<(Option<PathBuf>, Vec<String>)> {
    use std::{ffi::CStr, mem};
    // SAFETY: this plain C output structure is initialized before proc_pidinfo fills it.
    let mut info: libc::proc_vnodepathinfo = unsafe { mem::zeroed() };
    // SAFETY: info is writable for the exact size supplied and the flavor matches its type.
    let read = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            (&raw mut info).cast(),
            mem::size_of_val(&info) as i32,
        )
    };
    let cwd = if read == mem::size_of_val(&info) as i32 {
        // SAFETY: proc_pidinfo fills a NUL-terminated MAXPATHLEN path. libc represents
        // that contiguous buffer as nested arrays; flatten all rows, not only the first.
        let bytes =
            unsafe { CStr::from_ptr(info.pvi_cdir.vip_path.as_flattened().as_ptr()) }.to_bytes();
        Some(crate::platform::path_from_bytes(bytes))
    } else {
        None
    };
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    let mut length = 0;
    // SAFETY: this first read only asks for the buffer size; no new value is supplied.
    if unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            std::ptr::null_mut(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    } != 0
    {
        return None;
    }
    let mut bytes = vec![0u8; length];
    // SAFETY: bytes owns length writable bytes, and the MIB requests the same process args.
    if unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            bytes.as_mut_ptr().cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    } != 0
    {
        return None;
    }
    bytes.truncate(length);
    let argc = i32::from_ne_bytes(bytes.get(..4)?.try_into().ok()?) as usize;
    let rest = bytes.get(4..)?;
    let executable_end = rest.iter().position(|b| *b == 0)?;
    let arguments = &rest[executable_end..];
    let start = arguments.iter().position(|b| *b != 0)?;
    let argv = arguments[start..]
        .split(|b| *b == 0)
        .take(argc)
        .map(|s| String::from_utf8(s.to_vec()))
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    Some((cwd, argv))
}
