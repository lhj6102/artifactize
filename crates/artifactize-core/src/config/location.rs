//! Source locations for typed declaration validation, retained from the TOML parser.

use artifactize_tools::artf::{Masked, mask};
use toml::{Spanned, de::DeValue};

#[derive(Clone)]
pub(super) struct Location<'a, 'i> {
    source: &'a str,
    value: &'a Spanned<DeValue<'i>>,
    masked: Option<&'a Masked>,
    path: String,
    start: usize,
}

impl<'a, 'i> Location<'a, 'i> {
    pub fn root(source: &'a str, value: &'a Spanned<DeValue<'i>>) -> Self {
        Self::root_masked(source, value, None)
    }

    pub fn root_masked(
        source: &'a str,
        value: &'a Spanned<DeValue<'i>>,
        masked: Option<&'a Masked>,
    ) -> Self {
        Self {
            source,
            value,
            masked,
            path: String::new(),
            start: value.span().start,
        }
    }

    pub fn child(&self, key: &str) -> Self {
        let value = self.value.get_ref().get(key).unwrap_or(self.value);
        Self {
            source: self.source,
            value,
            masked: self.masked,
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

    pub fn value(&self) -> &'a Spanned<DeValue<'i>> {
        self.value
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn table(&self) -> Result<&toml::de::DeTable<'i>, String> {
        self.value
            .get_ref()
            .as_table()
            .ok_or_else(|| self.error("expected a table"))
    }

    /// Decode a typed value at its authored span, including errors inside its fields.
    pub fn deserialize<T: serde::de::DeserializeOwned>(&self) -> Result<T, String> {
        T::deserialize(toml::de::ValueDeserializer::from(self.value.clone())).map_err(
            |mut error| {
                if let Some(masked) = self.masked
                    && !masked.invocations().is_empty()
                {
                    let offset = error
                        .span()
                        .map_or(self.original_offset(self.start), |span| {
                            masked.original_offset(span.start)
                        });
                    return positioned(self.source, offset, &self.path, error.message());
                }
                error.set_input(Some(self.source));
                self.error(error)
            },
        )
    }

    pub fn check<T>(&self, result: Result<T, String>) -> Result<T, String> {
        result.map_err(|message| self.error(message))
    }

    pub fn error(&self, message: impl std::fmt::Display) -> String {
        positioned(
            self.source,
            self.original_offset(self.start),
            &self.path,
            message,
        )
    }

    pub fn original_offset(&self, masked_offset: usize) -> usize {
        self.masked.map_or(masked_offset, |masked| {
            masked.original_offset(masked_offset)
        })
    }
}

pub(super) fn error_at(source: &str, keys: &[&str], message: &str) -> String {
    let Ok(masked) = mask(source) else {
        return message.into();
    };
    let Ok(table) = toml::de::DeTable::parse(&masked.toml) else {
        return message.into();
    };
    let value = Spanned::new(table.span(), DeValue::Table(table.into_inner()));
    let location = keys.iter().fold(
        Location::root_masked(source, &value, Some(&masked)),
        |location, key| location.child(key),
    );
    location.error(message)
}

pub(super) fn toml_error(source: &str, masked: &Masked, mut error: toml::de::Error) -> String {
    if masked.invocations().is_empty() {
        error.set_input(Some(source));
        return error.to_string();
    }
    match error.span() {
        Some(span) => {
            positioned_message(source, masked.original_offset(span.start), error.message())
        }
        None => error.to_string(),
    }
}

pub(super) fn positioned_message(
    source: &str,
    start: usize,
    message: impl std::fmt::Display,
) -> String {
    let (line, column) = line_column(source, start);
    format!("line {line}, column {column}: {message}")
}

pub(super) fn positioned(
    source: &str,
    start: usize,
    path: &str,
    message: impl std::fmt::Display,
) -> String {
    let (line, column) = line_column(source, start);
    format!("line {line}, column {column}: {path}: {message}")
}

fn line_column(source: &str, start: usize) -> (usize, usize) {
    let prefix = &source[..start.min(source.len())];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix.rsplit('\n').next().unwrap().chars().count() + 1;
    (line, column)
}
