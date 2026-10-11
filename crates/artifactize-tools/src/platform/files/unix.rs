//! Unix pinned opens and descriptor-based directory listings.

use std::{
    ffi::{CString, OsStr, OsString},
    fs::{self, File, OpenOptions},
    io,
    os::unix::{ffi::OsStrExt, fs::OpenOptionsExt},
    path::{Path, PathBuf},
};

use rustix::{
    fs::{AtFlags, Dir, FileType, Mode, OFlags, openat, statat},
    io::Errno,
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
        .custom_flags((OFlags::NOFOLLOW | OFlags::NONBLOCK).bits() as i32)
        .open(path)
}

/// Open for reading without blocking on a FIFO, so its type can be checked first.
pub fn open_nonblocking(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(OFlags::NONBLOCK.bits() as i32)
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
    // NONBLOCK avoids waiting on a FIFO before its type can be rejected.
    Ok(openat(
        directory,
        &name.0,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )?
    .into())
}

/// Whether `open_entry` failed because the entry is a symlink: `O_NOFOLLOW` reports `ELOOP`
/// on Linux and macOS, and `EMLINK` on FreeBSD and DragonFly.
pub fn is_link_refusal(error: &io::Error) -> bool {
    let link = if cfg!(any(target_os = "freebsd", target_os = "dragonfly")) {
        Errno::MLINK
    } else {
        Errno::LOOP
    };
    error.raw_os_error() == Some(link.raw_os_error())
}

/// The entries of a pinned directory, listed through its descriptor rather than a path
/// that could have been replaced by a link.
pub fn read_dir(directory: &File) -> io::Result<impl Iterator<Item = io::Result<DirEntry<'_>>>> {
    // Open a new description, not dup: directory offsets must be independent across scans.
    let fd = openat(
        directory,
        c".",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    Ok(ReadDir {
        stream: Dir::new(fd)?,
        directory,
        state: Scan::Reading,
    })
}

/// A failed or exhausted stream never reads the directory again.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Scan {
    Reading,
    Done,
}

struct ReadDir<'a> {
    stream: Dir,
    directory: &'a File,
    state: Scan,
}

impl<'a> Iterator for ReadDir<'a> {
    type Item = io::Result<DirEntry<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.state == Scan::Reading {
            let entry = match self.stream.next() {
                Some(Ok(entry)) => entry,
                Some(Err(error)) => {
                    self.state = Scan::Done;
                    return Some(Err(error.into()));
                }
                None => {
                    self.state = Scan::Done;
                    return None;
                }
            };
            let name = entry.file_name().to_bytes();
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
    // Inspect the entry itself, never its target.
    let stat = statat(directory, &name.0, AtFlags::SYMLINK_NOFOLLOW)?;
    Ok(match FileType::from_raw_mode(stat.st_mode) {
        FileType::RegularFile => FileKind::File,
        FileType::Directory => FileKind::Directory,
        FileType::Symlink => FileKind::Symlink,
        _ => FileKind::Other,
    })
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
