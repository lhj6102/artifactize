//! Native path normalization and portable declaration-path checks.

use std::{
    io,
    path::{Component, Path, PathBuf},
};

pub(crate) fn canonical_target(path: &Path) -> io::Result<PathBuf> {
    let mut resolved = PathBuf::new();
    for component in std::path::absolute(path)?.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            // A Windows prefix alone, such as `\\?\C:`, names the volume device, not a folder.
            Component::Prefix(_) | Component::RootDir => resolved.push(component),
            Component::Normal(_) => {
                resolved.push(component);
                match resolved.symlink_metadata() {
                    Ok(_) => resolved = super::canonicalize(&resolved)?,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
        }
    }
    Ok(resolved)
}

/// Why `absolute_without_links` refused a path.
#[derive(Debug)]
pub(crate) enum LinkedPath {
    /// The path climbs with `..`.
    ParentTraversal,
    /// This existing component is a link or another special entry.
    Link(PathBuf),
    Io(io::Error),
}

/// The absolute form of `path`, checked component by component below the system's root
/// (and on Windows the volume prefix): no `..`, and no existing component that is a link,
/// a reparse point or another special entry. Missing components are allowed.
pub(crate) fn absolute_without_links(path: &Path) -> Result<PathBuf, LinkedPath> {
    let absolute = std::path::absolute(path).map_err(LinkedPath::Io)?;
    let mut current = PathBuf::new();
    for component in absolute.components() {
        if component == Component::ParentDir {
            return Err(LinkedPath::ParentTraversal);
        }
        current.push(component);
        // A Windows prefix alone, such as `\\?\C:`, names the volume device, not a folder.
        if matches!(component, Component::Prefix(_) | Component::RootDir) {
            continue;
        }
        match super::path_kind(&current) {
            Ok(super::FileKind::Symlink | super::FileKind::Other) => {
                return Err(LinkedPath::Link(current));
            }
            Err(error) if error.kind() != io::ErrorKind::NotFound => {
                return Err(LinkedPath::Io(error));
            }
            _ => {}
        }
    }
    Ok(current)
}

/// Reject drive-rooted logical paths even when declarations are read on another OS.
pub(crate) fn drive_rooted(value: &str) -> bool {
    value
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphabetic)
        && value.as_bytes().get(1) == Some(&b':')
        && value.as_bytes().get(2) == Some(&b'/')
}
