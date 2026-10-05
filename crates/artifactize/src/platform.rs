//! Operating-system specifics behind one interface: owner-only files and directories, scoped
//! opens and listings that never follow a link, process trees held until admitted, stop
//! signals, hidden terminal input, and host details.
//!
//! Unix uses modes, `openat` with `O_NOFOLLOW`, `/proc`, and process groups. Windows uses
//! protected owner-only DACLs, handle-relative opens that refuse every reparse point, and
//! kill-on-close Job Objects.

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
use unix as os;
#[cfg(windows)]
use windows as os;

pub(crate) use os::{
    Child, EntryName, HiddenInput, canonicalize, create_private_dir, create_private_dir_all,
    detach, entry_kind, exit_signal, host_name, is_executable, is_private_dir, is_private_file,
    open_directory, open_entry, open_no_follow, open_nonblocking, private_options,
    private_tempdir_in, process_start_time, read_dir, restrict_file, spawn_gated, stop_requested,
    sync_dir,
};

/// The type of a directory entry, read without following it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileKind {
    File,
    Directory,
    Symlink,
    /// FIFOs, sockets, devices, and on Windows reparse points that are not links.
    Other,
}
