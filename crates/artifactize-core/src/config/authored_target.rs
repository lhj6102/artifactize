//! The validated spelling used for a definition's PATH and for editor operations.

use super::{Target, Uri, validation};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{path::PathBuf, str::FromStr};

/// The spelling in the definition, including a folder's trailing slash or a Live URI.
/// Whether a folder is narrowed to an aspect depends on the separate `select` field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthoredTarget {
    text: String,
    target: Target,
}

impl AuthoredTarget {
    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub(super) fn into_parts(self) -> (String, Target) {
        (self.text, self.target)
    }
}

impl FromStr for AuthoredTarget {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let target = if Uri::has_scheme(text) {
            Target::Live(text.parse::<Uri>().map_err(|error| error.to_string())?)
        } else if text == "./" {
            Target::Folder(PathBuf::new())
        } else {
            let logical = text.strip_suffix('/').unwrap_or(text);
            if logical.contains(':') {
                return Err("Local paths must not contain ':'.".into());
            }
            if logical.is_empty() {
                return Err("path must name a file or folder".into());
            }
            validation::path(logical)?;
            if text.ends_with('/') {
                Target::Folder(logical.into())
            } else {
                Target::File(logical.into())
            }
        };
        Ok(Self {
            text: text.into(),
            target,
        })
    }
}

impl Serialize for AuthoredTarget {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AuthoredTarget {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}
