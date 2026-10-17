//! Parse schema-validated provider arguments once, before builtin execution.
use crate::Builtin;
use globset::GlobMatcher;
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Deserializer, de};
use serde_json::Value;

#[derive(Debug)]
pub enum Input {
    Read(Read),
    List(List),
    Glob(Glob),
    Grep(Grep),
    ViewImage(ViewImage),
}
impl Input {
    /// CLI values enter the same compiled domain as provider input, without JSON round trips.
    pub(crate) fn glob(pattern: &str, path: String) -> Result<Self, String> {
        pattern_size(pattern)?;
        Ok(Self::Glob(Glob {
            pattern: super::glob(pattern)?,
            path: LogicalPath::parse(path)?,
        }))
    }

    pub(crate) fn grep(
        pattern: &str,
        path: String,
        glob: Option<&str>,
        insensitive: bool,
    ) -> Result<Self, String> {
        Grep::new(
            pattern,
            LogicalPath::parse(path)?,
            glob,
            insensitive,
            super::MAX_RESULTS,
        )
        .map(Self::Grep)
    }

    pub fn parse(builtin: Builtin, value: Value) -> Result<Self, String> {
        let validator = crate::schema::compile(&super::input_schema(builtin))?;
        crate::schema::validate(&validator, &value)?;
        let input = match builtin {
            Builtin::Graph | Builtin::Section | Builtin::Help | Builtin::Open => {
                return Err("This builtin requires declared args.".into());
            }
            Builtin::Read => serde_json::from_value(value).map(Self::Read),
            Builtin::List => serde_json::from_value(value).map(Self::List),
            Builtin::Glob => serde_json::from_value(value).map(Self::Glob),
            Builtin::Grep => serde_json::from_value(value).map(Self::Grep),
            Builtin::ViewImage => serde_json::from_value(value).map(Self::ViewImage),
        };
        input.map_err(|error| error.to_string())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Read {
    pub path: LogicalPath,
    #[serde(default = "first_line", deserialize_with = "integer")]
    pub offset: usize,
    #[serde(default = "read_lines", deserialize_with = "integer")]
    pub limit: usize,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct List {
    #[serde(default)]
    pub path: LogicalPath,
    #[serde(default, deserialize_with = "integer")]
    pub offset: usize,
    #[serde(default = "max_results", deserialize_with = "integer")]
    pub limit: usize,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Glob {
    #[serde(deserialize_with = "glob_pattern")]
    pub pattern: GlobMatcher,
    #[serde(default)]
    pub path: LogicalPath,
}
#[derive(Debug)]
pub struct Grep {
    pub pattern: Regex,
    pub path: LogicalPath,
    pub glob: Option<GlobMatcher>,
    pub max_results: usize,
}
impl<'de> Deserialize<'de> for Grep {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Arguments {
            pattern: String,
            #[serde(default)]
            path: LogicalPath,
            #[serde(default)]
            glob: Option<String>,
            #[serde(default)]
            case_insensitive: bool,
            #[serde(default = "max_results", deserialize_with = "integer")]
            max_results: usize,
        }
        let args = Arguments::deserialize(deserializer)?;
        Self::new(
            &args.pattern,
            args.path,
            args.glob.as_deref(),
            args.case_insensitive,
            args.max_results,
        )
        .map_err(de::Error::custom)
    }
}

impl Grep {
    fn new(
        pattern: &str,
        path: LogicalPath,
        glob: Option<&str>,
        insensitive: bool,
        max_results: usize,
    ) -> Result<Self, String> {
        pattern_size(pattern)?;
        if let Some(glob) = glob {
            pattern_size(glob)?;
        }
        let pattern = RegexBuilder::new(pattern)
            .case_insensitive(insensitive)
            .size_limit(super::REGEX_BYTES)
            .build()
            .map_err(|_| "Invalid or oversized regex pattern.".to_owned())?;
        Ok(Self {
            pattern,
            path,
            glob: glob.map(super::glob).transpose()?,
            max_results,
        })
    }
}

fn pattern_size(pattern: &str) -> Result<(), String> {
    if pattern.is_empty() || pattern.chars().count() > super::MAX_PATTERN_UNITS {
        Err("Pattern must be nonempty and fit the declared pattern limit.".into())
    } else {
        Ok(())
    }
}

/// Validated logical syntax. The legacy file-root spelling is resolved against the owner
/// by Reader::root_path; it never grants a folder access to dot components.
#[derive(Debug, Default)]
pub struct LogicalPath(String);
impl LogicalPath {
    pub fn parse(path: String) -> Result<Self, String> {
        if path != "." {
            crate::scope::logical_path(&path).map_err(|error| error.to_string())?;
        }
        Ok(Self(path))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl std::ops::Deref for LogicalPath {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}
impl<'de> Deserialize<'de> for LogicalPath {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let path = String::deserialize(deserializer)?;
        Self::parse(path).map_err(de::Error::custom)
    }
}

fn glob_pattern<'de, D: Deserializer<'de>>(deserializer: D) -> Result<GlobMatcher, D::Error> {
    super::glob(&String::deserialize(deserializer)?).map_err(de::Error::custom)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewImage {
    pub path: LogicalPath,
}

fn first_line() -> usize {
    1
}
fn read_lines() -> usize {
    super::DEFAULT_READ_LINES
}
fn max_results() -> usize {
    super::MAX_RESULTS
}

/// JSON Schema treats 1 and 1.0 as the same integer. Preserve the validator's numeric
/// semantics and the previous saturating usize conversion, including 32-bit hosts.
fn integer<'de, D: Deserializer<'de>>(deserializer: D) -> Result<usize, D::Error> {
    struct Integer;
    impl de::Visitor<'_> for Integer {
        type Value = usize;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a nonnegative safe JSON integer")
        }
        fn visit_u64<E: de::Error>(self, value: u64) -> Result<usize, E> {
            self.visit_f64(value as f64)
        }
        fn visit_i64<E: de::Error>(self, value: i64) -> Result<usize, E> {
            self.visit_f64(value as f64)
        }
        fn visit_f64<E: de::Error>(self, value: f64) -> Result<usize, E> {
            if value.is_finite()
                && value >= 0.0
                && value.fract() == 0.0
                && value <= crate::MAX_SAFE_JSON_INTEGER as f64
            {
                Ok(value as usize)
            } else {
                Err(E::custom("expected a nonnegative safe JSON integer"))
            }
        }
    }
    deserializer.deserialize_any(Integer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn edge_rejects_out_of_range_and_unknown_arguments_without_caller_validation() {
        for (builtin, value) in [
            (Builtin::Read, json!({"path":"notes", "offset":0})),
            (Builtin::Read, json!({"path":"notes", "limit":501})),
            (Builtin::List, json!({"limit":201})),
            (Builtin::Grep, json!({"pattern":"notes", "maxResults":0})),
            (Builtin::Glob, json!({"pattern":""})),
            (Builtin::Read, json!({"path":"../notes"})),
            (Builtin::List, json!({"path":"nested//notes"})),
            (Builtin::Glob, json!({"pattern":"../*"})),
            (Builtin::Grep, json!({"pattern":"["})),
            (Builtin::Grep, json!({"pattern":"notes", "glob":"../*"})),
            (Builtin::ViewImage, json!({"path":"image", "unknown":true})),
        ] {
            assert!(Input::parse(builtin, value).is_err());
        }
        let parsed = Input::parse(
            Builtin::Read,
            json!({"path":"notes", "offset":1.0, "limit":2.0}),
        )
        .unwrap();
        let Input::Read(input) = parsed else {
            panic!("expected a read input")
        };
        assert_eq!((input.offset, input.limit), (1, 2));
    }
}
