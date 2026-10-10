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
    CACHE_VARIABLES, CRLF_LINE_ENDINGS, Child, DEFAULT_EDITOR, ENV_NAMES_IGNORE_CASE,
    HOME_VARIABLE, HOME_VARIABLES, HiddenInput, STATE_VARIABLES, SYSTEM_VARIABLES, TEMP_VARIABLES,
    USER_VARIABLES, VISIBLE_INPUT_FALLBACK, create_private_dir, create_private_dir_all, editor,
    exit_signal, file_identity, host_name, ipc, is_owner_only, is_private_dir, is_private_file,
    path_from_bytes, private_options, private_tempdir_in, process_start_time,
    resolve_system_aliases, restrict_file, spawn_detached, spawn_gated, stop_requested, sync_dir,
};

/// A file's identity on its volume. Two open files with equal identities are the same file,
/// whatever paths reached them; a replaced file gets a new identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FileIdentity {
    volume: u64,
    index: u64,
}

/// Poll contended process-shared file locks without blocking the async runtime;
/// 25 ms keeps session sends and credential refreshes responsive without busy-waiting.
pub(crate) const FILE_LOCK_RETRY_INTERVAL: std::time::Duration =
    std::time::Duration::from_millis(25);

pub(crate) use artifactize_tools::files::{
    EntryName, FileKind, canonicalize, entry_kind, open_directory, open_entry, open_no_follow,
    open_nonblocking, read_dir,
};
