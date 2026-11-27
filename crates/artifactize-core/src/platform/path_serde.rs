//! Serde for paths saved or shown as text: written with `/` separators on every system
//! (`path_text`, with a reversible encoding for names that are not Unicode), read back as the
//! same native paths (`path_from_text`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serializer};

pub(crate) fn serialize<S: Serializer>(path: &Path, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&super::path_text(path))
}

pub(crate) fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<PathBuf, D::Error> {
    let text = String::deserialize(deserializer)?;
    artifactize_tools::files::path_from_text(&text).map_err(serde::de::Error::custom)
}

/// The same for an optional path.
pub(crate) mod option {
    use std::path::PathBuf;

    use serde::{Deserialize, Deserializer, Serializer};

    pub(crate) fn serialize<S: Serializer>(
        path: &Option<PathBuf>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match path {
            Some(path) => serializer.serialize_some(&super::super::path_text(path)),
            None => serializer.serialize_none(),
        }
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<PathBuf>, D::Error> {
        Option::<String>::deserialize(deserializer)?
            .map(|text| {
                artifactize_tools::files::path_from_text(&text).map_err(serde::de::Error::custom)
            })
            .transpose()
    }
}

/// A path a started program uses as given: its native spelling, not `/` text. JSON carries only
/// Unicode, so a path that is not is refused rather than altered.
pub(crate) mod native {
    use std::path::{Path, PathBuf};

    use serde::{Serializer, ser::Error};

    pub(crate) fn serialize_path<S: Serializer>(
        path: &Path,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(text(path)?)
    }

    pub(crate) fn serialize<S: Serializer>(
        path: &Option<PathBuf>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match path {
            Some(path) => serializer.serialize_some(text(path)?),
            None => serializer.serialize_none(),
        }
    }

    fn text<E: Error>(path: &Path) -> Result<&str, E> {
        path.to_str()
            .ok_or_else(|| E::custom("A path given to a tool program must be Unicode text."))
    }
}

/// Files with a count each, such as the names a rename changed in them.
pub(crate) fn counts<S: Serializer>(
    files: &[(PathBuf, usize)],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::Serialize;
    files
        .iter()
        .map(|(path, count)| (super::path_text(path), count))
        .collect::<Vec<_>>()
        .serialize(serializer)
}

/// A file moved from one path to another, when one was.
pub(crate) fn optional_pair<S: Serializer>(
    pair: &Option<(PathBuf, PathBuf)>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::Serialize;
    pair.as_ref()
        .map(|(from, to)| (super::path_text(from), super::path_text(to)))
        .serialize(serializer)
}

