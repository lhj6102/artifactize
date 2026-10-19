//! Validation and descriptions of fixed builtin declarations.
use crate::{
    Builtin,
    scope::{self, ArtifactId, Scope},
};
use serde_json::{Value, json};
use std::path::Path;

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
        Builtin::Section => args.len() == 2 || !human && args.len() == 1,
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
    if builtin == Builtin::Section && args.get(1).is_some_and(|heading| heading.trim().is_empty()) {
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
    if builtin == Builtin::Graph
        || builtin == Builtin::Help
        || builtin == Builtin::Open && is_url(&args[0])
    {
        return Ok(());
    }
    // Existing targets are validated without following any symlink.
    let location = scope
        .resolve_path(owner, &args[0])
        .map_err(|error| error.to_string())?;
    let artifact = &scope.artifacts[&location.artifact_id];
    let base = scope::scoped_path(root, artifact.folder()).map_err(|error| error.to_string())?;
    let path = if location.path.is_empty() {
        artifact.file_name().unwrap_or("")
    } else {
        &location.path
    };
    scope::scoped_path(&base, Path::new(path))
        .map(|_| ())
        .map_err(|error| error.to_string())
}
