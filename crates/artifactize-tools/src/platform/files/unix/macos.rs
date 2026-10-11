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
    let path = rustix::fs::getpath(file)?;
    Ok(Path::new(OsStr::from_bytes(path.to_bytes())).file_name() == Some(name))
}

/// Ask the pinned directory's volume, since APFS supports both case policies.
pub fn case_sensitive(directory: &File) -> io::Result<bool> {
    // SAFETY: fpathconf queries a live descriptor and writes no user memory.
    let sensitive = unsafe { libc::fpathconf(directory.as_raw_fd(), libc::_PC_CASE_SENSITIVE) };
    if sensitive == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(sensitive != 0)
}
