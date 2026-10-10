//! Parse schema-validated provider arguments once, before builtin execution.
use crate::Builtin;
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
    pub fn parse(builtin: Builtin, value: Value) -> Result<Self, String> {
        let input = match builtin {
            Builtin::Read => serde_json::from_value(value).map(Self::Read),
            Builtin::List => serde_json::from_value(value).map(Self::List),
            Builtin::Glob => serde_json::from_value(value).map(Self::Glob),
            Builtin::Grep => serde_json::from_value(value).map(Self::Grep),
            Builtin::ViewImage => serde_json::from_value(value).map(Self::ViewImage),
        };
        input.map_err(|_| "Tool arguments cannot be read as the declared builtin input.".into())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Read {
    pub path: String,
    #[serde(default = "first_line", deserialize_with = "integer")]
    pub offset: usize,
    #[serde(default = "read_lines", deserialize_with = "integer")]
    pub limit: usize,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct List {
    #[serde(default)]
    pub path: String,
    #[serde(default, deserialize_with = "integer")]
    pub offset: usize,
    #[serde(default = "max_results", deserialize_with = "integer")]
    pub limit: usize,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Glob {
    pub pattern: String,
    #[serde(default)]
    pub path: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Grep {
    pub pattern: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub glob: Option<String>,
    #[serde(default)]
    pub case_insensitive: bool,
    #[serde(default = "max_results", deserialize_with = "integer")]
    pub max_results: usize,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewImage {
    pub path: String,
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
