//! Workspace paths and external state/output safety checks.

use std::{
    fs, io,
    os::unix::fs::DirBuilderExt,
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
            _ => {
                resolved.push(component);
                match resolved.symlink_metadata() {
                    Ok(_) => resolved = resolved.canonicalize()?,
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
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&path)?;
    let path = path.canonicalize()?;
    outside_workspace(workspace, &path)?;
    Ok(path)
}
