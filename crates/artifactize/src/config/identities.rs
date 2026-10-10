//! Declaration identifiers are validated once when configuration or saved profiles enter.

use serde::{Deserialize, Serialize};
use std::{borrow::Borrow, fmt, ops::Deref, str::FromStr};

macro_rules! identity {
    ($name:ident, $valid:expr) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);
        impl FromStr for $name {
            type Err = String;
            fn from_str(value: &str) -> Result<Self, Self::Err> {
                if ($valid)(value) {
                    Ok(Self(value.to_owned()))
                } else {
                    Err(format!("Invalid {}: {value:?}", stringify!($name)))
                }
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                use serde::de::Error;
                String::deserialize(deserializer)?
                    .parse()
                    .map_err(D::Error::custom)
            }
        }
        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl Deref for $name {
            type Target = str;
            fn deref(&self) -> &str {
                &self.0
            }
        }
        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }
        impl Borrow<str> for $name {
            fn borrow(&self) -> &str {
                &self.0
            }
        }
        impl Borrow<String> for $name {
            fn borrow(&self) -> &String {
                &self.0
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.0 == other
            }
        }
        impl PartialEq<&str> for $name {
            fn eq(&self, other: &&str) -> bool {
                self.0 == *other
            }
        }
        impl PartialEq<String> for $name {
            fn eq(&self, other: &String) -> bool {
                &self.0 == other
            }
        }
        impl PartialEq<$name> for String {
            fn eq(&self, other: &$name) -> bool {
                *self == other.0
            }
        }
        impl PartialEq<$name> for str {
            fn eq(&self, other: &$name) -> bool {
                self == other.0
            }
        }
        impl PartialEq<$name> for &str {
            fn eq(&self, other: &$name) -> bool {
                *self == other.0
            }
        }
    };
}
/// Two declaration identifiers of 64 bytes, joined by one underscore.
const MAX_TOOL_NAME_BYTES: usize = 129;
/// Endpoint namespaces retain a complete SHA-256 digest in lowercase hexadecimal.
const ENDPOINT_HEX_BYTES: usize = 64;

fn name(value: &str) -> bool {
    super::identifier(value, "").is_ok()
}
identity!(ModelId, |value: &str| !value.trim().is_empty());
identity!(ProfileVariantName, name);
identity!(ToolOperationName, name);
// Published tool names concatenate an operation and Artifact name with `_`; each
// component is at most 64 ASCII bytes. Collision detection remains a registry concern.
identity!(ToolName, |value: &str| !value.is_empty()
    && value.len() <= MAX_TOOL_NAME_BYTES
    && value.bytes().all(
        |byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
    ));
identity!(EndpointId, |value: &str| value.len() == ENDPOINT_HEX_BYTES
    && value.bytes().all(
        |byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()
    ));
identity!(HubEpoch, |value: &str| !value.is_empty()
    && value.len() <= crate::types::MAX_ID_BYTES
    && value.bytes().all(
        |byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
    ));

identity!(MountAlias, name);
identity!(ChildPrefix, |value: &str| crate::config::validation::path(
    value
)
.is_ok());
identity!(LogicalPath, |value: &str| value == "."
    || crate::config::validation::path(value).is_ok());

impl AsRef<std::path::Path> for LogicalPath {
    fn as_ref(&self) -> &std::path::Path {
        std::path::Path::new(self.as_str())
    }
}

impl AsRef<std::ffi::OsStr> for LogicalPath {
    fn as_ref(&self) -> &std::ffi::OsStr {
        std::ffi::OsStr::new(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn declaration_ids_validate_at_json_boundary_and_keep_string_forms() {
        for invalid in ["", " ", "bad/name", "_bad", "a\nb"] {
            assert!(serde_json::from_value::<ProfileVariantName>(json!(invalid)).is_err());
            assert!(serde_json::from_value::<ToolOperationName>(json!(invalid)).is_err());
        }
        let model: ModelId = serde_json::from_value(json!("provider/model-v1")).unwrap();
        assert_eq!(
            serde_json::to_value(model).unwrap(),
            json!("provider/model-v1")
        );
        for invalid in ["", "a/../b", "a\\b", "C:/outside"] {
            assert!(serde_json::from_value::<LogicalPath>(json!(invalid)).is_err());
        }
        for logical in [".", "folder/file.txt"] {
            let path: LogicalPath = serde_json::from_value(json!(logical)).unwrap();
            assert_eq!(serde_json::to_value(path).unwrap(), json!(logical));
        }
    }
}
