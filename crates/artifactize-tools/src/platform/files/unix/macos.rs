//! Descriptor-relative enumeration and exact entry names on macOS.

use std::{
    ffi::{CStr, OsStr, OsString},
    fs::File,
    io,
    os::{fd::AsRawFd, unix::ffi::OsStrExt},
    path::Path,
    ptr::NonNull,
};

use super::EntryName;
use crate::platform::files::FileKind;

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
                *libc::__error() = 0;
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

/// Require the directory entry's actual spelling, even on case-insensitive volumes. This
/// prevents logical exclusions being bypassed by case or Unicode normalization aliases.
pub fn exact_name(file: &File, name: &OsStr) -> io::Result<bool> {
    let mut path = [0_u8; libc::PATH_MAX as usize];
    // SAFETY: F_GETPATH writes at most PATH_MAX bytes into this buffer for a live descriptor.
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETPATH, path.as_mut_ptr()) } == -1 {
        return Err(io::Error::last_os_error());
    }
    let end = path
        .iter()
        .position(|&byte| byte == 0)
        .ok_or(io::ErrorKind::InvalidData)?;
    Ok(Path::new(OsStr::from_bytes(&path[..end])).file_name() == Some(name))
}

/// Ask the pinned directorys volume, since APFS supports both case policies.
pub fn case_sensitive(directory: &File) -> io::Result<bool> {
    // SAFETY: fpathconf queries a live descriptor and writes no user memory.
    let sensitive = unsafe { libc::fpathconf(directory.as_raw_fd(), libc::_PC_CASE_SENSITIVE) };
    if sensitive == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(sensitive != 0)
}
