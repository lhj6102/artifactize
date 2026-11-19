//! Validation and descriptions of fixed builtin declarations.
use crate::{
    Builtin,
    scope::{ArtifactId, Scope},
};
use serde_json::{Value, json};
use std::path::Path;

/// A `section` declaration that fixes its heading: the path, then the heading.
const SECTION_WITH_HEADING: usize = 2;
/// A `section` declaration that leaves the heading to each call (Agent tools only): the path.
const SECTION_HEADING_AT_CALL: usize = 1;

pub fn validate_args(builtin: Builtin, args: Option<&[String]>, human: bool) -> Result<(), String> {
    let Some(args) = args else {
        return if !human
            && matches!(
                builtin,
                Builtin::Read | Builtin::List | Builtin::Glob | Builtin::Grep | Builtin::ViewImage
            ) {
            Ok(())
        } else {
            Err("This builtin requires fixed args.".into())
        };
    };
    let valid = match builtin {
        Builtin::Graph => args.is_empty(),
        Builtin::Read | Builtin::List => args.len() == 1,
        Builtin::Section => {
            args.len() == SECTION_WITH_HEADING || !human && args.len() == SECTION_HEADING_AT_CALL
        }
        Builtin::Help => !args.is_empty(),
        Builtin::Open => human && args.len() == 1,
        Builtin::Glob | Builtin::Grep | Builtin::ViewImage => false,
    };
    if !valid
        || args.iter().any(|arg| arg.contains('\0'))
        || builtin == Builtin::Help && args.first().is_some_and(|arg| arg.trim().is_empty())
    {
        return Err("Invalid fixed args for this builtin/kind.".into());
    }
    if builtin == Builtin::Section
        && let Some(heading) = args.get(1)
    {
        validate_heading(heading)?;
    }
    Ok(())
}

pub(super) fn validate_heading(heading: &str) -> Result<(), String> {
    if heading.trim().is_empty() {
        return Err("Section heading must not be blank.".into());
    }
    Ok(())
}

pub fn fixed_description(builtin: Builtin, args: &[String]) -> &'static str {
    match builtin {
        Builtin::Read => {
            "Read the declared UTF-8 file in {artifactName}, up to 64 KiB. No input arguments; no symlinks or binary data."
        }
        Builtin::List => {
            "List the declared directory in {artifactName}. No input arguments; returns sorted entries and truncation metadata."
        }
        Builtin::Section if args.len() == 1 => {
            "Read the declared Markdown file in {artifactName} by exact heading. Supply only heading; includes nested sections until the next same or higher level heading. Missing headings list available titles."
        }
        Builtin::Section => {
            "Read the declared Markdown section in {artifactName}. No input arguments; includes nested sections until the next same or higher level heading."
        }
        Builtin::Help => {
            "Read the declared program/subcommand --help output. No input arguments; limited to 10 seconds and 64 KiB."
        }
        _ => super::super::description(builtin),
    }
}

pub fn fixed_schema(builtin: Builtin, args: &[String]) -> Value {
    if builtin == Builtin::Section && args.len() == 1 {
        super::super::input_schema(builtin)
    } else {
        json!({"type":"object", "properties":{}, "required":[], "additionalProperties":false})
    }
}

/// Only explicit HTTP(S) URLs bypass filesystem scope; local file URLs do not.
pub fn is_url(target: &str) -> bool {
    target.starts_with("https://") || target.starts_with("http://")
}

pub fn validate_target(
    builtin: Builtin,
    args: &[String],
    root: &Path,
    scope: &Scope,
    owner: &ArtifactId,
) -> Result<(), String> {
    if matches!(builtin, Builtin::Graph | Builtin::Help) {
        return Ok(());
    }
    let target = args.first().ok_or("Missing builtin target.")?;
    if builtin == Builtin::Open && is_url(target) {
        return Ok(());
    }
    // Existing targets are validated without following any symlink.
    scope
        .resolve_input(root, owner, target)
        .map(|_| ())
        .map_err(|error| error.to_string())
}
