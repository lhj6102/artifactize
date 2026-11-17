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

/// Portable keys for maps whose paths identify files, such as workbench optimistic bases.
pub(crate) mod map {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::{collections::BTreeMap, path::PathBuf};
    pub(crate) fn serialize<S: Serializer, T: Serialize>(
        paths: &BTreeMap<PathBuf, T>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        paths
            .iter()
            .map(|(path, value)| (super::super::path_text(path), value))
            .collect::<BTreeMap<_, _>>()
            .serialize(serializer)
    }
    pub(crate) fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
        deserializer: D,
    ) -> Result<BTreeMap<PathBuf, T>, D::Error> {
        BTreeMap::deserialize(deserializer)
    }
}
/// Migration source/destination pairs, rendered identically on every OS.
pub(crate) fn pairs<S: Serializer>(
    paths: &[(PathBuf, PathBuf)],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::Serialize;
    paths
        .iter()
        .map(|(from, to)| (super::path_text(from), super::path_text(to)))
        .collect::<Vec<_>>()
        .serialize(serializer)
}
