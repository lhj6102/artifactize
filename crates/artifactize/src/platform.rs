//! Operating-system specifics behind one interface: owner-only files and directories, scoped
//! opens and listings that never follow a link, process trees held until admitted, stop
//! signals, hidden terminal input, and host details.
//!
//! Unix uses modes, `openat` with `O_NOFOLLOW`, `/proc`, and process groups. Windows uses
//! protected owner-only DACLs, handle-relative opens that refuse every reparse point, and
//! kill-on-close Job Objects.

#[cfg(any(windows, test))]
mod directory_scan;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
use unix as os;
#[cfg(windows)]
use windows as os;

pub(crate) use os::{
    Child, DEFAULT_EDITOR, EntryName, HiddenInput, canonicalize, create_private_dir,
    create_private_dir_all, editor, entry_kind, exit_signal, host_name, is_executable,
    is_link_refusal, is_private_dir, is_private_file, open_directory, open_entry, open_no_follow,
    open_nonblocking, private_options, private_tempdir_in, process_start_time, read_dir,
    restrict_file, spawn_detached, spawn_gated, stop_requested, sync_dir,
};

#[cfg(unix)]
pub(crate) use unix::GROUP_OTHER_BITS;
#[cfg(windows)]
pub(crate) use windows::{is_owner_only, private_pipe, user_identity};

/// Poll contended process-shared file locks without blocking the async runtime;
/// 25 ms keeps session sends and credential refreshes responsive without busy-waiting.
pub(crate) const FILE_LOCK_RETRY_INTERVAL: std::time::Duration =
    std::time::Duration::from_millis(25);

/// The type of a directory entry, read without following it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileKind {
    File,
    Directory,
    Symlink,
    /// FIFOs, sockets, devices, and on Windows reparse points that are not links.
    Other,
}
