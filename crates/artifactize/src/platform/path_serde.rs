//! Serde for paths saved or shown as text: written with `/` separators on every system
//! (`path_text`), read back as native paths, which Windows accepts with `/`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serializer};

pub(crate) fn serialize<S: Serializer>(path: &Path, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&super::path_text(path))
}

pub(crate) fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<PathBuf, D::Error> {
    PathBuf::deserialize(deserializer)
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
        Option::<PathBuf>::deserialize(deserializer)
    }
}
