//! Fixed declarations shared by the Agent/Human registries and standalone CLI.
use std::{ffi::OsStr, io::Read, path::Path, process::Stdio, time::Duration};

use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio_util::sync::CancellationToken;

use crate::{
    Builtin, Content, ToolResult, program,
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

pub async fn call_fixed(
    builtin: Builtin,
    args: &[String],
    input: Value,
    root: &Path,
    scope: &Scope,
    owner: &ArtifactId,
    cancellation: &CancellationToken,
) -> ToolResult {
    if cancellation.is_cancelled() {
        return ToolResult::error("Tool call was cancelled.");
    }
    if let Err(error) = validate_args(builtin, Some(args), false) {
        return ToolResult::error(error);
    }
    if builtin == Builtin::Help {
        let cwd = root.join(scope.artifacts[owner].folder());
        return match help(args, &cwd, cancellation).await {
            Ok(text) => text_result(text),
            Err(error) => ToolResult::error(bounded(&error)),
        };
    }
    let root = root.to_owned();
    let scope = Scope {
        artifacts: scope.artifacts.clone(),
    };
    let owner = owner.clone();
    let args = args.to_vec();
    let cancellation = cancellation.child_token();
    let _cancel_on_drop = cancellation.clone().drop_guard();
    tokio::task::spawn_blocking(move || {
        call_text(builtin, &args, input, &root, &scope, &owner, &cancellation)
    })
    .await
    .unwrap_or_else(|_| ToolResult::error("Built-in tool execution failed."))
}

fn call_text(
    builtin: Builtin,
    args: &[String],
    input: Value,
    root: &Path,
    scope: &Scope,
    owner: &ArtifactId,
    cancellation: &CancellationToken,
) -> ToolResult {
    let result = match builtin {
        Builtin::Read => {
            read_text(root, scope, owner, &args[0], TEXT_BYTES).map(|text| bounded(&text))
        }
        Builtin::Section => {
            let heading = args
                .get(1)
                .map(String::as_str)
                .or_else(|| input["heading"].as_str());
            match heading {
                Some(heading) => read_text(root, scope, owner, &args[0], DOCUMENT_BYTES)
                    .and_then(|text| section(&text, heading)),
                None => Err("Section requires heading.".into()),
            }
        }
        Builtin::List => {
            let input = super::Input::parse(Builtin::List, json!({"path":args[0]}))
                .expect("fixed list input");
            return super::call(input, root, scope, owner, cancellation);
        }
        _ => Err("This builtin cannot run as a fixed Agent tool.".into()),
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
    let file = scope::open_input(
        root,
        &scope.artifacts[&location.artifact_id],
        &location.path,
    )
    .map_err(|error| error.to_string())?;
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

async fn capture(reader: impl AsyncRead + Unpin) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take((TEXT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .await
        .map_err(|error| error.to_string())?;
    if bytes.len() > TEXT_BYTES {
        return Err("Help output exceeds 64 KiB.".into());
    }
    Ok(bytes)
}

async fn help(
    args: &[String],
    cwd: &Path,
    cancellation: &CancellationToken,
) -> Result<String, String> {
    let program = program::resolve(OsStr::new(&args[0]), cwd).map_err(|error| error.to_string())?;
    let mut child = tokio::process::Command::new(program)
        .args(&args[1..])
        .arg("--help")
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| error.to_string())?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let execution = async {
        let (stdout, stderr, status) = tokio::try_join!(capture(stdout), capture(stderr), async {
            child.wait().await.map_err(|error| error.to_string())
        })?;
        if !status.success() {
            return Err(format!(
                "Help exited unsuccessfully ({status}).\n{}",
                String::from_utf8_lossy(&stderr)
            ));
        }
        let mut bytes = stdout;
        bytes.extend(stderr);
        if bytes.len() > TEXT_BYTES {
            return Err("Help output exceeds 64 KiB.".into());
        }
        Ok(String::from_utf8(crate::result::clean_output(&bytes)).expect("clean output is UTF-8"))
    };
    tokio::select! {
        _ = cancellation.cancelled() => Err("Tool call was cancelled.".into()),
        result = tokio::time::timeout(HELP_TIMEOUT, execution) => result.map_err(|_| "Help timed out.".to_owned())?,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
