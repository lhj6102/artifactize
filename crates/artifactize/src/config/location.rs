//! Source locations for typed declaration validation, retained from the TOML parser.

use toml::{Spanned, de::DeValue};

#[derive(Clone)]
pub(super) struct Location<'a, 'i> {
    source: &'a str,
    value: &'a Spanned<DeValue<'i>>,
    path: String,
    start: usize,
}

impl<'a, 'i> Location<'a, 'i> {
    pub fn root(source: &'a str, value: &'a Spanned<DeValue<'i>>) -> Self {
        Self {
            source,
            value,
            path: String::new(),
            start: value.span().start,
        }
    }

    pub fn child(&self, key: &str) -> Self {
        let value = self.value.get_ref().get(key).unwrap_or(self.value);
        Self {
            source: self.source,
            value,
            path: if self.path.is_empty() {
                key.into()
            } else {
                format!("{}.{key}", self.path)
            },
            start: value.span().start,
        }
    }

    pub fn key(&self, key: &str) -> Self {
        let mut child = self.child(key);
        if let Some((key, _)) = self
            .value
            .get_ref()
            .as_table()
            .and_then(|table| table.get_key_value(key))
        {
            child.start = key.span().start;
        }
        child
    }

    /// JSON-backed sum types buffer their fields; validate those fields with TOML first.
    pub fn deserialize<T: serde::de::DeserializeOwned>(&self) -> Result<T, String> {
        T::deserialize(toml::de::ValueDeserializer::from(self.value.clone())).map_err(
            |mut error| {
                error.set_input(Some(self.source));
                format!("{}: {error}", self.path)
            },
        )
    }

    pub fn check<T>(&self, result: Result<T, String>) -> Result<T, String> {
        result.map_err(|message| self.error(message))
    }

    pub fn error(&self, message: impl std::fmt::Display) -> String {
        positioned(self.source, self.start, &self.path, message)
    }
}

pub(super) fn error_at(source: &str, keys: &[&str], message: &str) -> String {
    let Ok(table) = toml::de::DeTable::parse(source) else {
        return message.into();
    };
    let value = Spanned::new(table.span(), DeValue::Table(table.into_inner()));
    let location = keys
        .iter()
        .fold(Location::root(source, &value), |location, key| {
            location.child(key)
        });
    location.error(message)
}

pub(super) fn positioned(
    source: &str,
    start: usize,
    path: &str,
    message: impl std::fmt::Display,
) -> String {
    let prefix = &source[..start];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix.rsplit('\n').next().unwrap().chars().count() + 1;
    format!("line {line}, column {column}: {path}: {message}")
}
