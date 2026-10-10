//! Exact entry names and volume case policy on macOS.

use std::{
    ffi::OsStr,
    fs::File,
    io,
    os::{fd::AsRawFd, unix::ffi::OsStrExt},
    path::Path,
};

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
