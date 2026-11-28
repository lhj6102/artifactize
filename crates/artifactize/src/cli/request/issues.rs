//! A person's issues for `request submit`: repeated `--issue` texts or a regular JSON file,
//! bounded and never echoed back in a parse error.

use std::{ffi::OsStr, io::Read, path::PathBuf};

use artifactize_core::{
    human::{self, Submission},
    runtime::Verdict,
};
use clap::{
    ArgMatches, Args, Command, Error, FromArgMatches, builder::TypedValueParser, error::ErrorKind,
};

#[derive(Debug, Default)]
pub enum IssuesSource {
    #[default]
    Absent,
    Inline(Vec<String>),
    File(PathBuf),
}

#[derive(Args)]
#[group(skip)]
struct IssuesFlags {
    /// One issue to change, required for RED; repeat for each.
    #[arg(long, value_name = "TEXT", conflicts_with = "issues_file", value_parser = IssueParser)]
    issue: Vec<String>,
    /// Read the issues from a regular file: a JSON array of strings.
    #[arg(long, value_name = "PATH")]
    issues_file: Option<PathBuf>,
}

impl IssuesFlags {
    fn source(self) -> Result<Option<IssuesSource>, Error> {
        match (self.issue.is_empty(), self.issues_file) {
            (true, None) => Ok(None),
            (false, None) => Ok(Some(IssuesSource::Inline(self.issue))),
            (true, Some(path)) => Ok(Some(IssuesSource::File(path))),
            (false, Some(_)) => Err(Error::raw(
                ErrorKind::ArgumentConflict,
                "Use --issue or --issues-file, not both.",
            )),
        }
    }
}

impl FromArgMatches for IssuesSource {
    fn from_arg_matches(matches: &ArgMatches) -> Result<Self, Error> {
        Ok(IssuesFlags::from_arg_matches(matches)?
            .source()?
            .unwrap_or_default())
    }

    fn update_from_arg_matches(&mut self, matches: &ArgMatches) -> Result<(), Error> {
        if let Some(source) = IssuesFlags::from_arg_matches(matches)?.source()? {
            *self = source;
        }
        Ok(())
    }
}

impl Args for IssuesSource {
    fn augment_args(command: Command) -> Command {
        IssuesFlags::augment_args(command)
    }

    fn augment_args_for_update(command: Command) -> Command {
        IssuesFlags::augment_args_for_update(command)
    }
}

impl IssuesSource {
    /// GREEN with no issue, or RED with at least one.
    pub(super) fn submission(self, verdict: Verdict) -> Result<Submission, String> {
        let issues = match self {
            Self::Absent => Vec::new(),
            Self::Inline(issues) => issues,
            Self::File(path) => read_file(path)?,
        };
        Submission::new(verdict, issues)
    }
}

#[derive(Clone)]
struct IssueParser;

impl TypedValueParser for IssueParser {
    type Value = String;

    fn parse_ref(
        &self,
        command: &clap::Command,
        _arg: Option<&clap::Arg>,
        value: &OsStr,
    ) -> Result<Self::Value, clap::Error> {
        let text = artifactize_core::platform::os_text(value).ok_or_else(|| {
            clap::Error::raw(ErrorKind::InvalidUtf8, "An issue must be UTF-8.").with_cmd(command)
        })?;
        // Clap's default validation error includes the raw value; don't echo a review.
        if text.len() > human::MAX_RESULT_BYTES {
            return Err(clap::Error::raw(
                ErrorKind::ValueValidation,
                format!("An issue exceeds {} bytes.", human::MAX_RESULT_BYTES),
            )
            .with_cmd(command));
        }
        Ok(text.to_owned())
    }
}

fn read_file(path: PathBuf) -> Result<Vec<String>, String> {
    let file = artifactize_core::platform::open_regular_input(&path)
        .map_err(|e| e.to_string())?
        .ok_or("Issues must be a regular JSON file.")?;
    let mut bytes = Vec::new();
    file.take(human::ISSUES_READ_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > human::MAX_RESULT_BYTES {
        return Err(format!("Issues exceed {} bytes.", human::MAX_RESULT_BYTES));
    }
    serde_json::from_slice(&bytes).map_err(|_| "Issues must be a JSON array of strings.".into())
}
