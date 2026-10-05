//! Workspace paths and external state/output safety checks.

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
                    Ok(_) => resolved = crate::platform::canonicalize(&resolved)?,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
        }
    }
    Ok(resolved)
}

pub(crate) fn outside_workspace(workspace: &Path, output: &Path) -> io::Result<()> {
    if output.starts_with(workspace) {
        return Err(io::Error::other(
            "State and output directories must be outside the reviewed repository.",
        ));
    }
    Ok(())
}

pub(crate) fn prepare_directory(path: &Path, workspace: &Path) -> io::Result<PathBuf> {
    let path = canonical_target(path)?;
    outside_workspace(workspace, &path)?;
    crate::platform::create_private_dir_all(&path)?;
    let path = crate::platform::canonicalize(&path)?;
    outside_workspace(workspace, &path)?;
    Ok(path)
}
