//! Open validated logical inputs through pinned parent directories, never through links.

use std::{
    ffi::OsStr,
    fs::File,
    io,
    path::{Path, PathBuf},
};
use thiserror::Error;

use super::{Artifact, InputPath, Location, ScopeError, file_input};
use crate::files as platform;

/// Why a scoped open failed. Missing entries stay distinct so callers can name the input.
#[derive(Debug, Error)]
pub enum OpenError {
    #[error("No such file or directory.")]
    NotFound,
    #[error("Cannot open Artifact input without symlink traversal.")]
    Link,
    #[error(transparent)]
    Refused(#[from] ScopeError),
}

impl Location<'_, '_> {
    pub(crate) fn open(&self, root: &Path) -> Result<File, OpenError> {
        open_owned(root, self.artifact, self.path).map(|opened| opened.file)
    }

    /// The virtual root of a file Artifact names its one file for direct text reads and open.
    pub(crate) fn direct_path(&self) -> Result<InputPath<&str>, ScopeError> {
        match self
            .artifact
            .file_name()
            .filter(|_| self.path.as_str().is_empty())
        {
            Some(name) => InputPath::parse(name),
            None => Ok(self.path),
        }
    }

    pub(crate) fn open_direct(&self, root: &Path) -> Result<(InputPath<&str>, File), OpenError> {
        let path = self.direct_path()?;
        open_owned(root, self.artifact, path).map(|opened| (path, opened.file))
    }

    pub(crate) fn native_path(&self, root: &Path) -> Result<PathBuf, ScopeError> {
        let path = self.direct_path()?;
        native_path(open_owned(root, self.artifact, path), root, path.as_str())
    }
}

/// Open each component relative to its pinned parent, so replacement cannot redirect a read
/// through a link.
pub fn open_input(root: &Path, artifact: &Artifact, path: &str) -> Result<File, OpenError> {
    let path = InputPath::parse(path)?;
    file_input(artifact, path.as_str())?;
    open_owned(root, artifact, path).map(|opened| opened.file)
}

struct Opened<'a> {
    file: File,
    names: Vec<&'a OsStr>,
}

fn open_owned<'a>(
    root: &'a Path,
    artifact: &'a Artifact,
    path: InputPath<&'a str>,
) -> Result<Opened<'a>, OpenError> {
    let local = artifact
        .path
        .local()
        .ok_or_else(|| ScopeError("Live Artifacts have no filesystem inputs.".into()))?;
    // Validate the target as well as the containing directory. In particular, a rooted or
    // drive-relative owner must not become an independent root through native joining.
    platform::relative_names(local).map_err(io_scope)?;
    let owner = platform::relative_names(artifact.folder()).map_err(io_scope)?;
    let mut opened = open_root(root)?;
    for name in owner {
        opened.descend(name)?;
    }
    let mut reserved: Vec<_> = artifact
        .nested
        .keys()
        .map(|prefix| prefix.split('/').peekable())
        .collect();
    for name in path.names().map(OsStr::new) {
        let parent = opened.file;
        let file = open_child(&parent, name)?;
        // Compare reserved spellings through this pinned parent, whose filesystem may
        // apply a different case or normalization policy from the Artifact root.
        let mut descendants = Vec::new();
        for mut prefix in reserved {
            if let Some(candidate) = prefix.next()
                && platform::same_entry_name(&parent, name, OsStr::new(candidate))
                    .map_err(io_scope)?
            {
                if prefix.peek().is_none() {
                    return Err(
                        ScopeError("Physical input conflicts with a child name.".into()).into(),
                    );
                }
                descendants.push(prefix);
            }
        }
        reserved = descendants;
        opened.file = file;
        opened.names.push(name);
    }
    if artifact.file_name().is_some()
        && !path.as_str().is_empty()
        && !opened.file.metadata().map_err(io_scope)?.is_file()
    {
        return Err(ScopeError("File Artifact target must remain a regular file.".into()).into());
    }
    Ok(opened)
}

impl<'a> Opened<'a> {
    fn descend(&mut self, name: &'a OsStr) -> Result<(), OpenError> {
        self.file = open_child(&self.file, name)?;
        self.names.push(name);
        Ok(())
    }
}

fn open_root(root: &Path) -> Result<Opened<'_>, OpenError> {
    if !platform::is_absolute(root) {
        return Err(ScopeError("Scoped roots must be absolute.".into()).into());
    }
    let (filesystem_root, names) = platform::split_root(root)
        .ok_or_else(|| ScopeError("Artifact path must not traverse parent directories.".into()))?;
    let mut file = platform::open_directory(filesystem_root).map_err(io_scope)?;
    // The host-selected root may use system aliases. Exact names are required below it.
    for name in names {
        file = open_named(&file, name, false)?;
    }
    Ok(Opened {
        file,
        names: Vec::new(),
    })
}

/// Open a relative path below an absolute root without following any symlink components.
pub fn open_scoped(root: &Path, path: &str) -> Result<File, OpenError> {
    walk(root, InputPath::parse(path)?).map(|opened| opened.file)
}

fn walk<'a>(root: &'a Path, path: InputPath<&'a str>) -> Result<Opened<'a>, OpenError> {
    let mut opened = open_root(root)?;
    for name in path.names().map(OsStr::new) {
        opened.descend(name)?;
    }
    Ok(opened)
}

/// Open one entry of a pinned directory without following a symlink.
pub fn open_child(directory: &File, name: &OsStr) -> Result<File, OpenError> {
    open_named(directory, name, true)
}

fn open_named(directory: &File, name: &OsStr, exact: bool) -> Result<File, OpenError> {
    let entry_name = platform::EntryName::new(name)
        .ok_or_else(|| ScopeError("Invalid Artifact path.".into()))?;
    let file = platform::open_entry(directory, &entry_name).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            OpenError::NotFound
        } else if platform::is_link_refusal(&error) {
            OpenError::Link
        } else {
            ScopeError(format!("Cannot open Artifact input: {error}")).into()
        }
    })?;
    if exact && !platform::exact_name(directory, &file, name).map_err(io_scope)? {
        return Err(
            ScopeError("Artifact path must use the exact directory entry name.".into()).into(),
        );
    }
    let metadata = file.metadata().map_err(io_scope)?;
    if !metadata.is_file() && !metadata.is_dir() {
        return Err(
            ScopeError("Artifact input must be a regular file or directory.".into()).into(),
        );
    }
    Ok(file)
}

/// Resolve a physical input below a canonical root. No component may be a symlink.
pub fn scoped_path(root: &Path, path: &Path) -> Result<PathBuf, ScopeError> {
    let relative = platform::scoped_relative(path).map_err(io_scope)?;
    native_path(
        walk(root, InputPath::parse(relative.as_str())?),
        root,
        &relative,
    )
}

fn native_path(
    opened: Result<Opened<'_>, OpenError>,
    root: &Path,
    path: &str,
) -> Result<PathBuf, ScopeError> {
    match opened {
        Ok(opened) => platform::verified_path(&opened.file, root, &opened.names).map_err(io_scope),
        Err(OpenError::NotFound) => {
            Err(ScopeError(format!("Artifact input does not exist: {path}")))
        }
        Err(OpenError::Link) => Err(ScopeError("Artifact symlinks are not supported.".into())),
        Err(OpenError::Refused(error)) => Err(error),
    }
}

fn io_scope(error: io::Error) -> ScopeError {
    ScopeError(error.to_string())
}
