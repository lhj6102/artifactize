//! Fixed tool parsing and scoped text execution shared by every caller.
use crate::{
    Builtin, Content, ToolResult,
    launch::Launcher,
    scope::{self, ArtifactId, Scope},
};
use serde::Deserialize;
use serde_json::Value;
use std::{io::Read, path::Path};
use tokio_util::sync::CancellationToken;
mod declaration;
mod help;
mod markdown;
pub use declaration::{fixed_description, fixed_schema, is_url, validate_args, validate_target};
use help::help;
use markdown::section;
/// Fixed text tools contribute at most one 64 KiB block to a review.
const TEXT_BYTES: usize = 64 * 1024;
/// Bound Markdown parsing, including a section near the end of the document.
const DOCUMENT_BYTES: usize = 8 * 1024 * 1024;

/// Where a fixed built-in runs: the workspace root, the scope it may read, the Artifact
/// that declares it, and the launcher that starts a `help` program.
pub struct Context<'a> {
    pub root: &'a Path,
    pub scope: &'a Scope,
    pub owner: &'a ArtifactId,
    pub launcher: &'a dyn Launcher,
}

pub async fn call_fixed(
    builtin: Builtin,
    args: &[String],
    input: Value,
    context: Context<'_>,
    cancellation: &CancellationToken,
) -> ToolResult {
    if cancellation.is_cancelled() {
        return ToolResult::error("Tool call was cancelled.");
    }
    let input = match FixedInput::parse(builtin, args, input) {
        Ok(input) => input,
        Err(error) => return ToolResult::error(error),
    };
    let Context {
        root,
        scope,
        owner,
        launcher,
    } = context;
    if let FixedInput::Help(args) = input {
        let cwd = root.join(scope.artifacts[owner].folder());
        return match help(&args, &cwd, launcher, cancellation).await {
            Ok(text) => text_result(text),
            Err(error) => ToolResult::error(bounded(&error)),
        };
    }
    let root = root.to_owned();
    let scope = Scope {
        artifacts: scope.artifacts.clone(),
    };
    let owner = owner.clone();
    let cancellation = cancellation.child_token();
    let _cancel_on_drop = cancellation.clone().drop_guard();
    tokio::task::spawn_blocking(move || call_text(input, &root, &scope, &owner, &cancellation))
        .await
        .unwrap_or_else(|error| {
            if error.is_panic() {
                std::panic::resume_unwind(error.into_panic());
            }
            ToolResult::error("Built-in tool execution failed.")
        })
}

/// The model's input to a fixed `section` tool: the heading, when the declaration leaves
/// it open.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SectionInput {
    heading: Option<String>,
}

/// Parsed fixed declarations and model input. Execution cannot index an absent argument
/// or combine a heading with a tool that does not accept one.
enum FixedInput {
    Read {
        path: super::input::LogicalPath,
    },
    List(super::input::List),
    Section {
        path: super::input::LogicalPath,
        heading: String,
    },
    Help(Vec<String>),
}

impl FixedInput {
    fn parse(builtin: Builtin, args: &[String], value: Value) -> Result<Self, String> {
        validate_args(builtin, Some(args), false)?;
        let validator = crate::schema::compile(&fixed_schema(builtin, args))?;
        crate::schema::validate(&validator, &value)?;
        match builtin {
            Builtin::Read => Ok(Self::Read {
                path: super::input::LogicalPath::parse(args[0].clone())?,
            }),
            Builtin::List => Ok(Self::List(super::input::List {
                path: super::input::LogicalPath::parse(args[0].clone())?,
                offset: 0,
                limit: super::MAX_RESULTS,
            })),
            Builtin::Section => {
                let input: SectionInput = serde_json::from_value(value)
                    .map_err(|_| "Tool arguments cannot be read as the declared builtin input.")?;
                let heading = args
                    .get(1)
                    .cloned()
                    .or(input.heading)
                    .ok_or("Section requires heading.")?;
                Ok(Self::Section {
                    path: super::input::LogicalPath::parse(args[0].clone())?,
                    heading,
                })
            }
            Builtin::Help => Ok(Self::Help(args.to_vec())),
            _ => Err("This builtin cannot run as a fixed Agent tool.".into()),
        }
    }
}

fn call_text(
    input: FixedInput,
    root: &Path,
    scope: &Scope,
    owner: &ArtifactId,
    cancellation: &CancellationToken,
) -> ToolResult {
    let result = match input {
        FixedInput::Read { path } => {
            read_text(root, scope, owner, &path, TEXT_BYTES).map(|text| bounded(&text))
        }
        FixedInput::Section { path, heading } => {
            read_text(root, scope, owner, &path, DOCUMENT_BYTES)
                .and_then(|text| section(&text, &heading))
        }
        FixedInput::List(input) => {
            return super::call(super::Input::List(input), root, scope, owner, cancellation);
        }
        FixedInput::Help(_) => unreachable!("help executes asynchronously at the process edge"),
    };
    if cancellation.is_cancelled() {
        return ToolResult::error("Tool call was cancelled.");
    }
    match result {
        Ok(text) => text_result(text),
        Err(error) => ToolResult::error(error),
    }
}

fn text_result(text: String) -> ToolResult {
    ToolResult::success(Content::Text { text })
}

fn read_text(
    root: &Path,
    scope: &Scope,
    owner: &ArtifactId,
    path: &str,
    limit: usize,
) -> Result<String, String> {
    let location = scope
        .resolve_path(owner, path)
        .map_err(|error| error.to_string())?;
    let artifact = &scope.artifacts[&location.artifact_id];
    let path = if location.path.is_empty() {
        artifact.file_name().unwrap_or("")
    } else {
        &location.path
    };
    let file = scope::open_input(root, artifact, path).map_err(|error| error.to_string())?;
    if !file
        .metadata()
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("Reading requires a regular file.".into());
    }
    let mut bytes = Vec::new();
    file.take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.contains(&0) {
        return Err("Reading requires UTF-8 text, not binary data.".into());
    }
    if bytes.len() > limit && limit == DOCUMENT_BYTES {
        return Err("Markdown document exceeds 8 MiB.".into());
    }
    if bytes.len() > limit {
        // Only an incomplete trailing UTF-8 sequence may be omitted at the byte boundary.
        if let Err(error) = std::str::from_utf8(&bytes) {
            if error.error_len().is_some() {
                return Err("Reading requires UTF-8 text.".into());
            }
            bytes.truncate(error.valid_up_to());
        }
        let text =
            String::from_utf8(bytes).map_err(|_| "Reading requires UTF-8 text.".to_owned())?;
        return Ok(bounded(&format!("{text}\n[output truncated]")));
    }
    String::from_utf8(bytes).map_err(|_| "Reading requires UTF-8 text.".into())
}

fn bounded(text: &str) -> String {
    if text.len() <= TEXT_BYTES {
        return text.to_owned();
    }
    let suffix = "\n[output truncated]";
    let mut end = TEXT_BYTES - suffix.len();
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{suffix}", &text[..end])
}
