//! The provider-independent Chat tool set, with file and Verify operations scoped to one worktree.
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use artifactize_tools::{
    Builtin, builtin, files,
    scope::{self, Artifact, ArtifactId, Scope},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::{
    runs, runtime,
    tools::{Content, ToolDefinition, ToolResult},
    types::RunId,
    verify_tools,
};

/// Bound each write and the amount an edit can read before replacing a single span.
const EDIT_BYTES: usize = 4 * 1024 * 1024;
/// Commands are stoppable and cannot occupy a Chat tool turn indefinitely.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(120);
const READ_TOOLS: [(Builtin, &str); 4] = [
    (Builtin::Read, "read"),
    (Builtin::List, "list"),
    (Builtin::Glob, "glob"),
    (Builtin::Grep, "grep"),
];

pub(crate) struct Tools<'a> {
    context: &'a runs::Context,
    output: &'a Path,
    checkout: Option<Option<&'a crate::repository::Identity>>,
    owner: ArtifactId,
    scope: Scope,
    definitions: Vec<ToolDefinition>,
}

pub(crate) struct Called {
    pub result: ToolResult,
    pub run: Option<RunId>,
}

impl From<ToolResult> for Called {
    fn from(result: ToolResult) -> Self {
        Self { result, run: None }
    }
}

impl<'a> Tools<'a> {
    pub(crate) fn new(context: &'a runs::Context, output: &'a Path) -> Self {
        let owner: ArtifactId =
            ArtifactId::new("worktree").expect("worktree is an Artifact identifier");
        let artifact = Artifact {
            path: scope::Target::Folder(
                PathBuf::new()
                    .try_into()
                    .expect("the root is a local target"),
            ),
            name: "Worktree".into(),
            nested: Default::default(),
            covering_part: None,
        };
        let scope = Scope {
            artifacts: [(owner.clone(), artifact)].into(),
        };
        let tool = |name: &str, description: String, input_schema: Value| ToolDefinition {
            name: name.parse().expect("built-in Chat tool name"),
            artifact_id: "worktree".parse().expect("tool owner name"),
            description,
            input_schema,
        };
        let mut definitions: Vec<_> = READ_TOOLS
            .iter()
            .map(|&(builtin, name)| {
                tool(
                    name,
                    builtin::description(builtin).to_owned(),
                    builtin::input_schema(builtin),
                )
            })
            .collect();
        definitions.extend([
            tool("write", "Write UTF-8 text to a file under the worktree. The parent directory must exist.".into(),
                json!({"type": "object", "properties": {"path":{"type":"string"},"text":{"type":"string"}}, "required":["path","text"], "additionalProperties":false})),
            tool("edit", "Replace exactly one occurrence of old_text with new_text in a UTF-8 worktree file.".into(),
                json!({"type":"object","properties":{"path":{"type":"string"},"old_text":{"type":"string","minLength":1},"new_text":{"type":"string"}},"required":["path","old_text","new_text"],"additionalProperties":false})),
            tool("run_command", "Run a program and its argument vector from the worktree, using the author's terminal environment. Optional cwd is relative to the worktree.".into(),
                json!({"type":"object","properties":{"program":{"type":"string","minLength":1},"args":{"type":"array","items":{"type":"string"}},"cwd":{"type":"string"}},"required":["program"],"additionalProperties":false})),
        ]);
        definitions.extend(
            verify_tools::tools()
                .into_iter()
                .filter(|tool| verify_tools::chat_tool(tool.name))
                .map(|verify| {
                    let mut schema = verify.schema;
                    if let Some(Value::Object(properties)) = schema.get_mut("properties") {
                        properties.remove("project");
                    }
                    tool(
                        verify.name,
                        verify.description.into_owned(),
                        Value::Object(schema),
                    )
                }),
        );
        Self {
            context,
            output,
            checkout: None,
            owner,
            scope,
            definitions,
        }
    }

    pub(crate) fn guard_checkout(
        mut self,
        identity: Option<&'a crate::repository::Identity>,
    ) -> Self {
        self.checkout = Some(identity);
        self
    }

    pub(crate) fn output(&self) -> &Path {
        self.output
    }

    pub(crate) fn list(&self) -> impl Iterator<Item = &ToolDefinition> {
        self.definitions.iter()
    }

    pub(crate) async fn call(
        &self,
        name: &str,
        arguments: Value,
        cancellation: CancellationToken,
    ) -> Called {
        match self.execute(name, arguments, cancellation).await {
            Ok(result) => result,
            Err(error) => ToolResult::error(error).into(),
        }
    }

    async fn execute(
        &self,
        name: &str,
        arguments: Value,
        cancellation: CancellationToken,
    ) -> Result<Called, String> {
        if cancellation.is_cancelled() {
            return Err("The Chat turn was stopped.".into());
        }
        let root = self.context.repo();
        if let Some(identity) = self.checkout {
            let path = root.to_owned();
            let identity = identity.cloned();
            crate::task::blocking(move || super::verify_checkout(&path, identity.as_ref()))
                .await??;
        }
        if let Some((builtin, _)) = READ_TOOLS.iter().find(|(_, tool)| *tool == name) {
            let input = builtin::Input::parse(*builtin, arguments)?;
            return Ok(builtin::call(input, root, &self.scope, &self.owner, &cancellation).into());
        }
        match name {
            "write" => {
                let WriteFile { path, text } = decode(arguments)?;
                let target = self.file(&path)?;
                write_file(&target, &text)?;
                Ok(ToolResult::success(Content::Text {
                    text: format!("Wrote {path}."),
                })
                .into())
            }
            "edit" => {
                let EditFile {
                    path,
                    old_text,
                    new_text,
                } = decode(arguments)?;
                let file = scope::open_scoped(root, &path).map_err(|error| error.to_string())?;
                if !file
                    .metadata()
                    .map_err(|error| error.to_string())?
                    .is_file()
                {
                    return Err("An edit needs an existing file.".into());
                }
                let mut text = String::new();
                file.take(EDIT_BYTES as u64 + 1)
                    .read_to_string(&mut text)
                    .map_err(|error| error.to_string())?;
                if text.len() > EDIT_BYTES || old_text.is_empty() {
                    return Err(
                        "An edit needs a nonempty old_text and a file within the edit limit."
                            .into(),
                    );
                }
                let mut matches = text.match_indices(&old_text);
                let Some((start, _)) = matches.next() else {
                    return Err("old_text does not occur in the file.".into());
                };
                if matches.next().is_some() {
                    return Err("old_text occurs more than once; supply a unique span.".into());
                }
                text.replace_range(start..start + old_text.len(), &new_text);
                write_file(&self.file(&path)?, &text)?;
                Ok(ToolResult::success(Content::Text {
                    text: format!("Edited {path}."),
                })
                .into())
            }
            "run_command" => {
                let Command { program, args, cwd } = decode(arguments)?;
                let cwd = if cwd == "." { "" } else { &cwd };
                let folder = self
                    .scope
                    .resolve_input(root, &self.owner, cwd)
                    .map_err(|error| error.to_string())?;
                let program =
                    crate::platform::program_path(&program).map_err(|error| error.to_string())?;
                let command = runtime::Command::prepare(
                    program.into_os_string(),
                    args.into_iter().map(Into::into).collect(),
                    &folder,
                    self.output,
                    Some(COMMAND_TIMEOUT),
                )
                .map_err(|error| error.to_string())?;
                let output = command
                    .output(Vec::new(), cancellation)
                    .await
                    .map_err(|error| error.to_string())?;
                let text = |bytes: &[u8]| {
                    String::from_utf8_lossy(&runtime::clean_output(bytes)).into_owned()
                };
                let stdout = text(&output.stdout);
                let stderr = text(&output.stderr);
                let Some(exit_code) = output.status.code() else {
                    return Err(format!(
                        "Command did not succeed: {}\n{stdout}\n{stderr}",
                        output.status
                    ));
                };
                let command = super::details::CommandOutput {
                    exit_code,
                    stdout: stdout.into(),
                    stderr: stderr.into(),
                    truncated: output.truncated,
                };
                let result = if exit_code == 0 {
                    ToolResult::success(Content::Json {
                        data: serde_json::to_value(command).map_err(|error| error.to_string())?,
                    })
                } else {
                    // A ToolResult error is one text block. Use the same JSON envelope as the
                    // success response so the transcript doesn't infer a numeric status from prose.
                    ToolResult::error(
                        serde_json::to_string(&command).map_err(|error| error.to_string())?,
                    )
                };
                Ok(result.into())
            }
            _ => {
                if !verify_tools::chat_tool(name) {
                    return Err(format!("Unknown Chat tool: {name}"));
                }
                let arguments = match arguments {
                    Value::Object(arguments) => arguments,
                    _ => return Err("Tool arguments must be an object.".into()),
                };
                let call =
                    verify_tools::Call::parse(name, arguments, time::OffsetDateTime::now_utc())
                        .map_err(|error| error.to_string())?;
                if call.project.is_some() {
                    return Err("Chat Verify tools are scoped to this session's worktree.".into());
                }
                let started = call.started_run();
                let result = call
                    .execute(self.context, cancellation, |_| async { Ok(()) })
                    .await
                    .map_err(|error| error.to_string())?;
                let run = started.then(|| result.observed_run().cloned()).flatten();
                Ok(Called {
                    result: ToolResult::success(Content::Json {
                        data: serde_json::to_value(result).map_err(|error| error.to_string())?,
                    }),
                    run,
                })
            }
        }
    }

    /// Resolve the existing parent without following links; creation is limited to one entry.
    fn file(&self, path: &str) -> Result<PathBuf, String> {
        scope::logical_path(path).map_err(|error| error.to_string())?;
        let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
        if name.is_empty() {
            return Err("A file path is required.".into());
        }
        let parent = self
            .scope
            .resolve_input(self.context.repo(), &self.owner, parent)
            .map_err(|error| error.to_string())?;
        Ok(parent.join(files::path_from_text(name).map_err(|error| error.to_string())?))
    }
}

fn write_file(path: &Path, text: &str) -> Result<(), String> {
    if text.len() > EDIT_BYTES {
        return Err(format!("File content exceeds {EDIT_BYTES} bytes."));
    }
    let mut file =
        crate::platform::paths::open_portable_write(path).map_err(|error| error.to_string())?;
    file.set_len(0)
        .and_then(|()| file.write_all(text.as_bytes()))
        .map_err(|error| error.to_string())
}

fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteFile {
    path: String,
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditFile {
    path: String,
    old_text: String,
    new_text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Command {
    program: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    cwd: String,
}
