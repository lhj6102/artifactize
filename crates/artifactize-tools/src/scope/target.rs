//! A target is a filesystem path or an opaque, validated Live address; never both.
use super::{LogicalPath, ScopeError};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    borrow::Cow,
    ffi::{OsStr, OsString},
    fmt,
    path::{Path, PathBuf},
};

// The declaration grammar distinguishes a URI by at least two scheme letters.
const MIN_SCHEME_BYTES: usize = 2;
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Uri(String);
impl Uri {
    pub fn has_scheme(value: &str) -> bool {
        value.split_once(':').is_some_and(|(scheme, _)| {
            scheme.len() >= MIN_SCHEME_BYTES
                && scheme.bytes().all(|byte| byte.is_ascii_alphabetic())
        })
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl std::str::FromStr for Uri {
    type Err = ScopeError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if !Self::has_scheme(value)
            || value.chars().any(|character| {
                character.is_control() || character.is_whitespace() || character == '\\'
            })
        {
            return Err(ScopeError("A Live URI needs a scheme of at least two letters and no whitespace, controls or backslashes.".into()));
        }
        Ok(Self(value.into()))
    }
}
impl fmt::Display for Uri {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}
impl Serialize for Uri {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}
impl<'de> Deserialize<'de> for Uri {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// A project-relative native path with verified logical entry names. Retain the names so
/// scoped opens can walk pinned directories without reparsing the trusted owner target.
#[derive(Debug, Clone)]
pub struct LocalTarget {
    path: PathBuf,
    names: Vec<OsString>,
}

impl TryFrom<PathBuf> for LocalTarget {
    type Error = ScopeError;

    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        let names = crate::files::relative_names(&path)
            .map_err(|error| ScopeError(error.to_string()))?
            .into_iter()
            .map(OsStr::to_owned)
            .collect();
        Ok(Self { path, names })
    }
}

/// Kind and local selection travel with the target, so a Live URI cannot be a folder.
#[derive(Debug, Clone)]
pub enum Target {
    Folder(LocalTarget),
    File(LocalTarget),
    Aspect {
        path: LocalTarget,
        select: Vec<LogicalPath>,
    },
    /// Historical saved graphs and the migration reader only.
    Graph(LocalTarget),
    Live(Uri),
}
impl Target {
    pub fn kind(&self) -> super::ArtifactKind {
        use super::ArtifactKind;
        match self {
            Self::Folder(_) => ArtifactKind::Folder,
            Self::File(_) => ArtifactKind::File,
            Self::Aspect { .. } => ArtifactKind::Aspect,
            Self::Graph(_) => ArtifactKind::Graph,
            Self::Live(_) => ArtifactKind::Live,
        }
    }
    pub fn local(&self) -> Option<&Path> {
        self.local_target().map(|local| local.path.as_path())
    }
    fn local_target(&self) -> Option<&LocalTarget> {
        match self {
            Self::Folder(path)
            | Self::File(path)
            | Self::Graph(path)
            | Self::Aspect { path, .. } => Some(path),
            Self::Live(_) => None,
        }
    }
    pub fn uri(&self) -> Option<&Uri> {
        match self {
            Self::Live(uri) => Some(uri),
            _ => None,
        }
    }
    pub fn select(&self) -> &[LogicalPath] {
        match self {
            Self::Aspect { select, .. } => select,
            _ => &[],
        }
    }
    pub fn folder(&self) -> &Path {
        match self {
            Self::Folder(local) | Self::Graph(local) | Self::Aspect { path: local, .. } => {
                &local.path
            }
            Self::File(local) => crate::files::parent(&local.path).unwrap_or(Path::new("")),
            Self::Live(_) => Path::new(""),
        }
    }
    pub(super) fn folder_names(&self) -> Option<&[OsString]> {
        match self {
            Self::File(local) => Some(local.names.split_last().map_or(&[], |(_, names)| names)),
            _ => self.local_target().map(|local| local.names.as_slice()),
        }
    }
    pub(super) fn file_name(&self) -> Option<&str> {
        match self {
            Self::File(local) => local.names.last()?.to_str(),
            _ => None,
        }
    }
    /// Join the separate fields of an existing local declaration or saved wire record.
    pub fn with_kind(
        self,
        kind: super::ArtifactKind,
        select: Vec<LogicalPath>,
    ) -> Result<Self, ScopeError> {
        use super::ArtifactKind;
        if kind != ArtifactKind::Aspect && !select.is_empty() {
            return Err(ScopeError(
                "Only an aspect target has selected paths.".into(),
            ));
        }
        let path = match self {
            Self::Folder(path)
            | Self::File(path)
            | Self::Graph(path)
            | Self::Aspect { path, .. } => path,
            Self::Live(_) if kind == ArtifactKind::Live => return Ok(self),
            Self::Live(_) => {
                return Err(ScopeError(
                    "A Live URI cannot have a filesystem kind.".into(),
                ));
            }
        };
        match kind {
            ArtifactKind::Folder => Ok(Self::Folder(path)),
            ArtifactKind::File => Ok(Self::File(path)),
            ArtifactKind::Aspect => Ok(Self::Aspect { path, select }),
            ArtifactKind::Graph => Ok(Self::Graph(path)),
            ArtifactKind::Live => Err(ScopeError("A Live target needs a URI.".into())),
        }
    }
    pub fn text(&self) -> Cow<'_, str> {
        match self {
            Self::Live(uri) => Cow::Borrowed(uri.as_str()),
            _ => Cow::Owned(crate::files::path_text(
                self.local().expect("local variant"),
            )),
        }
    }
}
impl TryFrom<PathBuf> for Target {
    type Error = ScopeError;

    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        LocalTarget::try_from(path).map(Self::Folder)
    }
}
impl TryFrom<&str> for Target {
    type Error = ScopeError;

    fn try_from(path: &str) -> Result<Self, Self::Error> {
        Self::try_from(PathBuf::from(path))
    }
}
impl fmt::Display for Target {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.text())
    }
}
impl Serialize for Target {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.text())
    }
}
impl<'de> Deserialize<'de> for Target {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        if Uri::has_scheme(&value) {
            value
                .parse()
                .map(Self::Live)
                .map_err(serde::de::Error::custom)
        } else {
            let path = crate::files::parse_target_path(&value).map_err(serde::de::Error::custom)?;
            Self::try_from(path).map_err(serde::de::Error::custom)
        }
    }
}
impl crate::files::PathText for Target {
    fn path_text(&self) -> String {
        self.to_string()
    }
}
impl PartialEq for Target {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Folder(first), Self::Folder(second))
            | (Self::File(first), Self::File(second))
            | (Self::Graph(first), Self::Graph(second)) => {
                crate::files::paths_equal(&first.path, &second.path)
            }
            (
                Self::Aspect {
                    path: first,
                    select,
                },
                Self::Aspect {
                    path: second,
                    select: other,
                },
            ) => crate::files::paths_equal(&first.path, &second.path) && select == other,
            (Self::Live(first), Self::Live(second)) => first == second,
            _ => false,
        }
    }
}
impl Eq for Target {}

impl PartialEq<PathBuf> for Target {
    fn eq(&self, other: &PathBuf) -> bool {
        self.local()
            .is_some_and(|path| crate::files::paths_equal(path, other))
    }
}
