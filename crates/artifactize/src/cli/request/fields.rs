//! Parse bounded, private Human owner fields from an argument or regular file.

use std::{ffi::OsStr, io::Read, path::PathBuf};

use artifactize_core::{
    human::{self, Submission},
    runtime::Verdict,
};
use clap::{
    ArgMatches, Args, Command, Error, FromArgMatches, builder::TypedValueParser, error::ErrorKind,
};
use serde_json::{Map, Value};

#[derive(Debug)]
pub enum FieldsSource {
    Absent,
    Inline(OwnerFields),
    File(PathBuf),
}

#[derive(Args)]
#[group(skip)]
struct FieldsFlags {
    /// Owner fields as a JSON object (default {}).
    #[arg(long, value_name = "JSON", conflicts_with = "fields_file", value_parser = OwnerFieldsParser)]
    fields: Option<OwnerFields>,
    /// Read owner fields from a regular JSON file.
    #[arg(long, value_name = "PATH")]
    fields_file: Option<PathBuf>,
}

impl FieldsFlags {
    fn source(self) -> Result<Option<FieldsSource>, Error> {
        match (self.fields, self.fields_file) {
            (None, None) => Ok(None),
            (Some(fields), None) => Ok(Some(FieldsSource::Inline(fields))),
            (None, Some(path)) => Ok(Some(FieldsSource::File(path))),
            (Some(_), Some(_)) => Err(Error::raw(
                ErrorKind::ArgumentConflict,
                "Use --fields or --fields-file, not both.",
            )),
        }
    }
}

impl FromArgMatches for FieldsSource {
    fn from_arg_matches(matches: &ArgMatches) -> Result<Self, Error> {
        Ok(FieldsFlags::from_arg_matches(matches)?
            .source()?
            .unwrap_or(Self::Absent))
    }

    fn update_from_arg_matches(&mut self, matches: &ArgMatches) -> Result<(), Error> {
        if let Some(source) = FieldsFlags::from_arg_matches(matches)?.source()? {
            *self = source;
        }
        Ok(())
    }
}

impl Args for FieldsSource {
    fn augment_args(command: Command) -> Command {
        FieldsFlags::augment_args(command)
    }

    fn augment_args_for_update(command: Command) -> Command {
        FieldsFlags::augment_args_for_update(command)
    }
}

impl FieldsSource {
    pub(super) fn submission(self, verdict: Verdict) -> Result<Submission, String> {
        let fields = match self {
            Self::Absent => OwnerFields::default(),
            Self::Inline(fields) => fields,
            Self::File(path) => OwnerFields::read_file(path)?,
        };
        Ok(Submission::new(verdict, fields.0).expect("OwnerFields excludes the verdict"))
    }
}

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
struct OwnerFieldsParser;

impl TypedValueParser for OwnerFieldsParser {
    type Value = OwnerFields;

    fn parse_ref(
        &self,
        command: &clap::Command,
        _arg: Option<&clap::Arg>,
        value: &OsStr,
    ) -> Result<Self::Value, clap::Error> {
        let text = artifactize_core::platform::argument_text(value).ok_or_else(|| {
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
