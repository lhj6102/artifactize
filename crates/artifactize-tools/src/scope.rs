//! Logical scope data and path resolution independent of project configuration.

use std::{
    borrow::Borrow,
    collections::BTreeMap,
    fmt,
    fs::{self, File},
    io,
    path::{Component, Path, PathBuf},
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
    pub children: BTreeMap<String, ArtifactId>,
    /// Mount alias to the mounted Artifact.
    pub mounts: BTreeMap<String, ArtifactId>,
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
        open_input(root, artifact, path).map_err(|error| ScopeError(error.to_string()))?;
        if artifact.file_name().is_some() && !resolved.is_file() {
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
    if let Some(first) = path.split('/').next().filter(|part| !part.is_empty())
        && artifact
            .mounts
            .keys()
            .any(|alias| alias.eq_ignore_ascii_case(first))
        && !platform::case_sensitive(&owner).map_err(|error| ScopeError(error.to_string()))?
    {
        return Err(
            ScopeError("Physical input conflicts with a logical mount name.".into()).into(),
        );
    }
    // Keep the owner pinned between checking its namespace and opening each component.
    let mut file = owner;
    for component in path.split('/').filter(|part| !part.is_empty()) {
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
    let target = root.join(path);
    // `/`, or on Windows the volume or share root such as `C:\`.
    let filesystem_root = target.ancestors().last().unwrap_or(&target);
    let mut directory =
        platform::open_directory(filesystem_root).map_err(|e| ScopeError(e.to_string()))?;
    // Host-selected roots may use system aliases (Windows TEMP commonly uses 8.3 names).
    // Logical components below that trusted root must use exact entry spellings. Both
    // walks stay descriptor-relative and refuse links in every component.
    let components = root.components().map(|component| (component, false)).chain(
        Path::new(path)
            .components()
            .map(|component| (component, true)),
    );
    for (component, exact) in components {
        directory = match component {
            Component::Prefix(_) | Component::RootDir => continue,
            Component::Normal(name) => open_named(&directory, name, exact)?,
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
            ScopeError("Cannot open Artifact input without symlink traversal.".into()).into()
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

/// Resolve a physical input below a canonical root. No component may be a symlink.
pub fn scoped_path(root: &Path, path: &Path) -> Result<PathBuf, ScopeError> {
    let path = path
        .to_str()
        .ok_or_else(|| ScopeError("Artifact paths must be UTF-8.".into()))?;
    if path.contains(['\0', '\\'])
        || Path::new(path).is_absolute()
        || path.split('/').any(|part| part == "." || part == "..")
    {
        return Err(ScopeError(
            "Artifact path must be relative to its declared root.".into(),
        ));
    }
    let mut target = root.to_owned();
    for component in path.split('/').filter(|part| !part.is_empty()) {
        target.push(component);
        let metadata = fs::symlink_metadata(&target)
            .map_err(|error| ScopeError(format!("{}: {error}", target.display())))?;
        if metadata.is_symlink() {
            return Err(ScopeError("Artifact symlinks are not supported.".into()));
        }
    }
    // The path-returning interface must enforce the same exact spelling as pinned reads;
    // otherwise a Human tool or explicit fingerprint input could bypass logical ownership.
    #[cfg(any(windows, target_os = "macos"))]
    open_scoped(root, path).map_err(|error| ScopeError(error.to_string()))?;
    let actual = platform::canonicalize(&target)
        .map_err(|error| ScopeError(format!("{}: {error}", target.display())))?;
    if !actual.starts_with(root) {
        return Err(ScopeError(
            "Artifact path escapes its declared root.".into(),
        ));
    }
    let metadata = fs::metadata(&actual)
        .map_err(|error| ScopeError(format!("{}: {error}", actual.display())))?;
    if !metadata.is_file() && !metadata.is_dir() {
        return Err(ScopeError(
            "Artifact input must be a file or directory.".into(),
        ));
    }
    Ok(actual)
}
