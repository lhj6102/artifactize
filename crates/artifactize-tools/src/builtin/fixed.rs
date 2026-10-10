//! Fixed declarations shared by the Agent/Human registries and standalone CLI.
use std::{
    ffi::{OsStr, OsString},
    io::Read,
    path::Path,
    time::Duration,
};

use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::{
    Builtin, Content, ToolResult,
    launch::{Launch, LaunchError, Launcher},
    program,
    scope::{self, ArtifactId, Scope},
};

/// Fixed text tools contribute at most one 64 KiB block to a review.
const TEXT_BYTES: usize = 64 * 1024;
/// Bound Markdown parsing, including a section near the end of the document.
const DOCUMENT_BYTES: usize = 8 * 1024 * 1024;
/// Help is documentation, not a long-running owner command.
const HELP_TIMEOUT: Duration = Duration::from_secs(10);

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
        _ => super::description(builtin),
    }
}

pub fn fixed_schema(builtin: Builtin, args: &[String]) -> Value {
    if builtin == Builtin::Section && args.len() == 1 {
        super::input_schema(builtin)
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
    if builtin == Builtin::Help || builtin == Builtin::Open && is_url(&args[0]) {
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
    Read { path: String },
    List(super::input::List),
    Section { path: String, heading: String },
    Help(Vec<String>),
}

impl FixedInput {
    fn parse(builtin: Builtin, args: &[String], value: Value) -> Result<Self, String> {
        validate_args(builtin, Some(args), false)?;
        let validator = crate::schema::compile(&fixed_schema(builtin, args))?;
        crate::schema::validate(&validator, &value)?;
        match builtin {
            Builtin::Read => Ok(Self::Read {
                path: args[0].clone(),
            }),
            Builtin::List => Ok(Self::List(super::input::List {
                path: args[0].clone(),
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
                    path: args[0].clone(),
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
    ToolResult {
        content: vec![Content::Text { text }],
        is_error: false,
    }
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

fn section(text: &str, wanted: &str) -> Result<String, String> {
    let mut headings = Vec::new();
    let mut heading = None;
    for (event, range) in Parser::new(text).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                heading = Some((level, range.start, String::new()))
            }
            Event::Text(value) | Event::Code(value) => {
                if let Some((_, _, title)) = &mut heading {
                    title.push_str(&value);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some((_, _, title)) = &mut heading {
                    title.push(' ');
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(heading) = heading.take() {
                    headings.push(heading);
                }
            }
            _ => {}
        }
    }
    let Some(index) = headings.iter().position(|(_, _, title)| title == wanted) else {
        return Err(bounded(&format!(
            "No matching heading: {wanted}. Available headings:\n{}",
            headings
                .iter()
                .map(|(_, _, title)| title.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        )));
    };
    let (level, start, _) = &headings[index];
    let end = headings[index + 1..]
        .iter()
        .find(|(next, _, _)| next <= level)
        .map_or(text.len(), |(_, start, _)| *start);
    Ok(bounded(&text[*start..end]))
}

async fn help(
    args: &[String],
    cwd: &Path,
    launcher: &dyn Launcher,
    cancellation: &CancellationToken,
) -> Result<String, String> {
    let program = program::resolve(OsStr::new(&args[0]), cwd).map_err(|error| error.to_string())?;
    let launch = Launch {
        program,
        args: args[1..]
            .iter()
            .map(OsString::from)
            .chain([OsString::from("--help")])
            .collect(),
        cwd: cwd.to_owned(),
        timeout: HELP_TIMEOUT,
        output_limit: TEXT_BYTES,
    };
    let finished = launcher
        .run(launch, cancellation)
        .await
        .map_err(|error| match error {
            LaunchError::TimedOut => "Help timed out.".to_owned(),
            error => error.to_string(),
        })?;
    if !finished.status.success() {
        return Err(format!(
            "Help exited unsuccessfully ({}).\n{}",
            crate::platform::exit_description(&finished.status),
            String::from_utf8_lossy(&finished.stderr)
        ));
    }
    let mut bytes = finished.stdout;
    bytes.extend(finished.stderr);
    if finished.truncated || bytes.len() > TEXT_BYTES {
        return Err("Help output exceeds 64 KiB.".into());
    }
    Ok(String::from_utf8(crate::result::clean_output(&bytes)).expect("clean output is UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_inputs_reject_missing_declarations_and_unexpected_model_fields() {
        for (builtin, args, input) in [
            (Builtin::Read, vec![], json!({})),
            (Builtin::Read, vec!["notes".into()], json!({"path":"other"})),
            (Builtin::Help, vec!["program".into()], json!({"extra":true})),
            (Builtin::Section, vec!["notes".into()], json!({})),
            (
                Builtin::Section,
                vec!["notes".into(), "Title".into()],
                json!({"heading":"Other"}),
            ),
        ] {
            assert!(FixedInput::parse(builtin, &args, input).is_err());
        }
        assert!(matches!(
            FixedInput::parse(Builtin::Section, &["notes".into()], json!({"heading":"Title"})),
            Ok(FixedInput::Section { heading, .. }) if heading == "Title"
        ));
    }

    #[test]
    fn sections_follow_markdown_headings_not_code_blocks() {
        let text = "# Top\nintro\n## Exact\nbody\n### Child\nchild\n```md\n## Fake\n```\n## Next\nnext\n\nSetext\n======\nlast\n";
        assert_eq!(
            section(text, "Exact").unwrap(),
            "## Exact\nbody\n### Child\nchild\n```md\n## Fake\n```\n"
        );
        assert_eq!(section(text, "Setext").unwrap(), "Setext\n======\nlast\n");
        let error = section(text, "missing").unwrap_err();
        assert!(error.contains("Top\nExact\nChild\nNext\nSetext"));
        assert!(!error.contains("Fake"));
    }
}
