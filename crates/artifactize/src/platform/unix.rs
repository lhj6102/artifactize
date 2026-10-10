//! Unix private file modes, process control, signals, and termios.
//! Pinned file access lives in artifactize-tools.

pub(crate) mod ipc;
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

/// How a private directory and file are described in messages.
pub(crate) const PRIVATE_DIRECTORY: &str = "owner-only (mode 0700)";
pub(crate) const PRIVATE_FILE: &str = "readable by their owner only (mode 0600)";

/// The group and other permission bits; a private path has none of them set.
pub(crate) const GROUP_OTHER_BITS: u32 = 0o077;

/// The permission bits of a mode, without the file type and set-id bits, so a file can be
/// compared with [`PRIVATE_FILE_MODE`] exactly.
const PERMISSION_BITS: u32 = 0o777;

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

/// A file's identity on its volume: its device and inode.
pub(crate) fn file_identity(file: &File) -> io::Result<super::FileIdentity> {
    let metadata = file.metadata()?;
    Ok(super::FileIdentity {
        volume: metadata.dev(),
        index: metadata.ino(),
    })
}

/// The effective user, the owner that private paths must have.
pub(crate) fn current_user() -> u32 {
    // SAFETY: geteuid has no preconditions and does not mutate process identity.
    unsafe { libc::geteuid() }
}

/// Whether an open file or directory belongs to the current user and grants nothing to its
/// group or others.
pub(crate) fn is_owner_only(file: &File) -> io::Result<bool> {
    let metadata = file.metadata()?;
    Ok(metadata.mode() & GROUP_OTHER_BITS == 0 && metadata.uid() == current_user())
}

/// Environment variable names compare exactly.
pub(crate) const ENV_NAMES_IGNORE_CASE: bool = false;

/// The variable naming the user's home directory.
pub(crate) const HOME_VARIABLE: &str = "HOME";

/// Variables programs read their home directory from.
pub(crate) const HOME_VARIABLES: &[&str] = &[HOME_VARIABLE];

/// Variables programs read their cache directory from.
pub(crate) const CACHE_VARIABLES: &[&str] = &["XDG_CACHE_HOME"];

/// Variables programs read their temporary directory from. `TMP` and `TEMP` are set too, for
/// programs written for Windows.
pub(crate) const TEMP_VARIABLES: &[&str] = &["TMPDIR", "TMP", "TEMP"];

/// System variables a child cannot start without; none beyond `PATH` here.
pub(crate) const SYSTEM_VARIABLES: &[&str] = &[];

/// Variables naming a per-user state directory, in order; artifactize keeps its state in an
/// `artifactize` folder below the first one set, and otherwise below `HOME`.
pub(crate) const STATE_VARIABLES: &[&str] = &["XDG_STATE_HOME"];

/// The variables naming the signed-in user, in order.
pub(crate) const USER_VARIABLES: &[&str] = &["USER", "LOGNAME"];

/// The user's home directory.
pub(crate) fn home_directory() -> Option<std::path::PathBuf> {
    std::env::var_os(HOME_VARIABLE)
        .filter(|path| !path.is_empty())
        .map(Into::into)
}

/// A path from the raw bytes a tool such as git prints: any bytes name a Unix path.
pub(crate) fn path_from_bytes(bytes: &[u8]) -> std::path::PathBuf {
    use std::os::unix::ffi::OsStringExt;
    std::ffi::OsString::from_vec(bytes.to_vec()).into()
}

/// Unix programs end a line with LF only.
pub(crate) const CRLF_LINE_ENDINGS: bool = false;

/// Whether a terminal that cannot hide input is still read, visibly, after a warning. A Unix
/// terminal always can; failing to hide input is an error.
pub(crate) const VISIBLE_INPUT_FALLBACK: bool = false;

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

/// The operating system's host name, without depending on a proc filesystem.
pub(crate) fn host_name() -> Option<String> {
    hostname::get().ok()?.into_string().ok()
}

/// The absolute form of `path` with a leading macOS system alias (`/tmp`, `/var`, `/etc`)
/// replaced by its fixed `/private` target. The alias and its parents must be root-owned
/// and not replaceable; descendants are left for the caller to open without following links.
#[cfg(target_os = "macos")]
pub(crate) fn resolve_system_aliases(path: &Path) -> io::Result<std::path::PathBuf> {
    let absolute = std::path::absolute(path)?;
    for name in ["tmp", "var", "etc"] {
        let alias = Path::new("/").join(name);
        if let Ok(suffix) = absolute.strip_prefix(&alias) {
            let metadata = alias.symlink_metadata()?;
            let target = Path::new("/private").join(name);
            let link = fs::read_link(&alias)?;
            let link = if link.is_absolute() {
                link
            } else {
                Path::new("/").join(link)
            };
            if !metadata.is_symlink() || metadata.uid() != 0 || link != target {
                return Err(io::Error::other(format!(
                    "Refusing an untrusted system alias: {}",
                    alias.display()
                )));
            }
            for parent in [Path::new("/"), Path::new("/private")] {
                let metadata = parent.symlink_metadata()?;
                if !metadata.is_dir() || metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
                    return Err(io::Error::other(
                        "System alias parents must not be replaceable.",
                    ));
                }
            }
            return Ok(target.join(suffix));
        }
    }
    Ok(absolute)
}

/// The absolute form of `path`; this system has no aliased system roots.
#[cfg(not(target_os = "macos"))]
pub(crate) fn resolve_system_aliases(path: &Path) -> io::Result<std::path::PathBuf> {
    std::path::absolute(path)
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
