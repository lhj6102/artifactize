//! Logical scope data and path resolution independent of project configuration.

use std::{
    borrow::Borrow,
    collections::BTreeMap,
    fmt,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize, Serializer};
use thiserror::Error;

mod open;
pub use open::{OpenError, open_child, open_input, open_scoped, scoped_path};
mod logical;
pub use logical::LogicalPath;
mod target;
pub use target::{FileTarget, LocalTarget, Target, Uri};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ArtifactKind {
    Folder,
    File,
    /// Selected paths below the folder of its declaration.
    Aspect,
    Graph,
    Live,
}

/// Access data computed by the caller, without declarations, evals, or owner commands.
#[derive(Debug, Clone)]
pub struct Artifact {
    pub path: Target,
    pub name: String,
    /// Logical path prefix to the nested folder Artifact that owns it.
    pub nested: BTreeMap<ChildPrefix, ArtifactId>,
    /// A folder part whose target contains this entire target's containing folder.
    pub covering_part: Option<ArtifactId>,
}

impl Artifact {
    pub fn kind(&self) -> ArtifactKind {
        self.path.kind()
    }
    pub fn select(&self) -> &[LogicalPath] {
        self.path.select()
    }
    pub fn folder(&self) -> &Path {
        self.path.folder()
    }

    /// Whether only some paths below its folder are the Artifact's: a file Artifact's
    /// target or an aspect's selection, under a virtual root.
    pub fn selective(&self) -> bool {
        matches!(self.kind(), ArtifactKind::File | ArtifactKind::Aspect)
    }

    /// Whether the logical `path` below its folder is the Artifact's: always for a folder,
    /// and for an aspect a selected path, a path below one, or a folder leading to one.
    pub fn admits(&self, path: InputPath<&str>) -> bool {
        let path = path.as_str();
        if path.is_empty() {
            return true;
        }
        match self.kind() {
            ArtifactKind::Folder | ArtifactKind::Graph => true,
            ArtifactKind::Live => false,
            ArtifactKind::File => self.file_name() == Some(path),
            ArtifactKind::Aspect => self.select().iter().any(|selected| {
                let within = |outer: &str, inner: &str| {
                    inner
                        .strip_prefix(outer)
                        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
                };
                within(selected, path) || within(path, selected)
            }),
        }
    }

    pub fn file_name(&self) -> Option<&str> {
        self.path.file_name()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedPath {
    pub artifact_id: ArtifactId,
    pub path: InputPath,
}

#[derive(Debug)]
pub struct Scope {
    pub artifacts: BTreeMap<ArtifactId, Artifact>,
}

impl Scope {
    pub(crate) fn artifact(&self, id: &ArtifactId) -> Result<&Artifact, ScopeError> {
        self.artifacts
            .get(id)
            .ok_or_else(|| ScopeError(format!("Artifact is outside this review: {id}")))
    }

    /// Pure logical resolution. Each child hop consumes path components.
    pub fn resolve_path(
        &self,
        artifact_id: &ArtifactId,
        path: &str,
    ) -> Result<ScopedPath, ScopeError> {
        let location = self.locate(artifact_id, InputPath::parse(path)?)?;
        Ok(ScopedPath {
            artifact_id: location.artifact_id.clone(),
            path: location.path.owned(),
        })
    }

    pub(crate) fn locate<'a, 'p>(
        &'a self,
        artifact_id: &'a ArtifactId,
        path: InputPath<&'p str>,
    ) -> Result<Location<'a, 'p>, ScopeError> {
        let mut current = artifact_id;
        let mut remaining = path;
        loop {
            let artifact = self.artifact(current)?;
            if artifact.kind() == ArtifactKind::Live {
                return Err(ScopeError(
                    "Live Artifacts have no filesystem inputs.".into(),
                ));
            }
            if let Some(part) = &artifact.covering_part
                && !self.artifacts.contains_key(part)
            {
                return Err(ScopeError(format!(
                    "Artifact {current} is covered by folder part {part}; reference {{{part}}} to read its files."
                )));
            }
            if let Some((target, rest)) = artifact.nested.iter().find_map(|(prefix, target)| {
                remaining.after(prefix.as_str()).map(|rest| (target, rest))
            }) {
                current = target;
                remaining = rest;
                continue;
            }
            file_input(artifact, remaining)?;
            return Ok(Location {
                artifact_id: current,
                artifact,
                path: remaining,
            });
        }
    }

    /// Existing file or directory, without traversing links even inside the owner.
    pub fn resolve_input(
        &self,
        root: &Path,
        artifact_id: &ArtifactId,
        path: &str,
    ) -> Result<PathBuf, ScopeError> {
        self.locate(artifact_id, InputPath::parse(path)?)?
            .native_path(root)
    }
}

pub(crate) struct Location<'a, 'p> {
    pub artifact_id: &'a ArtifactId,
    pub artifact: &'a Artifact,
    pub path: InputPath<&'p str>,
}

/// A validated tool path, distinct from declaration selections. Empty denotes the root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct InputPath<T = String>(T);

impl<T: AsRef<str>> InputPath<T> {
    pub(crate) fn parse(path: T) -> Result<Self, ScopeError> {
        logical_path(path.as_ref())?;
        Ok(Self(path))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_ref()
    }

    pub(crate) fn borrowed(&self) -> InputPath<&str> {
        InputPath(self.as_str())
    }

    /// A native entry has already been checked as a single logical name by the platform.
    pub(crate) fn child(&self, name: &str) -> Option<InputPath<String>> {
        let path = if self.as_str().is_empty() {
            name.to_owned()
        } else {
            format!("{}/{name}", self.as_str())
        };
        (path.encode_utf16().count() <= MAX_PATH_UNITS).then_some(InputPath(path))
    }
}

impl std::str::FromStr for InputPath<String> {
    type Err = ScopeError;

    fn from_str(path: &str) -> Result<Self, Self::Err> {
        InputPath::parse(path).map(InputPath::owned)
    }
}

impl<T: AsRef<str>> fmt::Display for InputPath<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl<T: AsRef<str>> Serialize for InputPath<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<T: AsRef<str>> AsRef<Path> for InputPath<T> {
    fn as_ref(&self) -> &Path {
        Path::new(self.as_str())
    }
}

impl<'a> InputPath<&'a str> {
    pub(crate) fn into_str(self) -> &'a str {
        self.0
    }

    pub(crate) fn root() -> Self {
        Self("")
    }

    fn after(self, prefix: &str) -> Option<Self> {
        let suffix = self.0.strip_prefix(prefix)?;
        if suffix.is_empty() {
            Some(Self::root())
        } else {
            suffix.strip_prefix('/').map(Self)
        }
    }

    fn names(self) -> impl Iterator<Item = &'a str> {
        self.0.split('/').filter(|name| !name.is_empty())
    }

    pub(crate) fn owned(self) -> InputPath<String> {
        InputPath(self.0.to_owned())
    }
}

/// A file Artifact exposes a virtual root containing only its target, and an aspect one
/// containing only its selected paths.
fn file_input(artifact: &Artifact, path: InputPath<&str>) -> Result<(), ScopeError> {
    if artifact.admits(path) {
        return Ok(());
    }
    Err(ScopeError(match artifact.file_name() {
        Some(target) => format!(
            "File Artifact {} exposes only its target {target}.",
            artifact.name
        ),
        None => format!(
            "Aspect {} exposes only its selected paths: {}.",
            artifact.name,
            artifact.select().join(", ")
        ),
    }))
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
    crate::files::logical_from_native(path)
}
