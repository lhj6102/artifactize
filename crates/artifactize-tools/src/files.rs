//! Shared safe file access: pinned no-follow opens and directory listings.

#[cfg(any(windows, test))]
mod directory_scan;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use unix::{
    EntryName, canonicalize, entry_kind, is_link_refusal, open_directory, open_entry,
    open_no_follow, open_nonblocking, read_dir,
};
#[cfg(windows)]
pub use windows::{
    EntryName, canonicalize, entry_kind, is_link_refusal, open_directory, open_entry,
    open_no_follow, open_nonblocking, read_dir,
};

/// The type of a directory entry, read without following it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    File,
    Directory,
    Symlink,
    /// FIFOs, sockets, devices, and on Windows reparse points that are not links.
    Other,
}
