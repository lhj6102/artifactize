//! Parse bounded, private Human owner fields from an argument or regular file.

use std::{ffi::OsStr, io::Read, path::PathBuf};

use artifactize_core::{
    human::{self, Submission},
    runtime::Verdict,
};
use clap::{builder::TypedValueParser, error::ErrorKind};
use serde_json::{Map, Value};

/// A bounded JSON object without the verdict, supplied separately by `--verdict`.
#[derive(Clone, Debug, Default)]
pub struct OwnerFields(Map<String, Value>);

impl std::str::FromStr for OwnerFields {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.len() > human::MAX_RESULT_BYTES {
            return Err(format!(
                "Human fields exceed {} bytes.",
                human::MAX_RESULT_BYTES
            ));
        }
        let Value::Object(fields) = serde_json::from_str(text).map_err(|e| e.to_string())? else {
            return Err("Human fields must be a JSON object.".into());
        };
        if fields.contains_key("verdict") {
            return Err("Use --verdict, not a verdict inside --fields.".into());
        }
        Ok(Self(fields))
    }
}

#[derive(Clone)]
pub(super) struct OwnerFieldsParser;

impl TypedValueParser for OwnerFieldsParser {
    type Value = OwnerFields;

    fn parse_ref(
        &self,
        command: &clap::Command,
        _arg: Option<&clap::Arg>,
        value: &OsStr,
    ) -> Result<Self::Value, clap::Error> {
        let text = value.to_str().ok_or_else(|| {
            clap::Error::raw(ErrorKind::InvalidUtf8, "Human fields must be UTF-8.")
                .with_cmd(command)
        })?;
        // Clap's default validation error includes the raw value; don't echo owner fields.
        text.parse().map_err(|error: String| {
            clap::Error::raw(ErrorKind::ValueValidation, error).with_cmd(command)
        })
    }
}

impl OwnerFields {
    pub(super) fn submission(
        verdict: Verdict,
        fields: Option<Self>,
        fields_file: Option<PathBuf>,
    ) -> Result<Submission, String> {
        let fields = match (fields, fields_file) {
            (None, None) => Self::default(),
            (Some(fields), None) => fields,
            (None, Some(path)) => Self::read_file(path)?,
            (Some(_), Some(_)) => return Err("Use --fields or --fields-file, not both.".into()),
        };
        Ok(Submission::new(verdict, fields.0).expect("OwnerFields excludes the verdict"))
    }

    fn read_file(path: PathBuf) -> Result<Self, String> {
        let file = artifactize_core::platform::open_regular_input(&path)
            .map_err(|e| e.to_string())?
            .ok_or("Human fields must be a regular JSON file.")?;
        let mut bytes = Vec::new();
        file.take(human::FIELDS_READ_BYTES)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        std::str::from_utf8(&bytes)
            .map_err(|e| e.to_string())?
            .parse()
    }
}
