//! Shared safe file access: pinned no-follow opens and directory listings.

#[cfg(any(windows, test))]
mod directory_scan;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use unix::{
    EntryName, canonicalize, case_sensitive, entry_kind, exact_name, is_link_refusal,
    open_directory, open_entry, open_no_follow, open_nonblocking, read_dir,
};
#[cfg(windows)]
pub use windows::{
    EntryName, canonicalize, case_sensitive, entry_kind, exact_name, is_link_refusal,
    open_directory, open_entry, open_no_follow, open_nonblocking, read_dir,
};

/// The filesystem root of an absolute path (`/`, or a Windows volume or share) and the names
/// below it, in order. `None` when a component is `.` or `..`.
pub fn split_root(path: &std::path::Path) -> Option<(&std::path::Path, Vec<&std::ffi::OsStr>)> {
    use std::path::Component;
    let root = path.ancestors().last()?;
    path.components()
        .filter_map(|component| match component {
            Component::Prefix(_) | Component::RootDir => None,
            Component::Normal(name) => Some(Some(name)),
            Component::CurDir | Component::ParentDir => Some(None),
        })
        .collect::<Option<Vec<_>>>()
        .map(|names| (root, names))
}

/// The target a link names, read without following it: a symlink's text, or on Windows a
/// junction's or symbolic link's substitute name.
pub fn link_target(path: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    std::fs::read_link(path)
}

/// Whether a name can reach an entry spelled differently, as case-insensitive Windows and
/// macOS volumes allow; path-returning interfaces then also check the exact spelling.
pub const ALIASED_NAMES: bool = cfg!(any(windows, target_os = "macos"));

/// The type of a directory entry, read without following it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    File,
    Directory,
    Symlink,
    /// FIFOs, sockets, devices, and on Windows reparse points that are not links.
    Other,
}
