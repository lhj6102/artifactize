//! Non-mutating observations of fixture file handles.
use std::{fs, io, path::Path};

/// Whether this process has released the SQLite file. This neither renames the file
/// nor changes its contents; an open read-only SQLite connection must also be detected.
pub fn database_released(path: &Path) -> io::Result<bool> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        let database = fs::metadata(path)?;
        // Both Unix platforms expose this process's open descriptors here. Inspect
        // their targets, not just SQLite locks (an idle reader need not hold a lock).
        for entry in fs::read_dir("/dev/fd")? {
            match fs::metadata(entry?.path()) {
                Ok(open) if open.dev() == database.dev() && open.ino() == database.ino() => {
                    return Ok(false);
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    // A different thread may close a descriptor while it is enumerated.
                }
                Err(error) => return Err(error),
            }
        }
        Ok(true)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::{
            Foundation::ERROR_SHARING_VIOLATION,
            Storage::FileSystem::{DELETE, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE},
        };

        // SQLite denies delete sharing even for a read-only connection. Requesting
        // DELETE access observes that denial; it does not request file deletion.
        match fs::OpenOptions::new()
            .access_mode(DELETE)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .open(path)
        {
            Ok(_) => Ok(true),
            Err(error) if error.raw_os_error() == Some(ERROR_SHARING_VIOLATION as i32) => Ok(false),
            Err(error) => Err(error),
        }
    }
}
