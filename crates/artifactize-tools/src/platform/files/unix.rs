//! Unix pinned opens and descriptor-based directory listings.

use std::{
    ffi::{CStr, CString, OsStr, OsString},
    fs::{self, File, OpenOptions},
    io,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::OpenOptionsExt},
    },
    path::{Path, PathBuf},
    ptr::NonNull,
};

use super::FileKind;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{case_sensitive, exact_name};

/// The absolute path of an existing file with every link resolved.
pub fn canonicalize(path: &Path) -> io::Result<PathBuf> {
    fs::canonicalize(path)
}

/// Open without following a link in the last component or blocking on a FIFO.
pub fn open_no_follow(options: &mut OpenOptions, path: &Path) -> io::Result<File> {
    options
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
}

/// Open for reading without blocking on a FIFO, so its type can be checked first.
pub fn open_nonblocking(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
}

/// Open a directory by path, such as the root a scoped walk starts from.
pub fn open_directory(path: &Path) -> io::Result<File> {
    File::open(path)
}

/// One path component, ready for `openat`.
pub struct EntryName(CString);

impl EntryName {
    pub fn new(name: &OsStr) -> Option<Self> {
        let bytes = name.as_bytes();
        (!bytes.is_empty() && !matches!(bytes, b"." | b"..") && !bytes.contains(&b'/'))
            .then(|| CString::new(bytes).ok().map(Self))
            .flatten()
    }
}

/// Open one entry of a pinned directory read-only, without following a symlink.
pub fn open_entry(directory: &File, name: &EntryName) -> io::Result<File> {
    // O_NONBLOCK avoids waiting on a FIFO before its type can be rejected.
    // SAFETY: a valid directory descriptor and a NUL-terminated name; the result is checked.
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.0.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: openat returned a new descriptor that nothing else owns.
    Ok(unsafe { File::from_raw_fd(fd) })
}

/// Whether `open_entry` failed because the entry is a symlink: `O_NOFOLLOW` reports `ELOOP`
/// on Linux and macOS, and `EMLINK` on FreeBSD and DragonFly.
pub fn is_link_refusal(error: &io::Error) -> bool {
    let link = if cfg!(any(target_os = "freebsd", target_os = "dragonfly")) {
        libc::EMLINK
    } else {
        libc::ELOOP
    };
    error.raw_os_error() == Some(link)
}

/// The entries of a pinned directory, listed through its descriptor rather than a path
/// that could have been replaced by a link.
pub fn read_dir(directory: &File) -> io::Result<impl Iterator<Item = io::Result<DirEntry<'_>>>> {
    // Open a new description, not dup: directory offsets must be independent across scans.
    // SAFETY: a live descriptor and a constant NUL-terminated name; no links are followed.
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            c".".as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fd is an owned directory descriptor. fdopendir takes ownership on success.
    let stream = unsafe { libc::fdopendir(fd) };
    let Some(stream) = NonNull::new(stream) else {
        let error = io::Error::last_os_error();
        // SAFETY: fdopendir failed, leaving fd owned here.
        unsafe { libc::close(fd) };
        return Err(error);
    };
    Ok(ReadDir {
        stream,
        directory,
        finished: false,
    })
}

struct ReadDir<'a> {
    stream: NonNull<libc::DIR>,
    directory: &'a File,
    finished: bool,
}

impl<'a> Iterator for ReadDir<'a> {
    type Item = io::Result<DirEntry<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        while !self.finished {
            // SAFETY: this iterator exclusively owns the stream. errno distinguishes EOF
            // from failure; the name is copied before the next readdir can overwrite it.
            let entry = unsafe {
                *errno() = 0;
                libc::readdir(self.stream.as_ptr())
            };
            if entry.is_null() {
                self.finished = true;
                let error = io::Error::last_os_error();
                return (error.raw_os_error() != Some(0)).then_some(Err(error));
            }
            // SAFETY: readdir returned a dirent with a NUL-terminated d_name.
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
            if name == b"." || name == b".." {
                continue;
            }
            return Some(Ok(DirEntry {
                directory: self.directory,
                name: OsStr::from_bytes(name).to_owned(),
            }));
        }
        None
    }
}

impl Drop for ReadDir<'_> {
    fn drop(&mut self) {
        // SAFETY: this stream is owned exclusively, and closed exactly once.
        unsafe { libc::closedir(self.stream.as_ptr()) };
    }
}

pub struct DirEntry<'a> {
    directory: &'a File,
    name: OsString,
}

impl DirEntry<'_> {
    pub fn file_name(&self) -> OsString {
        self.name.clone()
    }

    pub fn file_type(&self) -> io::Result<FileKind> {
        entry_kind(self.directory, &self.name)
    }
}

/// The type of one entry of a pinned directory, without following it.
pub fn entry_kind(directory: &File, name: &OsStr) -> io::Result<FileKind> {
    let name = EntryName::new(name).ok_or(io::ErrorKind::InvalidInput)?;
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: the descriptor and name are live and stat is a writable output buffer.
    // AT_SYMLINK_NOFOLLOW inspects the entry itself, never its target.
    if unsafe {
        libc::fstatat(
            directory.as_raw_fd(),
            name.0.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fstatat filled the struct on success.
    Ok(match unsafe { stat.assume_init() }.st_mode & libc::S_IFMT {
        libc::S_IFREG => FileKind::File,
        libc::S_IFDIR => FileKind::Directory,
        libc::S_IFLNK => FileKind::Symlink,
        _ => FileKind::Other,
    })
}

/// This thread's `errno`, which `readdir` leaves unchanged at the end of a directory.
fn errno() -> *mut libc::c_int {
    // SAFETY: both functions return this thread's errno location and have no preconditions.
    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
    unsafe {
        libc::__error()
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "freebsd")))]
    unsafe {
        libc::__errno_location()
    }
}

/// Other Unix platforms keep their existing case-sensitive open behavior.
#[cfg(not(target_os = "macos"))]
pub fn exact_name(_file: &File, _name: &OsStr) -> io::Result<bool> {
    Ok(true)
}

/// Preserve the existing case-sensitive logical namespace on other Unix systems.
#[cfg(not(target_os = "macos"))]
pub fn case_sensitive(_directory: &File) -> io::Result<bool> {
    Ok(true)
}
