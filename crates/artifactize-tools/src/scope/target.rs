//! A target is a filesystem path or an opaque, validated Live address; never both.
use super::{LogicalPath, ScopeError};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    borrow::Cow,
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

/// Kind and local selection travel with the target, so a Live URI cannot be a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Folder(PathBuf),
    File(PathBuf),
    Aspect {
        path: PathBuf,
        select: Vec<LogicalPath>,
    },
    /// Historical saved graphs and the migration reader only.
    Graph(PathBuf),
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
            Self::Folder(path) | Self::Graph(path) | Self::Aspect { path, .. } => path,
            Self::File(path) => path.parent().unwrap_or(Path::new("")),
            Self::Live(_) => Path::new(""),
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
        if let Self::Live(_) = self {
            return if kind == ArtifactKind::Live {
                Ok(self)
            } else {
                Err(ScopeError(
                    "A Live URI cannot have a filesystem kind.".into(),
                ))
            };
        }
        let path = self.local().expect("local variant").to_owned();
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
impl From<PathBuf> for Target {
    fn from(path: PathBuf) -> Self {
        Self::Folder(path)
    }
}
impl From<&str> for Target {
    fn from(path: &str) -> Self {
        Self::Folder(path.into())
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
            crate::files::parse_target_path(&value)
                .map(Self::Folder)
                .map_err(serde::de::Error::custom)
        }
    }
}
impl crate::files::PathText for Target {
    fn path_text(&self) -> String {
        self.to_string()
    }
}
impl PartialEq<PathBuf> for Target {
    fn eq(&self, other: &PathBuf) -> bool {
        self.local().is_some_and(|path| path == other)
    }
}
