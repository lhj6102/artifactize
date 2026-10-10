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

/// Reject drive-rooted logical paths even when declarations are read on another OS.
pub(crate) fn drive_rooted(value: &str) -> bool {
    value
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphabetic)
        && value.as_bytes().get(1) == Some(&b':')
        && value.as_bytes().get(2) == Some(&b'/')
}

/// Inspect an entry through its opened parent, including Windows reparse points.
pub(crate) fn entry_kind(path: &Path) -> io::Result<super::FileKind> {
    let absolute = std::path::absolute(path)?;
    let Some(name) = absolute.file_name() else {
        return super::open_directory(&absolute).map(|_| super::FileKind::Directory);
    };
    let parent = super::open_directory(absolute.parent().expect("entry has a parent"))?;
    super::entry_kind(&parent, name)
}
