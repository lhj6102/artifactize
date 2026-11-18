//! Native path normalization and portable declaration-path checks.

use std::{
    ffi::OsStr,
    fs::File,
    io,
    path::{Component, Path, PathBuf},
};

/// Whether the target exists through any links or aliases. A dangling link is absent;
/// other filesystem errors leave the target's existence unknown.
pub(crate) fn target_exists(path: &Path) -> io::Result<bool> {
    path.try_exists()
}

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

/// Whether a prospective directory can be reused, without following links or resolving
/// another spelling on case-insensitive filesystems. Refuse case variants on all volumes so
/// a project migrated on a case-sensitive volume can also run on a case-insensitive one.
pub(crate) fn reusable_directory(parent: &Path, name: &str) -> io::Result<bool> {
    let parent = super::open_directory(parent)?;
    let name = OsStr::new(name);
    let mut found = false;
    for entry in super::read_dir(&parent)? {
        let entry = entry?;
        let actual = entry.file_name();
        if actual == name && entry.file_type()? == super::FileKind::Directory {
            found = true;
        } else if actual.eq_ignore_ascii_case(name) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "The destination must be a real directory with the exact name.",
            ));
        }
    }
    Ok(found)
}

/// Remove a regular file, refusing links and other special entries. The unlink itself never
/// follows the final component, including if the entry changes after the type probe.
pub(crate) fn remove_regular(path: &Path) -> io::Result<()> {
    match super::path_kind(path)? {
        super::FileKind::File => std::fs::remove_file(path),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        )),
    }
}

/// Open a regular file by name in a pinned directory, refusing links and reparse points.
pub(crate) fn open_regular_entry(directory: &File, name: &OsStr) -> io::Result<File> {
    let file = open_named(directory, name)?;
    if file.metadata()?.is_file() {
        Ok(file)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ))
    }
}

/// Open a file and its pinned parent within a trusted root. Resolve directory aliases for
/// containment, then walk from the physical filesystem root without following any links.
pub(crate) fn open_regular_beneath(root: &Path, path: &Path) -> io::Result<(File, File)> {
    let root = super::canonicalize(root)?;
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(io::ErrorKind::InvalidInput.into());
    };
    let parent = super::canonicalize(parent)?;
    let ancestor = parent
        .ancestors()
        .find(|ancestor| super::paths_equal(ancestor, &root))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Runtime log is outside saved Run output.",
            )
        })?;
    let relative = parent.strip_prefix(ancestor).expect("matching ancestor");
    let (volume, names) =
        artifactize_tools::files::split_root(&root).ok_or(io::ErrorKind::InvalidInput)?;
    let mut directory = names
        .into_iter()
        .try_fold(super::open_directory(volume)?, |directory, name| {
            open_named(&directory, name)
        })?;
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(io::ErrorKind::InvalidInput.into());
        };
        directory = open_named(&directory, name)?;
    }
    let file = open_regular_entry(&directory, name)?;
    Ok((directory, file))
}

/// Open a single native filename without following it.
pub(crate) fn open_named(directory: &File, name: &OsStr) -> io::Result<File> {
    let name = super::EntryName::new(name).ok_or(io::ErrorKind::InvalidInput)?;
    super::open_entry(directory, &name)
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

/// Definition names are portable filenames even when authored on a permissive volume.
pub(crate) fn portable_filename(name: &str) -> Result<(), String> {
    let lower = name.to_ascii_lowercase();
    if matches!(lower.as_str(), "con" | "prn" | "aux" | "nul" | "clock$")
        || lower
            .strip_prefix("com")
            .or_else(|| lower.strip_prefix("lpt"))
            .is_some_and(|number| {
                matches!(number, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
    {
        Err(format!("Artifact name {name:?} is a reserved device name."))
    } else {
        Ok(())
    }
}

/// Restore bytes through a regular-file handle, refusing links and reparse points.
pub(crate) fn restore_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;

    super::program::reject_link_ancestors(path).map_err(io::Error::other)?;
    let mut file = super::open_no_follow(
        File::options().write(true).create(true).truncate(false),
        path,
    )?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    file.set_len(0)?;
    file.write_all(bytes)
}

/// Physical directory identity and a usable path for display and subsequent lookups.
pub(crate) fn directory_identity(path: &Path) -> io::Result<(super::FileIdentity, PathBuf)> {
    let canonical = super::canonicalize(path)?;
    let identity = super::file_identity(&super::open_directory(&canonical)?)?;
    Ok((identity, canonical))
}

/// Publish a file snapshot while a reader may still hold the old file open.
/// On Windows std's rename supports that case, unlike tempfile's MoveFileEx persistence.
pub(crate) fn replace_open_file(
    file: tempfile::NamedTempFile,
    destination: &Path,
) -> io::Result<()> {
    let temporary = file.into_temp_path().keep().map_err(|error| error.error)?;
    if let Err(error) = std::fs::rename(&temporary, destination) {
        let _ = std::fs::remove_file(temporary);
        return Err(error);
    }
    Ok(())
}

/// Try creating and removing a directory at the closest existing state ancestor.
pub(crate) fn probe_writable(state: &Path) -> io::Result<()> {
    let parent = state
        .ancestors()
        .find_map(|path| match path.try_exists() {
            Ok(true) => Some(Ok(path)),
            Ok(false) => None,
            Err(error) => Some(Err(error)),
        })
        .ok_or_else(|| io::Error::other("No existing state directory ancestor."))??;
    if !std::fs::metadata(parent)?.is_dir() {
        return Err(io::Error::other(
            "State directory or its ancestor is not a directory.",
        ));
    }
    tempfile::Builder::new()
        .prefix(".artifactize-doctor-")
        .tempdir_in(parent)?
        .close()
}

/// Marker names are case-exact on every host, independent of volume lookup behavior.
pub(crate) fn has_exact_entry(directory: &Path, name: &str) -> io::Result<bool> {
    match std::fs::read_dir(directory) {
        Ok(entries) => {
            for entry in entries {
                if entry?.file_name() == name {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            // A search-only ancestor permits probing a fixed name but not listing it.
            // If the volume aliases that spelling, conservatively treat it as a marker:
            // this check guards writes, and exact case cannot be observed by enumeration.
            match std::fs::symlink_metadata(directory.join(name)) {
                Ok(_) => Ok(true),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
                Err(error) => Err(error),
            }
        }
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

/// Stable namespace spelling even before the leaf exists. Resolve existing aliases first,
/// then normalize case on volumes which do not distinguish it.
pub(crate) fn ipc_identity(path: &Path) -> io::Result<Vec<u8>> {
    let path = canonical_target(path)?;
    let sensitive = path
        .ancestors()
        .find_map(|ancestor| {
            super::open_directory(ancestor)
                .ok()
                .filter(|file| file.metadata().is_ok_and(|metadata| metadata.is_dir()))
                .and_then(|file| artifactize_tools::files::case_sensitive(&file).ok())
        })
        .unwrap_or(!super::ENV_NAMES_IGNORE_CASE);
    let raw = path.as_os_str();
    Ok(if sensitive {
        raw.as_encoded_bytes().to_vec()
    } else {
        raw.as_encoded_bytes()
            .utf8_chunks()
            .flat_map(|chunk| {
                // Avoid expanding one character into several, which can merge distinct names.
                chunk
                    .valid()
                    .chars()
                    .map(|character| {
                        let mut upper = character.to_uppercase();
                        let first = upper.next().unwrap_or(character);
                        if upper.next().is_some() {
                            character
                        } else {
                            first
                        }
                    })
                    .collect::<String>()
                    .into_bytes()
                    .into_iter()
                    .chain(chunk.invalid().iter().copied())
            })
            .collect()
    })
}

/// Relative path below an ancestor, using volume identity for alias-equivalent ancestor names.
pub(crate) fn relative_beneath(path: &Path, root: &Path) -> Option<PathBuf> {
    let path = canonical_target(path).unwrap_or_else(|_| path.to_owned());
    let root = canonical_target(root).unwrap_or_else(|_| root.to_owned());
    let ancestor = path
        .ancestors()
        .find(|candidate| super::paths_equal(candidate, &root))?;
    path.strip_prefix(ancestor).ok().map(Path::to_owned)
}
