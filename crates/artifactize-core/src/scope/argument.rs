//! Validated declared argv references and their exact authored string form.

use super::{ScopeError, logical_path};
use serde::{Deserialize, Serialize};

/// An Artifact root has no suffix; non-root suffixes are validated logical paths.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogicalTarget {
    Root,
    Suffix(LogicalSuffix),
}

/// A nonempty relative logical path. Its private storage cannot bypass validation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LogicalSuffix(String);

impl std::str::FromStr for LogicalSuffix {
    type Err = ScopeError;
    fn from_str(path: &str) -> Result<Self, Self::Err> {
        logical_path(path)?;
        if path.is_empty() {
            return Err(ScopeError(
                "Artifact argument suffix must be nonempty.".into(),
            ));
        }
        Ok(Self(path.to_owned()))
    }
}

impl std::str::FromStr for LogicalTarget {
    type Err = ScopeError;
    fn from_str(path: &str) -> Result<Self, Self::Err> {
        if path.is_empty() {
            Ok(Self::Root)
        } else {
            path.parse().map(Self::Suffix)
        }
    }
}

impl std::ops::Deref for LogicalTarget {
    type Target = str;
    fn deref(&self) -> &str {
        match self {
            Self::Root => "",
            Self::Suffix(path) => &path.0,
        }
    }
}

impl LogicalTarget {
    pub fn as_str(&self) -> &str {
        self
    }
}

impl Serialize for LogicalTarget {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for LogicalTarget {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

impl AsRef<std::path::Path> for LogicalTarget {
    fn as_ref(&self) -> &std::path::Path {
        std::path::Path::new(self.as_str())
    }
}

impl std::fmt::Display for LogicalTarget {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self)
    }
}

/// A declared argument, parsed once when its declaration loads: either a plain literal, or an
/// `@{path name[/sub]}` / `--flag=@{path name[/sub]}` Artifact path. Parsing here only checks
/// shape (one slot at the end, a valid path suffix); whether `name` names a real Artifact is a
/// separate, config-wide check ([`reference_target`]) run once every declaration is loaded.
/// [`std::fmt::Display`] writes the declared spelling, where a literal `@{` is `@@{`, so saved
/// `Vec<String>` forms (`StoredProfile`, Views) read back unchanged; [`Argument::literal`] is
/// the text a literal passes to a program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Argument {
    Literal(String),
    Reference {
        prefix: String,
        name: crate::config::ReferenceName,
        path: LogicalTarget,
    },
}

/// The opener of the one slot kind an argument may hold.
pub(crate) const PATH_SLOT: &str = "@{path ";

impl Argument {
    /// A literal's own text, as a program receives it.
    pub fn literal(&self) -> Option<&str> {
        match self {
            Self::Literal(text) => Some(text),
            Self::Reference { .. } => None,
        }
    }
}

impl std::str::FromStr for Argument {
    type Err = ScopeError;
    fn from_str(argument: &str) -> Result<Self, Self::Err> {
        let mut text = String::new();
        let mut rest = argument;
        let mut slot = None;
        while let Some(at) = rest.find("@{").map(|index| {
            // `@@{` is an escaped literal opener.
            if index > 0 && rest.as_bytes()[index - 1] == b'@' {
                index - 1
            } else {
                index
            }
        }) {
            text.push_str(&rest[..at]);
            if rest[at..].starts_with("@@{") {
                text.push_str("@{");
                rest = &rest[at + 3..];
                continue;
            }
            if slot.is_some() || !rest[at..].starts_with(PATH_SLOT) {
                return Err(ScopeError(
                    "An argument holds one `@{path NAME[/sub]}` slot, or `@@{` for a literal `@{`."
                        .into(),
                ));
            }
            let close = rest[at..]
                .find('}')
                .ok_or_else(|| ScopeError("This `@{path …}` slot has no closing `}`.".into()))?;
            slot = Some((
                text.clone(),
                rest[at + PATH_SLOT.len()..at + close].trim().to_owned(),
            ));
            rest = &rest[at + close + 1..];
            if !rest.is_empty() {
                return Err(ScopeError(
                    "An argument's `@{path …}` slot ends it; write the path inside the slot."
                        .into(),
                ));
            }
        }
        text.push_str(rest);
        let Some((prefix, target)) = slot else {
            return Ok(Self::Literal(text));
        };
        let flag = prefix
            .strip_prefix("--")
            .and_then(|flag| flag.strip_suffix('='))
            .is_some_and(|flag| {
                !flag.is_empty()
                    && flag
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            });
        if !prefix.is_empty() && !flag {
            return Err(ScopeError(
                "An Artifact argument is `@{path NAME[/sub]}` or `--flag=@{path NAME[/sub]}`."
                    .into(),
            ));
        }
        let (name, path) = target.split_once('/').unwrap_or((&target, ""));
        if target.contains('/') && path.is_empty() {
            return Err(ScopeError(
                "Artifact argument suffix must be a nonempty /path.".into(),
            ));
        }
        logical_path(path)?;
        Ok(Self::Reference {
            prefix,
            name: name
                .parse()
                .map_err(|_| ScopeError(format!("`@{{path {target}}}` names no Artifact.")))?,
            path: path.parse()?,
        })
    }
}

impl std::fmt::Display for Argument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Literal(text) => f.write_str(&text.replace("@{", "@@{")),
            Self::Reference { prefix, name, path } => {
                write!(f, "{}{PATH_SLOT}{name}", prefix.replace("@{", "@@{"))?;
                if !path.is_empty() {
                    write!(f, "/{path}")?;
                }
                f.write_str("}")
            }
        }
    }
}

impl TryFrom<String> for Argument {
    type Error = ScopeError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<Argument> for String {
    fn from(argument: Argument) -> Self {
        argument.to_string()
    }
}

impl Serialize for Argument {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for Argument {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        String::deserialize(deserializer)?
            .parse()
            .map_err(|failure: ScopeError| D::Error::custom(failure.0))
    }
}

/// For fixtures: a trusted literal string, as declarations and test tables already spell it.
impl From<&str> for Argument {
    fn from(value: &str) -> Self {
        value.parse().expect("a valid declared argument")
    }
}

/// Equal iff `other`, spelled as a declared argument, is the same literal or reference: not a
/// mere prefix/suffix match, and never panics on a malformed `other` (it is just unequal).
impl PartialEq<str> for Argument {
    fn eq(&self, other: &str) -> bool {
        other.parse::<Self>().is_ok_and(|parsed| *self == parsed)
    }
}
impl PartialEq<&str> for Argument {
    fn eq(&self, other: &&str) -> bool {
        self == *other
    }
}
impl PartialEq<Argument> for str {
    fn eq(&self, other: &Argument) -> bool {
        other == self
    }
}
impl PartialEq<Argument> for String {
    fn eq(&self, other: &Argument) -> bool {
        other == self.as_str()
    }
}
impl PartialEq<String> for Argument {
    fn eq(&self, other: &String) -> bool {
        self == other.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_suffix_boundary_preserves_root_and_rejects_unsafe_paths() {
        for text in [
            "@{path owner}",
            "@{path owner/folder/file}",
            "--input=@{path owner/folder/file}",
            "literal @@{ and {braces}",
        ] {
            let argument: Argument = serde_json::from_value(serde_json::json!(text)).unwrap();
            assert_eq!(
                serde_json::to_value(argument).unwrap(),
                serde_json::json!(text)
            );
        }
        let root: LogicalTarget = "".parse().unwrap();
        assert_eq!(root, LogicalTarget::Root);
        assert_eq!(serde_json::to_value(root).unwrap(), "");
        let suffix: LogicalTarget = "folder/file".parse().unwrap();
        assert_eq!(serde_json::to_value(suffix).unwrap(), "folder/file");
        for invalid in ["", ".", "../outside", "a//b", "a/../b", "a\\b", "a\0b"] {
            assert!(invalid.parse::<LogicalSuffix>().is_err(), "{invalid:?}");
        }
        for invalid in [
            "@{path owner/}",
            "@{path owner/../outside}",
            "@{path owner/a//b}",
            "@{path owner/.}",
            "@{path owner}/after",
            "x=@{path owner}",
            "@{artf owner}",
            "@{path a}@{path b}",
        ] {
            assert!(
                serde_json::from_value::<Argument>(serde_json::json!(invalid)).is_err(),
                "{invalid}"
            );
        }
        let literal: Argument = "a @@{path b} {c}".parse().unwrap();
        assert_eq!(literal.literal(), Some("a @{path b} {c}"));
    }
}
