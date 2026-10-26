//! The pipe ends a child held before exec gives back.
//!
//! A forked child holds a copy of every descriptor its parent had open at the fork, including
//! the pipes of runs that were starting at the same time. Close-on-exec drops those copies only
//! at exec, and a gated child waits for its admission before it execs, so until then another
//! run's output pipe could not reach its end. The child therefore closes, before it waits, each
//! inherited pipe end whose other end it holds as well: such a pair is another run's pipe that
//! this child never uses. A lone write end is kept, because std reports a failed exec through
//! one whose read end only the parent holds.

use std::{io, os::fd::RawFd};

/// Room for descriptors opened between [`Inherited::reserve`] and the fork, on top of twice
/// the count open at the reservation: the forked child cannot allocate, and a descriptor it has
/// no room for is only kept open until exec, as without this release.
const SPARE_DESCRIPTORS: usize = 64;

/// One end of a pipe this process holds: which end it is, and which end it pairs with.
#[derive(Clone, Copy)]
struct PipeEnd {
    fd: RawFd,
    end: Key,
    peer: Key,
}

/// Identifies one end of one pipe within this process: on Linux the pipe's inode and the end's
/// direction, on macOS the end's own handle.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Key(u64, u64);

/// Memory reserved in the parent for what the forked child finds.
pub(super) struct Inherited {
    ends: Vec<PipeEnd>,
    #[cfg(target_os = "macos")]
    listed: Vec<libc::proc_fdinfo>,
}

impl Inherited {
    /// Reserve room for the descriptors this process holds now and for some opened before the
    /// fork. Counting them is best effort: a failed count only leaves less room.
    pub(super) fn reserve() -> Self {
        let capacity = open_descriptors()
            .unwrap_or(0)
            .saturating_mul(2)
            .saturating_add(SPARE_DESCRIPTORS);
        Self {
            ends: Vec::with_capacity(capacity),
            #[cfg(target_os = "macos")]
            listed: Vec::with_capacity(capacity),
        }
    }

    /// In the forked child, before it waits for admission: close every inherited close-on-exec
    /// pipe end whose other end this child holds too, except `keep`.
    ///
    /// Only async-signal-safe system calls run here, on memory reserved before the fork.
    pub(super) fn release(&mut self, keep: RawFd) {
        self.ends.clear();
        self.collect(keep);
        self.ends.sort_unstable_by_key(|end| end.end);
        for end in &self.ends {
            if self
                .ends
                .binary_search_by_key(&end.peer, |other| other.end)
                .is_ok()
            {
                // SAFETY: `fd` was listed open in this single-threaded child and is not
                // `keep`; exec would close it anyway, as it is close-on-exec.
                unsafe { libc::close(end.fd) };
            }
        }
    }

    fn push(&mut self, end: PipeEnd) {
        // Pushing past the reserved capacity would allocate in the forked child.
        if self.ends.len() < self.ends.capacity() {
            self.ends.push(end);
        }
    }
}

/// Whether `fd` is one of artifactize's own: close-on-exec, above the standard streams.
fn inherited(fd: RawFd, keep: RawFd) -> bool {
    if fd <= libc::STDERR_FILENO || fd == keep {
        return false;
    }
    // SAFETY: F_GETFD only reads this process's descriptor flags.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    flags >= 0 && flags & libc::FD_CLOEXEC != 0
}

/// The descriptors open in this process.
#[cfg(not(target_os = "macos"))]
fn open_descriptors() -> io::Result<usize> {
    Ok(std::fs::read_dir("/proc/self/fd")?.count())
}

/// Linux `struct linux_dirent64`: `d_reclen` is a u16 at offset 16, and `d_name` starts at 19.
#[cfg(not(target_os = "macos"))]
const RECORD_LENGTH_OFFSET: usize = 16;
#[cfg(not(target_os = "macos"))]
const NAME_OFFSET: usize = 19;

/// One `getdents64` batch of directory records, on the forked child's stack.
#[cfg(not(target_os = "macos"))]
const DIRECTORY_BUFFER_BYTES: usize = 4096;

#[cfg(not(target_os = "macos"))]
impl Inherited {
    /// List `/proc/self/fd` with raw `getdents64`, which, unlike `readdir`, does not allocate.
    fn collect(&mut self, keep: RawFd) {
        // SAFETY: open with a constant NUL-terminated path; the result is checked.
        let directory = unsafe {
            libc::open(
                c"/proc/self/fd".as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        };
        if directory < 0 {
            return;
        }
        let mut buffer = [0_u8; DIRECTORY_BUFFER_BYTES];
        loop {
            // SAFETY: the kernel writes at most `buffer.len()` bytes of records into it.
            let read = unsafe {
                libc::syscall(
                    libc::SYS_getdents64,
                    directory,
                    buffer.as_mut_ptr(),
                    buffer.len(),
                )
            };
            let Ok(read) = usize::try_from(read) else {
                break;
            };
            if read == 0 {
                break;
            }
            let mut records = buffer.get(..read).unwrap_or_default();
            while let Some(&[low, high]) = records.get(RECORD_LENGTH_OFFSET..NAME_OFFSET - 1) {
                let length = usize::from(u16::from_ne_bytes([low, high]));
                let (Some(name), Some(rest)) =
                    (records.get(NAME_OFFSET..length), records.get(length..))
                else {
                    break;
                };
                if let Some(fd) = descriptor(name)
                    && fd != directory
                    && inherited(fd, keep)
                    && let Some(end) = pipe_end(fd)
                {
                    self.push(end);
                }
                records = rest;
            }
        }
        // SAFETY: closes the directory descriptor opened above.
        unsafe { libc::close(directory) };
    }
}

/// The descriptor a `/proc/self/fd` entry names: its NUL-terminated decimal digits.
#[cfg(not(target_os = "macos"))]
fn descriptor(name: &[u8]) -> Option<RawFd> {
    let digits = name.split(|&byte| byte == 0).next()?;
    if digits.is_empty() {
        return None;
    }
    digits.iter().try_fold(0 as RawFd, |fd, &byte| {
        let digit = RawFd::from(byte.checked_sub(b'0').filter(|digit| *digit <= 9)?);
        fd.checked_mul(10)?.checked_add(digit)
    })
}

/// Both ends of a Linux pipe share its inode; the access mode tells them apart.
#[cfg(not(target_os = "macos"))]
fn pipe_end(fd: RawFd) -> Option<PipeEnd> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: fstat writes a complete stat on success, which is checked before reading it.
    let stat = unsafe {
        if libc::fstat(fd, stat.as_mut_ptr()) != 0 {
            return None;
        }
        stat.assume_init()
    };
    if stat.st_mode & libc::S_IFMT != libc::S_IFIFO {
        return None;
    }
    // SAFETY: F_GETFL only reads this descriptor's status flags.
    let mode = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    let writes = match mode {
        ..0 => return None,
        mode if mode & libc::O_ACCMODE == libc::O_RDONLY => 0,
        mode if mode & libc::O_ACCMODE == libc::O_WRONLY => 1,
        // A FIFO opened for both reading and writing is no pipe of a run.
        _ => return None,
    };
    let inode = stat.st_ino;
    Some(PipeEnd {
        fd,
        end: Key(inode, writes),
        peer: Key(inode, 1 - writes),
    })
}

/// Darwin sys/proc_info.h `struct proc_fileinfo`.
#[cfg(target_os = "macos")]
#[repr(C)]
struct ProcFileInfo {
    open_flags: u32,
    status: u32,
    offset: libc::off_t,
    kind: i32,
    guard_flags: u32,
}

/// Darwin sys/proc_info.h `struct pipe_info`: each end has its own handle and names its peer.
#[cfg(target_os = "macos")]
#[repr(C)]
struct PipeInfo {
    stat: libc::vinfo_stat,
    handle: u64,
    peer_handle: u64,
    status: i32,
    reserved: i32,
}

/// Darwin sys/proc_info.h `struct pipe_fdinfo`.
#[cfg(target_os = "macos")]
#[repr(C)]
struct PipeFdInfo {
    file: ProcFileInfo,
    pipe: PipeInfo,
}

/// Darwin sys/proc_info.h `PROC_PIDFDPIPEINFO`.
#[cfg(target_os = "macos")]
const PROC_PIDFDPIPEINFO: libc::c_int = 6;

#[cfg(target_os = "macos")]
fn open_descriptors() -> io::Result<usize> {
    let pid = own_pid().ok_or_else(|| io::Error::other("the process ID exceeds pid_t"))?;
    // SAFETY: a null buffer asks libproc for the size of the descriptor list only.
    let bytes =
        unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0) };
    let bytes = usize::try_from(bytes).map_err(|_| io::Error::last_os_error())?;
    Ok(bytes / std::mem::size_of::<libc::proc_fdinfo>())
}

#[cfg(target_os = "macos")]
impl Inherited {
    /// List the descriptors with libproc, which writes into the reserved list.
    fn collect(&mut self, keep: RawFd) {
        let Some(pid) = own_pid() else {
            return;
        };
        self.listed.clear();
        let room = self.listed.capacity() * std::mem::size_of::<libc::proc_fdinfo>();
        let Ok(room) = libc::c_int::try_from(room) else {
            return;
        };
        // SAFETY: libproc writes at most `room` bytes of whole records into the reserved list.
        let written = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDLISTFDS,
                0,
                self.listed.as_mut_ptr().cast(),
                room,
            )
        };
        let Ok(written) = usize::try_from(written) else {
            return;
        };
        let count =
            (written / std::mem::size_of::<libc::proc_fdinfo>()).min(self.listed.capacity());
        // SAFETY: libproc initialized these `count` records, within the capacity.
        unsafe { self.listed.set_len(count) };
        for index in 0..count {
            let listed = &self.listed[index];
            let fd = listed.proc_fd;
            if listed.proc_fdtype == libc::PROX_FDTYPE_PIPE as u32
                && inherited(fd, keep)
                && let Some(end) = pipe_end(pid, fd)
            {
                self.push(end);
            }
        }
    }
}

/// This process's ID as libproc takes it.
#[cfg(target_os = "macos")]
fn own_pid() -> Option<libc::pid_t> {
    libc::pid_t::try_from(std::process::id()).ok()
}

#[cfg(target_os = "macos")]
fn pipe_end(pid: libc::pid_t, fd: RawFd) -> Option<PipeEnd> {
    let mut info = std::mem::MaybeUninit::<PipeFdInfo>::uninit();
    let size = libc::c_int::try_from(std::mem::size_of::<PipeFdInfo>()).ok()?;
    // SAFETY: libproc writes a complete record of exactly `size` bytes on success, which is
    // checked before reading it.
    let info = unsafe {
        if libc::proc_pidfdinfo(pid, fd, PROC_PIDFDPIPEINFO, info.as_mut_ptr().cast(), size) != size
        {
            return None;
        }
        info.assume_init()
    };
    // A pipe whose other end is closed everywhere names no peer.
    if info.pipe.peer_handle == 0 {
        return None;
    }
    Some(PipeEnd {
        fd,
        end: Key(info.pipe.handle, 0),
        peer: Key(info.pipe.peer_handle, 0),
    })
}
