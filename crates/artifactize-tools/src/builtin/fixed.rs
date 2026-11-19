//! Fixed tool parsing and scoped text execution shared by every caller.
use super::input::{List, LogicalPath};
use crate::{
    Builtin, Content, ToolResult,
    launch::Launcher,
    scope::{ArtifactId, Scope},
};
use serde::Deserialize;
use serde_json::Value;
use std::{ffi::OsString, io::Read, path::Path};
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
/// Read one byte past the cap to distinguish an exact fit from truncated input.
const OVERFLOW_PROBE_BYTES: usize = 1;

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
    execute(input, context, cancellation).await
}

pub(crate) async fn execute(
    input: FixedInput,
    context: Context<'_>,
    cancellation: &CancellationToken,
) -> ToolResult {
    if cancellation.is_cancelled() {
        return ToolResult::error("Tool call was cancelled.");
    }
    let Context {
        root,
        scope,
        owner,
        launcher,
    } = context;
    let input = match input.0 {
        Invocation::Text(input) => input,
        Invocation::Help { program, args } => {
            let cwd = match scope
                .artifact(owner)
                .and_then(|artifact| artifact.native_folder(root))
            {
                Ok(cwd) => cwd,
                Err(error) => return ToolResult::error(error.to_string()),
            };
            return match help(&program, args, &cwd, launcher, cancellation).await {
                Ok(text) => text_result(text),
                Err(error) => ToolResult::error(bounded(&error)),
            };
        }
    };
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
pub(crate) struct FixedInput(Invocation);

enum Invocation {
    Text(TextInput),
    Help {
        program: OsString,
        args: Vec<OsString>,
    },
}

enum TextInput {
    Read { path: LogicalPath },
    List(List),
    Section { path: LogicalPath, heading: String },
}

impl FixedInput {
    fn text(input: TextInput) -> Self {
        Self(Invocation::Text(input))
    }

    pub(crate) fn read(path: String) -> Result<Self, String> {
        Ok(Self::text(TextInput::Read {
            path: LogicalPath::parse(path)?,
        }))
    }

    pub(crate) fn list(path: String) -> Result<Self, String> {
        Ok(Self::text(TextInput::List(List {
            path: LogicalPath::parse(path)?,
            offset: 0,
            limit: super::MAX_RESULTS,
        })))
    }

    pub(crate) fn section(path: String, heading: String) -> Result<Self, String> {
        declaration::validate_heading(&heading)?;
        Self::with_heading(path, heading)
    }

    fn with_heading(path: String, heading: String) -> Result<Self, String> {
        Ok(Self::text(TextInput::Section {
            path: LogicalPath::parse(path)?,
            heading,
        }))
    }

    pub(crate) fn help(args: &[String]) -> Result<Self, String> {
        validate_args(Builtin::Help, Some(args), false)?;
        Self::with_help(args)
    }

    fn with_help(args: &[String]) -> Result<Self, String> {
        let (program, args) = args
            .split_first()
            .expect("validated help requires a program");
        // A program given as shown path text (encoded or verbatim) is read back to its native
        // path before the lookup.
        let program = crate::files::path_from_text(program).map_err(|error| error.to_string())?;
        Ok(Self(Invocation::Help {
            program: program.into_os_string(),
            args: args.iter().map(OsString::from).collect(),
        }))
    }

    fn parse(builtin: Builtin, args: &[String], value: Value) -> Result<Self, String> {
        validate_args(builtin, Some(args), false)?;
        let validator = crate::schema::compile(&fixed_schema(builtin, args))?;
        crate::schema::validate(&validator, &value)?;
        let path = || {
            args.first()
                .expect("validated builtin requires a target")
                .clone()
        };
        match builtin {
            Builtin::Read => Self::read(path()),
            Builtin::List => Self::list(path()),
            Builtin::Section => {
                let input: SectionInput = serde_json::from_value(value)
                    .map_err(|_| "Tool arguments cannot be read as the declared builtin input.")?;
                let heading = args
                    .get(1)
                    .cloned()
                    .or(input.heading)
                    .expect("validated section requires a heading");
                Self::with_heading(path(), heading)
            }
            Builtin::Help => Self::with_help(args),
            _ => Err("This builtin cannot run as a fixed Agent tool.".into()),
        }
    }
}

fn call_text(
    input: TextInput,
    root: &Path,
    scope: &Scope,
    owner: &ArtifactId,
    cancellation: &CancellationToken,
) -> ToolResult {
    let result = match input {
        TextInput::Read { path } => read_text(root, scope, owner, &path, ReadKind::Excerpt),
        TextInput::Section { path, heading } => {
            read_text(root, scope, owner, &path, ReadKind::Document)
                .and_then(|text| section(&text, &heading))
        }
        TextInput::List(input) => {
            return super::call(super::Input::List(input), root, scope, owner, cancellation);
        }
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

#[derive(Clone, Copy)]
enum ReadKind {
    Excerpt,
    Document,
}

impl ReadKind {
    fn limit(self) -> usize {
        match self {
            Self::Excerpt => TEXT_BYTES,
            Self::Document => DOCUMENT_BYTES,
        }
    }
}

fn read_text(
    root: &Path,
    scope: &Scope,
    owner: &ArtifactId,
    path: &LogicalPath,
    kind: ReadKind,
) -> Result<String, String> {
    let path = path.resolve(scope, owner, false)?;
    let location = scope
        .locate(owner, path)
        .map_err(|error| error.to_string())?;
    let (path, file) = location
        .open_direct(root)
        .map_err(|error| error.to_string())?;
    if !file
        .metadata()
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("Reading requires a regular file.".into());
    }
    let file = crate::artf::reader(file, path.as_ref()).map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    file.take((kind.limit() + OVERFLOW_PROBE_BYTES) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    text_from_bytes(bytes, kind)
}

fn text_from_bytes(bytes: Vec<u8>, kind: ReadKind) -> Result<String, String> {
    if bytes.contains(&0) {
        return Err("Reading requires UTF-8 text, not binary data.".into());
    }
    let overflow = bytes.len() > kind.limit();
    if overflow && matches!(kind, ReadKind::Document) {
        return Err("Markdown document exceeds 8 MiB.".into());
    }
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        // Only an incomplete trailing UTF-8 sequence may be omitted at the byte boundary.
        Err(error) if overflow && error.utf8_error().error_len().is_none() => {
            let end = error.utf8_error().valid_up_to();
            let mut bytes = error.into_bytes();
            bytes.truncate(end);
            String::from_utf8(bytes).expect("valid UTF-8 prefix")
        }
        Err(_) => return Err("Reading requires UTF-8 text.".into()),
    };
    Ok(if overflow { truncated(&text) } else { text })
}

fn bounded(text: &str) -> String {
    if text.len() <= TEXT_BYTES {
        return text.to_owned();
    }
    truncated(text)
}

fn truncated(text: &str) -> String {
    let suffix = "\n[output truncated]";
    let end = text.floor_char_boundary((TEXT_BYTES - suffix.len()).min(text.len()));
    format!("{}{suffix}", &text[..end])
}
