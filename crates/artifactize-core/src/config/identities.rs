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
/// A provider-safe tool name: a tool's FQN and its Artifact's name joined by `_`.
const MAX_TOOL_NAME_BYTES: usize = MAX_TOOL_FQN_BYTES + 1 + 64;
/// A tool's dotted name: a few namespace segments and the tool's own identifier.
const MAX_TOOL_FQN_BYTES: usize = 128;
/// Endpoint namespaces retain a complete SHA-256 digest in lowercase hexadecimal.
const ENDPOINT_HEX_BYTES: usize = 64;

fn name(value: &str) -> bool {
    super::identifier(value, "").is_ok()
}
identity!(ModelId, |value: &str| !value.trim().is_empty());
identity!(ProfileName, name);
identity!(ToolFqn, |value: &str| ToolFqn::namespace(value).is_ok());
// Published tool names concatenate an operation and Artifact name with `_`; each
// component is at most 64 ASCII bytes. Collision detection remains a registry concern.
identity!(ToolName, |value: &str| !value.is_empty()
    && value.len() <= MAX_TOOL_NAME_BYTES
    && value.bytes().all(
        |byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
    ));
// Provenance also names session lifecycle commands after a `:`, which no published tool name
// or tool FQN contains, so `svc.start` and a session's start never share a pin.
identity!(ExecutionPinName, |value: &str| value
    .strip_suffix(":start")
    .or_else(|| value.strip_suffix(":stop"))
    .unwrap_or(value)
    .parse::<ToolName>()
    .is_ok());
identity!(EndpointId, |value: &str| value.len() == ENDPOINT_HEX_BYTES
    && value.bytes().all(
        |byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()
    ));
identity!(HubEpoch, |value: &str| !value.is_empty()
    && value.len() <= crate::types::MAX_ID_BYTES
    && value.bytes().all(
        |byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
    ));

// An instruction or argv reference names an Artifact.
identity!(ReferenceName, |value: &str| super::artifact_name(value, "")
    .is_ok());
impl From<&crate::types::ArtifactName> for ReferenceName {
    fn from(value: &crate::types::ArtifactName) -> Self {
        // Artifact names and reference names have the same declaration grammar.
        Self(value.to_string())
    }
}
identity!(ChildPrefix, |value: &str| crate::config::validation::path(
    value
)
.is_ok());
pub use artifactize_tools::scope::LogicalPath;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn declaration_ids_validate_at_json_boundary_and_keep_string_forms() {
        for invalid in ["", " ", "bad/name", "_bad", "a\nb", "a.", ".a", "a..b", "1a.b"] {
            assert!(serde_json::from_value::<ProfileName>(json!(invalid)).is_err());
            assert!(serde_json::from_value::<ToolFqn>(json!(invalid)).is_err());
        }
        let fqn: ToolFqn = serde_json::from_value(json!("desktop.harness.snapshot")).unwrap();
        assert_eq!(fqn.namespace_part(), "desktop.harness");
        let model: ModelId = serde_json::from_value(json!("provider/model-v1")).unwrap();
        assert_eq!(
            serde_json::to_value(model).unwrap(),
            json!("provider/model-v1")
        );
        for invalid in [
            "",
            "a/../b",
            "a\\b",
            &crate::test_os::windows_drive("outside"),
        ] {
            assert!(serde_json::from_value::<LogicalPath>(json!(invalid)).is_err());
        }
        for logical in [".", "folder/file.txt"] {
            let path: LogicalPath = serde_json::from_value(json!(logical)).unwrap();
            assert_eq!(serde_json::to_value(path).unwrap(), json!(logical));
        }
    }
}

/// Match other declaration keys so plugin type names remain short and portable.
const MAX_ARTIFACT_TYPE_BYTES: usize = 64;

identity!(PluginName, |value: &str| {
    value.len() <= MAX_ARTIFACT_TYPE_BYTES
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && crate::platform::paths::portable_filename(value).is_ok()
});

impl ToolFqn {
    /// `a.b.c`: dot-separated identifiers, each starting with a letter.
    pub fn namespace(value: &str) -> Result<(), String> {
        if value.is_empty() || value.len() > MAX_TOOL_FQN_BYTES {
            return Err(format!("A tool name is 1–{MAX_TOOL_FQN_BYTES} bytes: {value:?}."));
        }
        for segment in value.split('.') {
            if !segment.as_bytes().first().is_some_and(u8::is_ascii_alphabetic) {
                return Err(format!(
                    "Each part of a tool name starts with a letter: {value:?}."
                ));
            }
            super::identifier(segment, "A tool name's part")?;
        }
        Ok(())
    }

    /// The namespace of `ns.tool`: everything before its last dot.
    pub fn namespace_part(&self) -> &str {
        self.0.rsplit_once('.').map_or("", |(namespace, _)| namespace)
    }

    /// The name a provider sees: `{fqn with . as _}_{artifact}`. Registries reject two tools
    /// whose names meet here.
    pub fn published(&self, artifact: &str) -> ToolName {
        format!("{}_{artifact}", self.0.replace('.', "_"))
            .parse()
            .expect("a validated FQN and Artifact name form a provider-safe tool name")
    }
}
