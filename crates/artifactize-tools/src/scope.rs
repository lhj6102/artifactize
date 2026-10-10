//! Logical scope data and path resolution independent of project configuration.

use std::{
    borrow::Borrow,
    collections::BTreeMap,
    fmt,
    fs::File,
    io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::files as platform;

/// Bound logical tool paths in UTF-16 units, matching JSON Schema string limits
/// across supported clients without tying scoped paths to an OS-specific PATH_MAX.
pub const MAX_PATH_UNITS: usize = 4096;

/// Bound Artifact IDs like artifactize's Artifact names, so they stay short in tool output.
pub const MAX_ARTIFACT_ID_BYTES: usize = 64;

#[derive(Debug, Error)]
#[error("{0}")]
pub struct ScopeError(pub String);

/// An Artifact's ID: `[A-Za-z0-9][A-Za-z0-9_-]{0,63}`, so it can never smuggle a path
/// separator, a dot component, or control text into a scope key or a tool result.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ArtifactId(String);

impl ArtifactId {
    pub fn new(value: impl Into<String>) -> Result<Self, ScopeError> {
        let value = value.into();
        let valid = value.len() <= MAX_ARTIFACT_ID_BYTES
            && value
                .bytes()
                .next()
                .is_some_and(|first| first.is_ascii_alphanumeric())
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'));
        if valid {
            Ok(Self(value))
        } else {
            Err(ScopeError(format!(
                "Artifact ID must match [A-Za-z0-9][A-Za-z0-9_-]{{0,63}}: {value:?}"
            )))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Lets scope maps be looked up by a borrowed name; the derived order matches `str`'s.
impl Borrow<str> for ArtifactId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ArtifactId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl PartialEq<str> for ArtifactId {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for ArtifactId {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl PartialEq<String> for ArtifactId {
    fn eq(&self, other: &String) -> bool {
        self.0 == *other
    }
}

/// A safe relative logical path below an Artifact: `/`-separated plain names, such as a
/// child Artifact's folder prefix. It holds no empty, `.` or `..` component.
macro_rules! logical_name {
    ($name:ident, $what:literal, $valid:expr) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, ScopeError> {
                let value = value.into();
                if !value.is_empty() && logical_path(&value).is_ok() && ($valid)(&value) {
                    Ok(Self(value))
                } else {
                    Err(ScopeError(format!(
                        concat!("Invalid ", $what, ": {:?}"),
                        value
                    )))
                }
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Borrow<str> for $name {
            fn borrow(&self) -> &str {
                &self.0
            }
        }

        impl std::ops::Deref for $name {
            type Target = str;
            fn deref(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

logical_name!(ChildPrefix, "child folder prefix", |_: &str| true);
// A mount alias is one name: the first component of the paths it serves.
logical_name!(MountAlias, "mount alias", |value: &str| !value
    .contains('/'));

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ArtifactKind {
    Folder,
    File,
}

/// Access data computed by the caller, without declarations, evals, or owner commands.
#[derive(Debug, Clone)]
pub struct Artifact {
    pub path: PathBuf,
    pub kind: ArtifactKind,
    pub name: String,
    /// Logical path prefix to the child Artifact that owns it.
    pub children: BTreeMap<ChildPrefix, ArtifactId>,
    /// Mount alias to the mounted Artifact.
    pub mounts: BTreeMap<MountAlias, ArtifactId>,
}

impl Artifact {
    pub fn folder(&self) -> &Path {
        match self.kind {
            ArtifactKind::Folder => &self.path,
            ArtifactKind::File => self.path.parent().unwrap_or(Path::new("")),
        }
    }

    pub fn file_name(&self) -> Option<&str> {
        (self.kind == ArtifactKind::File)
            .then(|| self.path.file_name().and_then(|name| name.to_str()))
            .flatten()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedPath {
    pub artifact_id: ArtifactId,
    pub path: String,
}

#[derive(Debug)]
pub struct Scope {
    pub artifacts: BTreeMap<ArtifactId, Artifact>,
}

impl Scope {
    /// Pure logical resolution. Each mount or child hop consumes path components.
    pub fn resolve_path(
        &self,
        artifact_id: &ArtifactId,
        path: &str,
    ) -> Result<ScopedPath, ScopeError> {
        logical_path(path)?;
        let mut current = artifact_id;
        let mut remaining = path;
        loop {
            let artifact = self
                .artifacts
                .get(current)
                .ok_or_else(|| ScopeError(format!("Artifact is outside this review: {current}")))?;
            if remaining.is_empty() {
                break;
            }
            let (first, rest) = remaining.split_once('/').unwrap_or((remaining, ""));
            if let Some(target) = artifact.mounts.get(first) {
                current = target;
                remaining = rest;
                continue;
            }
            if let Some((prefix, target)) = artifact.children.iter().find(|(prefix, _)| {
                remaining == prefix.as_str()
                    || remaining
                        .strip_prefix(prefix.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
            }) {
                current = target;
                remaining = remaining[prefix.len()..].strip_prefix('/').unwrap_or("");
                continue;
            }
            break;
        }
        let artifact = &self.artifacts[current];
        file_input(artifact, remaining)?;
        Ok(ScopedPath {
            artifact_id: current.clone(),
            path: remaining.to_owned(),
        })
    }

    /// Existing file or directory, without traversing links even inside the owner.
    pub fn resolve_input(
        &self,
        root: &Path,
        artifact_id: &ArtifactId,
        path: &str,
    ) -> Result<PathBuf, ScopeError> {
        let location = self.resolve_path(artifact_id, path)?;
        let artifact = &self.artifacts[&location.artifact_id];
        let owner = scoped_path(root, artifact.folder())?;
        let path = if location.path.is_empty() {
            artifact.file_name().unwrap_or("")
        } else {
            &location.path
        };
        let resolved = scoped_path(&owner, Path::new(path))?;
        let file =
            open_input(root, artifact, path).map_err(|error| ScopeError(error.to_string()))?;
        let regular = file
            .metadata()
            .map_err(|error| ScopeError(error.to_string()))?
            .is_file();
        if artifact.file_name().is_some() && !regular {
            return Err(ScopeError(
                "File Artifact target must remain a regular file.".into(),
            ));
        }
        Ok(resolved)
    }
}

/// Why a scoped open failed. A missing entry stays distinct, so a caller that knows the
/// logical path the user asked for can name it.
#[derive(Debug, Error)]
pub enum OpenError {
    #[error("No such file or directory.")]
    NotFound,
    /// A component is a link, which scoped access never follows.
    #[error("Cannot open Artifact input without symlink traversal.")]
    Link,
    #[error(transparent)]
    Refused(#[from] ScopeError),
}

/// Open each component relative to its pinned parent, so replacement cannot redirect a read
/// through a link.
pub fn open_input(root: &Path, artifact: &Artifact, path: &str) -> Result<File, OpenError> {
    if !root.is_absolute() || artifact.path.is_absolute() {
        return Err(
            ScopeError("Artifact roots must be absolute and owner paths relative.".into()).into(),
        );
    }
    logical_path(path)?;
    file_input(artifact, path)?;
    let owner = open_scoped(&root.join(artifact.folder()), "")?;
    // Keep the owner pinned between checking its namespace and opening each component.
    // Check the parent of each reserved component: a nested APFS mount can have a different
    // case policy from its owner volume. Exact-name checks additionally reject OS aliases.
    let components: Vec<_> = path.split('/').filter(|part| !part.is_empty()).collect();
    let mut file = owner;
    for (depth, component) in components.iter().enumerate() {
        let mount = depth == 0
            && artifact
                .mounts
                .keys()
                .any(|alias| alias.eq_ignore_ascii_case(component));
        let child = artifact.children.keys().any(|prefix| {
            let prefix: Vec<_> = prefix.split('/').collect();
            prefix.len() == depth + 1
                && prefix
                    .iter()
                    .zip(&components)
                    .all(|(left, right)| left.eq_ignore_ascii_case(right))
        });
        if (mount || child)
            && !platform::case_sensitive(&file).map_err(|error| ScopeError(error.to_string()))?
        {
            return Err(ScopeError(
                "Physical input conflicts with a child or logical mount name.".into(),
            )
            .into());
        }
        file = open_child(&file, std::ffi::OsStr::new(component))?;
    }
    if artifact.file_name().is_some()
        && !path.is_empty()
        && !file
            .metadata()
            .map_err(|e| ScopeError(e.to_string()))?
            .is_file()
    {
        return Err(ScopeError("File Artifact target must remain a regular file.".into()).into());
    }
    Ok(file)
}

/// A file Artifact exposes a virtual root containing only its target and mounts.
fn file_input(artifact: &Artifact, path: &str) -> Result<(), ScopeError> {
    if let Some(target) = artifact.file_name()
        && !path.is_empty()
        && path != target
    {
        return Err(ScopeError(format!(
            "File Artifact {} exposes only its target {target} and mounts.",
            artifact.name
        )));
    }
    Ok(())
}

/// Open a relative path below an absolute root without following any symlink components.
pub fn open_scoped(root: &Path, path: &str) -> Result<File, OpenError> {
    logical_path(path)?;
    if !root.is_absolute() {
        return Err(ScopeError("Scoped roots must be absolute.".into()).into());
    }
    let (filesystem_root, names) = platform::split_root(root)
        .ok_or_else(|| ScopeError("Artifact path must not traverse parent directories.".into()))?;
    let mut directory =
        platform::open_directory(filesystem_root).map_err(|e| ScopeError(e.to_string()))?;
    // Host-selected roots may use system aliases, such as short names. Logical components
    // below that trusted root must use exact entry spellings. Both walks stay
    // descriptor-relative and refuse links in every component.
    let components = names.into_iter().map(|name| (name, false)).chain(
        path.split('/')
            .filter(|part| !part.is_empty())
            .map(|part| (std::ffi::OsStr::new(part), true)),
    );
    for (name, exact) in components {
        directory = match name {
            name if name != ".." && name != "." => open_named(&directory, name, exact)?,
            _ => {
                return Err(ScopeError(
                    "Artifact path must not traverse parent directories.".into(),
                )
                .into());
            }
        };
    }
    Ok(directory)
}

/// Open one entry of a pinned directory without following a symlink.
pub fn open_child(directory: &File, name: &std::ffi::OsStr) -> Result<File, OpenError> {
    open_named(directory, name, true)
}

fn open_named(directory: &File, name: &std::ffi::OsStr, exact: bool) -> Result<File, OpenError> {
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
    if exact && !platform::exact_name(&file, name).map_err(|error| ScopeError(error.to_string()))? {
        return Err(
            ScopeError("Artifact path must use the exact directory entry name.".into()).into(),
        );
    }
    let metadata = file.metadata().map_err(|e| ScopeError(e.to_string()))?;
    if !metadata.is_file() && !metadata.is_dir() {
        return Err(
            ScopeError("Artifact input must be a regular file or directory.".into()).into(),
        );
    }
    Ok(file)
}

pub fn logical_path(path: &str) -> Result<(), ScopeError> {
    if path.encode_utf16().count() > MAX_PATH_UNITS
        || path
            .bytes()
            .any(|byte| byte.is_ascii_control() || matches!(byte, b'\\' | b':'))
        || (!path.is_empty()
            && path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == ".."))
    {
        return Err(ScopeError(
            "Artifact path must be a safe relative logical path.".into(),
        ));
    }
    Ok(())
}

/// The logical form of a native relative path: its components joined with `/`, whatever
/// separator the platform uses. `None` when a component is not UTF-8 or not a plain name.
pub fn logical_from_native(path: &Path) -> Option<String> {
    platform::logical_from_native(path)
}

/// Resolve a physical input below a canonical root. No component may be a symlink.
pub fn scoped_path(root: &Path, path: &Path) -> Result<PathBuf, ScopeError> {
    let relative =
        platform::scoped_relative(path).map_err(|error| ScopeError(error.to_string()))?;
    // The pinned, no-follow walk decides: it refuses links, special files and spellings
    // other than the directory entry's own, so the returned path names what it opened.
    match open_scoped(root, &relative) {
        Ok(_) => Ok(relative
            .split('/')
            .filter(|part| !part.is_empty())
            .fold(root.to_owned(), |path, part| path.join(part))),
        Err(OpenError::NotFound) => Err(ScopeError(format!(
            "Artifact input does not exist: {relative}"
        ))),
        Err(OpenError::Link) => Err(ScopeError("Artifact symlinks are not supported.".into())),
        Err(OpenError::Refused(error)) => Err(error),
    }
}
