//! Unix private file modes, process control, signals, and termios.
//! Pinned file access lives in artifactize-tools.

mod process;

use std::{
    fs::{self, DirBuilder, File, OpenOptions, Permissions},
    future::Future,
    io,
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
        process::ExitStatusExt,
    },
    path::Path,
    process::ExitStatus,
};

use tempfile::TempDir;
use tokio::signal::unix::{SignalKind, signal};

pub(crate) use process::{Child, process_start_time, spawn_detached, spawn_gated};

/// Owner-only directory mode (rwx------): these directories hold credentials, Agent sessions,
/// sockets and review state, so no other local user may list or enter them.
const PRIVATE_DIR_MODE: u32 = 0o700;

/// Owner-only file mode (rw-------): these files hold credentials, Agent transcripts and review
/// evidence, so no other local user may read them.
const PRIVATE_FILE_MODE: u32 = 0o600;

/// The group and other permission bits; a private path has none of them set.
pub(crate) const GROUP_OTHER_BITS: u32 = 0o077;

/// The permission bits of a mode, without the file type and set-id bits, so a file can be
/// compared with [`PRIVATE_FILE_MODE`] exactly.
const PERMISSION_BITS: u32 = 0o777;

/// The owner, group and other execute bits; any one of them makes a file runnable.
const EXECUTE_BITS: u32 = 0o111;

/// Create a directory and its missing parents as 0700; existing ones keep their mode.
pub(crate) fn create_private_dir_all(path: &Path) -> io::Result<()> {
    DirBuilder::new()
        .recursive(true)
        .mode(PRIVATE_DIR_MODE)
        .create(path)
}

/// Create one directory as exactly 0700, whatever the umask.
pub(crate) fn create_private_dir(path: &Path) -> io::Result<()> {
    DirBuilder::new().mode(PRIVATE_DIR_MODE).create(path)?;
    fs::set_permissions(path, Permissions::from_mode(PRIVATE_DIR_MODE))
}

/// A new 0700 temporary directory below `parent`.
pub(crate) fn private_tempdir_in(prefix: &str, parent: &Path) -> io::Result<TempDir> {
    let directory = tempfile::Builder::new()
        .prefix(prefix)
        .permissions(Permissions::from_mode(PRIVATE_DIR_MODE))
        .tempdir_in(parent)?;
    fs::set_permissions(directory.path(), Permissions::from_mode(PRIVATE_DIR_MODE))?;
    Ok(directory)
}

/// Whether a directory grants nothing to its group or others.
pub(crate) fn is_private_dir(path: &Path) -> io::Result<bool> {
    Ok(fs::metadata(path)?.mode() & GROUP_OTHER_BITS == 0)
}

/// Whether an open file is a regular, single-link 0600 file.
pub(crate) fn is_private_file(file: &File) -> io::Result<bool> {
    let metadata = file.metadata()?;
    Ok(metadata.is_file()
        && metadata.mode() & PERMISSION_BITS == PRIVATE_FILE_MODE
        && metadata.nlink() == 1)
}

/// Options that create files as 0600.
pub(crate) fn private_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.mode(PRIVATE_FILE_MODE);
    options
}

/// Make an open file 0600.
pub(crate) fn restrict_file(file: &File) -> io::Result<()> {
    file.set_permissions(Permissions::from_mode(PRIVATE_FILE_MODE))
}

/// Persist a directory's entries, such as a file just renamed into it.
pub(crate) fn sync_dir(path: &Path) -> io::Result<()> {
    File::open(path).and_then(|directory| directory.sync_all())
}

/// Whether a path is a regular file with an execute bit.
pub(crate) fn is_executable(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & EXECUTE_BITS != 0)
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
