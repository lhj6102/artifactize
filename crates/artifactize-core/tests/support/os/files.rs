//! Non-mutating observations of fixture file handles.
use std::{fs, io, path::Path};

/// Whether this process has released the SQLite file. This neither renames the file
/// nor changes its contents; an open read-only SQLite connection must also be detected.
pub fn database_released(path: &Path) -> io::Result<bool> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        let database = fs::metadata(path)?;
        // Enumerate descriptor numbers, then inspect the descriptors themselves.
        // Darwin's /dev/fd entries are not Linux-style links to the target files;
        // stat on an entry can fail with EBADF. Neither probe opens or borrows an
        // owned descriptor, nor relies on SQLite locks (an idle reader has none).
        for entry in fs::read_dir("/dev/fd")? {
            let descriptor = entry?
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<libc::c_int>().ok())
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "non-numeric descriptor entry")
                })?;
            let mut open = std::mem::MaybeUninit::<libc::stat>::uninit();
            // SAFETY: fstat only observes the descriptor and writes to a valid out
            // pointer. A concurrent close is reported as EBADF, not owned or closed here.
            if unsafe { libc::fstat(descriptor, open.as_mut_ptr()) } == -1 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::EBADF) {
                    // Descriptor enumeration is not a snapshot: another thread may
                    // have closed this slot, or Darwin may list an unused slot.
                    continue;
                }
                return Err(error);
            }
            // SAFETY: a successful fstat initialized the entire stat structure.
            let open = unsafe { open.assume_init() };
            // Darwin's dev_t is signed; Linux's is unsigned. Widen without truncation.
            if i128::from(open.st_dev) == i128::from(database.dev())
                && open.st_ino == database.ino()
            {
                return Ok(false);
            }
        }
        Ok(true)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Foundation::ERROR_SHARING_VIOLATION;

        // Denying read/write sharing detects even an idle SQLite reader through
        // another hard link to the file. DELETE access is insufficient: Windows
        // can permit deleting one link while a handle holds another link open.
        // This requests read access only and never changes the file or its names.
        match fs::OpenOptions::new().read(true).share_mode(0).open(path) {
            Ok(_) => Ok(true),
            Err(error) if error.raw_os_error() == Some(ERROR_SHARING_VIOLATION as i32) => Ok(false),
            Err(error) => Err(error),
        }
    }
}
