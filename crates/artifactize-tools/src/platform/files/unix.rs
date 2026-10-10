//! Unix pinned opens and descriptor-based directory listings.

use std::{
    ffi::{CString, OsStr},
    fs::{self, File, OpenOptions},
    io,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::OpenOptionsExt},
    },
    path::{Path, PathBuf},
};

#[cfg(not(target_os = "macos"))]
use std::ffi::OsString;

#[cfg(not(target_os = "macos"))]
use super::FileKind;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{case_sensitive, entry_kind, exact_name, read_dir};

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
#[cfg(not(target_os = "macos"))]
pub fn read_dir(directory: &File) -> io::Result<impl Iterator<Item = io::Result<DirEntry>> + '_> {
    Ok(
        fs::read_dir(format!("/proc/self/fd/{}", directory.as_raw_fd()))?
            .map(|entry| entry.map(DirEntry)),
    )
}

#[cfg(not(target_os = "macos"))]
pub struct DirEntry(fs::DirEntry);

#[cfg(not(target_os = "macos"))]
impl DirEntry {
    pub fn file_name(&self) -> OsString {
        self.0.file_name()
    }

    pub fn file_type(&self) -> io::Result<FileKind> {
        self.0.file_type().map(kind)
    }
}

/// The type of one entry of a pinned directory, without following it.
#[cfg(not(target_os = "macos"))]
pub fn entry_kind(directory: &File, name: &OsStr) -> io::Result<FileKind> {
    let path = Path::new(&format!("/proc/self/fd/{}", directory.as_raw_fd())).join(name);
    fs::symlink_metadata(path).map(|metadata| kind(metadata.file_type()))
}

#[cfg(not(target_os = "macos"))]
fn kind(file_type: fs::FileType) -> FileKind {
    if file_type.is_file() {
        FileKind::File
    } else if file_type.is_dir() {
        FileKind::Directory
    } else if file_type.is_symlink() {
        FileKind::Symlink
    } else {
        FileKind::Other
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
