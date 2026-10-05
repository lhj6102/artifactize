//! Unix: file modes, `openat` without following links, listings through `/proc/self/fd`,
//! signals, and termios.

mod process;

use std::{
    ffi::{CString, OsStr, OsString},
    fs::{self, DirBuilder, File, OpenOptions, Permissions},
    future::Future,
    io,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
            process::ExitStatusExt,
        },
    },
    path::{Path, PathBuf},
    process::ExitStatus,
};

use tempfile::TempDir;
use tokio::signal::unix::{SignalKind, signal};

use super::FileKind;

pub(crate) use process::{Child, process_start_time, spawn_detached, spawn_gated};

/// Create a directory and its missing parents as 0700; existing ones keep their mode.
pub(crate) fn create_private_dir_all(path: &Path) -> io::Result<()> {
    DirBuilder::new().recursive(true).mode(0o700).create(path)
}

/// Create one directory as exactly 0700, whatever the umask.
pub(crate) fn create_private_dir(path: &Path) -> io::Result<()> {
    DirBuilder::new().mode(0o700).create(path)?;
    fs::set_permissions(path, Permissions::from_mode(0o700))
}

/// A new 0700 temporary directory below `parent`.
pub(crate) fn private_tempdir_in(prefix: &str, parent: &Path) -> io::Result<TempDir> {
    let directory = tempfile::Builder::new()
        .prefix(prefix)
        .permissions(Permissions::from_mode(0o700))
        .tempdir_in(parent)?;
    fs::set_permissions(directory.path(), Permissions::from_mode(0o700))?;
    Ok(directory)
}

/// Whether a directory grants nothing to its group or others.
pub(crate) fn is_private_dir(path: &Path) -> io::Result<bool> {
    Ok(fs::metadata(path)?.mode() & 0o077 == 0)
}

/// Whether an open file is a regular, single-link 0600 file.
pub(crate) fn is_private_file(file: &File) -> io::Result<bool> {
    let metadata = file.metadata()?;
    Ok(metadata.is_file() && metadata.mode() & 0o777 == 0o600 && metadata.nlink() == 1)
}

/// Options that create files as 0600.
pub(crate) fn private_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.mode(0o600);
    options
}

/// Make an open file 0600.
pub(crate) fn restrict_file(file: &File) -> io::Result<()> {
    file.set_permissions(Permissions::from_mode(0o600))
}

/// The absolute path of an existing file with every link resolved.
pub(crate) fn canonicalize(path: &Path) -> io::Result<PathBuf> {
    fs::canonicalize(path)
}

/// Persist a directory's entries, such as a file just renamed into it.
pub(crate) fn sync_dir(path: &Path) -> io::Result<()> {
    File::open(path).and_then(|directory| directory.sync_all())
}

/// Open without following a link in the last component or blocking on a FIFO.
pub(crate) fn open_no_follow(options: &mut OpenOptions, path: &Path) -> io::Result<File> {
    options
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
}

/// Open for reading without blocking on a FIFO, so its type can be checked first.
pub(crate) fn open_nonblocking(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
}

/// Open a directory by path, such as the root a scoped walk starts from.
pub(crate) fn open_directory(path: &Path) -> io::Result<File> {
    File::open(path)
}

/// One path component, ready for `openat`.
pub(crate) struct EntryName(CString);

impl EntryName {
    pub fn new(name: &OsStr) -> Option<Self> {
        CString::new(name.as_bytes()).ok().map(Self)
    }
}

/// Open one entry of a pinned directory read-only, without following a symlink.
pub(crate) fn open_entry(directory: &File, name: &EntryName) -> io::Result<File> {
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

/// The entries of a pinned directory, listed through its descriptor rather than a path
/// that could have been replaced by a link.
pub(crate) fn read_dir(
    directory: &File,
) -> io::Result<impl Iterator<Item = io::Result<DirEntry>> + '_> {
    Ok(
        fs::read_dir(format!("/proc/self/fd/{}", directory.as_raw_fd()))?
            .map(|entry| entry.map(DirEntry)),
    )
}

pub(crate) struct DirEntry(fs::DirEntry);

impl DirEntry {
    pub fn file_name(&self) -> OsString {
        self.0.file_name()
    }

    pub fn file_type(&self) -> io::Result<FileKind> {
        self.0.file_type().map(kind)
    }
}

/// The type of one entry of a pinned directory, without following it.
pub(crate) fn entry_kind(directory: &File, name: &OsStr) -> io::Result<FileKind> {
    let path = Path::new(&format!("/proc/self/fd/{}", directory.as_raw_fd())).join(name);
    fs::symlink_metadata(path).map(|metadata| kind(metadata.file_type()))
}

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

/// Whether a path is a regular file with an execute bit.
pub(crate) fn is_executable(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// The editor a review opens fields in when `EDITOR` is unset.
pub(crate) const DEFAULT_EDITOR: &str = "vi";

/// Run `$EDITOR file` through `sh`, as `EDITOR` may hold arguments, such as `code --wait`.
pub(crate) fn editor(editor: &str, file: &Path) -> tokio::process::Command {
    let mut command = tokio::process::Command::new("sh");
    command
        .arg("-c")
        .arg(format!("{editor} \"$1\""))
        .arg("sh")
        .arg(file);
    command
}

/// The signal that ended a process, if one did.
pub(crate) fn exit_signal(status: &ExitStatus) -> Option<i32> {
    status.signal()
}

pub(crate) fn host_name() -> Option<String> {
    fs::read_to_string("/proc/sys/kernel/hostname").ok()
}

/// Register for SIGINT and SIGTERM now; the future completes at the first of them.
pub(crate) fn stop_requested() -> io::Result<impl Future<Output = ()> + Send + 'static> {
    let mut interrupt = signal(SignalKind::interrupt())?;
    let mut terminate = signal(SignalKind::terminate())?;
    Ok(async move {
        tokio::select! { _ = interrupt.recv() => {}, _ = terminate.recv() => {} }
    })
}

/// Turns terminal echo off until dropped, so a pasted secret is not shown.
pub(crate) struct HiddenInput(libc::termios);

impl HiddenInput {
    pub fn new() -> io::Result<Self> {
        let mut termios = std::mem::MaybeUninit::uninit();
        // SAFETY: tcgetattr fills the struct on success; it is read only then.
        let saved = unsafe {
            if libc::tcgetattr(libc::STDIN_FILENO, termios.as_mut_ptr()) != 0 {
                return Err(io::Error::last_os_error());
            }
            termios.assume_init()
        };
        let mut hidden = saved;
        hidden.c_lflag &= !libc::ECHO;
        // SAFETY: a valid termios for the same descriptor.
        if unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &hidden) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(saved))
    }
}

impl Drop for HiddenInput {
    fn drop(&mut self) {
        // SAFETY: restores the attributes read in `new`.
        unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.0) };
    }
}
