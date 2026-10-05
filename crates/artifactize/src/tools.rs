//! Scoped audience-specific tools and language-neutral command protocols.

pub mod builtin;
pub mod human;
mod image;
pub mod pins;
mod result;
pub(crate) mod schema;

pub use result::{Content, ToolResult};

use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::{
    config::{AgentTool, CommandTool, Profile, RepoConfig, ToolProtocol},
    runtime,
    scope::{self, Scope},
    workspace,
};

const DEFAULT_TIMEOUT_MS: u32 = 120_000;
const OUTPUT_LIMIT: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDefinition {
    pub name: String,
    pub artifact_id: String,
    pub description: String,
    pub input_schema: Value,
}

struct RegisteredTool<'a> {
    definition: ToolDefinition,
    declaration: &'a AgentTool,
    validator: jsonschema::Validator,
}

struct Invocation {
    program: OsString,
    argv: Vec<OsString>,
    context: Value,
    protocol: ToolProtocol,
    timeout_ms: u32,
}

/// Borrows the resolved declarations; discovery and listing never execute owner code.
/// Calls own their temporary directories, leaving the caller's output root intact.
pub struct Registry<'a> {
    config: &'a RepoConfig,
    scope: Scope<'a>,
    tools: BTreeMap<String, RegisteredTool<'a>>,
}

impl<'a> Registry<'a> {
    pub fn new(config: &'a RepoConfig, eval_id: &str) -> Result<Self, String> {
        let eval = config
            .evals
            .iter()
            .find(|eval| eval.id == eval_id)
            .ok_or_else(|| format!("Unknown eval: {eval_id}"))?;
        if !matches!(eval.declaration.profile, Profile::Agent { .. }) {
            return Err("Agent tools require an Agent eval.".into());
        }
        Self::with_scope(
            config,
            scope::eval_scope(config, eval).map_err(|e| e.to_string())?,
        )
    }

    pub fn for_artifact(config: &'a RepoConfig, artifact_id: &str) -> Result<Self, String> {
        Self::with_scope(
            config,
            scope::artifact_scope(config, &[artifact_id]).map_err(|e| e.to_string())?,
        )
    }

    fn with_scope(config: &'a RepoConfig, scope: Scope<'a>) -> Result<Self, String> {
        let mut tools = BTreeMap::new();
        for (id, artifact) in &scope.artifacts {
            for (operation, declaration) in &artifact.views.agent_tools {
                let name = format!("{operation}_{id}");
                let description = match declaration {
                    AgentTool::Command(tool) => tool.description.clone(),
                    AgentTool::Builtin(tool) => tool
                        .description
                        .clone()
                        .unwrap_or_else(|| builtin::description(tool.builtin).into()),
                }
                .replace("{artifactName}", id);
                let input_schema = declaration.input_schema();
                let validator = schema::compile(&input_schema)?;
                let entry = RegisteredTool {
                    definition: ToolDefinition {
                        name: name.clone(),
                        artifact_id: (*id).into(),
                        description,
                        input_schema,
                    },
                    declaration,
                    validator,
                };
                if tools.insert(name.clone(), entry).is_some() {
                    return Err(format!("Artifact tool names collide: {name}"));
                }
            }
        }
        Ok(Self {
            config,
            scope,
            tools,
        })
    }

    pub fn list(&self) -> impl Iterator<Item = &ToolDefinition> {
        self.tools.values().map(|tool| &tool.definition)
    }

    /// Resolve static prerequisites without substituting free arguments.
    pub fn preflight(&self, name: &str) -> Result<(), String> {
        let tool = self
            .tools
            .get(name)
            .ok_or("Unknown registered Agent tool.")?;
        let owner = &tool.definition.artifact_id;
        self.scope
            .resolve_input(&self.config.root, owner, "")
            .map_err(|e| e.to_string())?;
        if let AgentTool::Command(command) = tool.declaration {
            preflight_executable(&self.config.root, &self.scope, owner, &command.command)?;
            if matches!(command.protocol, ToolProtocol::Json) {
                scope::resolve_argv(self.config, &self.scope, owner, &command.args)
                    .map_err(|e| e.to_string())?;
            }
            for path in &command.execution_paths {
                scope::scoped_path(&self.config.root, Path::new(path))
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    pub async fn call(
        &self,
        name: &str,
        args: Value,
        output_root: &Path,
        cancellation: CancellationToken,
    ) -> ToolResult {
        let Some(tool) = self.tools.get(name) else {
            return ToolResult::error("Unknown registered Agent tool.");
        };
        if let Err(message) = schema::validate(&tool.validator, &args) {
            return ToolResult::error(message);
        }
        if cancellation.is_cancelled() {
            return ToolResult::error("Agent tool call was cancelled.");
        }
        let command = match tool.declaration {
            AgentTool::Command(command) => command,
            AgentTool::Builtin(tool_declaration) => {
                let builtin = tool_declaration.builtin;
                let root = self.config.root.clone();
                let owner = tool.definition.artifact_id.clone();
                let artifacts: BTreeMap<_, _> = self
                    .scope
                    .artifacts
                    .iter()
                    .map(|(id, artifact)| ((*id).to_owned(), (*artifact).clone()))
                    .collect();
                let cancellation = cancellation.child_token();
                let _cancel_on_drop = cancellation.clone().drop_guard();
                return tokio::task::spawn_blocking(move || {
                    let scope = Scope {
                        artifacts: artifacts
                            .iter()
                            .map(|(id, artifact)| (id.as_str(), artifact))
                            .collect(),
                    };
                    builtin::call(builtin, &root, &scope, &owner, &args, &cancellation)
                })
                .await
                .unwrap_or_else(|_| ToolResult::error("Built-in Agent tool execution failed."));
            }
        };
        let invocation = match self.prepare(&tool.definition.artifact_id, command, &args) {
            Ok(invocation) => invocation,
            Err(()) => return ToolResult::error("Agent tool preparation failed."),
        };
        let workspace = self.config.root.clone();
        let output_root = output_root.to_owned();
        let cancellation = cancellation.child_token();
        let _cancel_on_drop = cancellation.clone().drop_guard();
        // Keep cleanup alive if the caller drops its future while a command is running.
        tokio::spawn(async move {
            invoke(invocation, args, &workspace, &output_root, cancellation).await
        })
        .await
        .unwrap_or_else(|_| ToolResult::error("Agent tool execution failed."))
    }

    fn prepare(&self, owner: &str, tool: &CommandTool, args: &Value) -> Result<Invocation, ()> {
        let cwd = self
            .scope
            .resolve_input(&self.config.root, owner, "")
            .map_err(|_| ())?;
        let program = executable(&self.config.root, &self.scope, owner, &tool.command)?;
        let argv = match tool.protocol {
            ToolProtocol::Json => {
                scope::resolve_argv(self.config, &self.scope, owner, &tool.args).map_err(|_| ())?
            }
            ToolProtocol::Plain => tool
                .args
                .iter()
                .map(|arg| {
                    plain_argument(arg, |name| {
                        if tool.input_schema["properties"].get(name).is_none() {
                            return Err("Undeclared plain tool placeholder.".into());
                        }
                        let value = args.get(name).ok_or("Missing plain tool argument.")?;
                        let value = match value {
                            Value::String(value) => value.clone(),
                            _ => value.to_string(),
                        };
                        if value.contains('\0') {
                            return Err("Plain tool arguments cannot contain NUL.".into());
                        }
                        Ok(value)
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| ())?,
        };
        let mut scoped = serde_json::Map::new();
        for (id, artifact) in &self.scope.artifacts {
            let path = scope::scoped_path(&self.config.root, &artifact.path).map_err(|_| ())?;
            let mut entry =
                json!({"path":path,"children":artifact.children,"mounts":artifact.mounts});
            if let Some(family) = &artifact.family {
                for material in &family.material {
                    scope::scoped_path(&path, Path::new(material)).map_err(|_| ())?;
                }
                entry["family"] = json!({"name":family.name,"material":family.material});
            }
            scoped.insert((*id).into(), entry);
        }
        let mut context = json!({"artifactId":owner,"artifactPath":cwd,"scope":scoped});
        if !tool.execution_paths.is_empty() {
            let mut paths = serde_json::Map::new();
            for path in &tool.execution_paths {
                let resolved =
                    scope::scoped_path(&self.config.root, Path::new(path)).map_err(|_| ())?;
                paths.insert(path.clone(), json!(resolved));
            }
            context["executionPaths"] = paths.into();
        }
        Ok(Invocation {
            program,
            argv: argv.into_iter().map(Into::into).collect(),
            context,
            protocol: tool.protocol,
            timeout_ms: tool.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS),
        })
    }
}

fn executable(root: &Path, scope: &Scope<'_>, owner: &str, command: &str) -> Result<OsString, ()> {
    if !Path::new(command).is_absolute() && command.contains('/') {
        let relative = command.strip_prefix("./").unwrap_or(command);
        let program = scope.resolve_input(root, owner, relative).map_err(|_| ())?;
        if !program.is_file() {
            return Err(());
        }
        Ok(program.into_os_string())
    } else {
        Ok(command.into())
    }
}

fn preflight_executable(
    root: &Path,
    scope: &Scope<'_>,
    owner: &str,
    command: &str,
) -> Result<(), String> {
    let program = executable(root, scope, owner, command)
        .map_err(|_| "Tool executable path is unavailable or outside scope.".to_owned())?;
    let found = if Path::new(&program).is_absolute() {
        crate::platform::is_executable(Path::new(&program))
    } else {
        let cwd = scope
            .resolve_input(root, owner, "")
            .map_err(|e| e.to_string())?;
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .any(|dir| crate::platform::is_executable(&cwd.join(dir).join(&program)))
    };
    if found {
        Ok(())
    } else {
        Err(format!("Tool executable is unavailable: {command}"))
    }
}

async fn invoke(
    invocation: Invocation,
    args: Value,
    workspace: &Path,
    output_root: &Path,
    cancellation: CancellationToken,
) -> ToolResult {
    let run = || {
        let root = workspace::prepare_directory(output_root, workspace).map_err(|_| ())?;
        tempfile::Builder::new()
            .prefix("tool-")
            .tempdir_in(root)
            .map_err(|_| ())
    };
    let Ok(directory) = run() else {
        return ToolResult::error("Agent tool preparation failed.");
    };
    let Invocation {
        program,
        argv,
        mut context,
        protocol,
        timeout_ms,
    } = invocation;
    let result = async {
        let mut command =
            runtime::Command::prepare(program, argv, workspace, directory.path(), Some(timeout_ms))
                .map_err(|_| "Agent tool preparation failed.")?;
        command.cwd = PathBuf::from(
            context["artifactPath"]
                .as_str()
                .ok_or("Agent tool preparation failed.")?,
        );
        let output_dir = command.directory().join("output");
        context["outputDir"] = json!(output_dir);
        context["tmpDir"] = json!(command.directory().join("tmp"));
        let input = match protocol {
            ToolProtocol::Json => json!({"version":1,"context":context,"args":args})
                .to_string()
                .into_bytes(),
            ToolProtocol::Plain => Vec::new(),
        };
        let output = command
            .tool_output(input, OUTPUT_LIMIT, cancellation.clone())
            .await
            .map_err(|error| match error {
                crate::process::Error::Timeout => "Agent tool timed out.",
                crate::process::Error::Cancelled => "Agent tool call was cancelled.",
                _ => "Agent tool execution failed.",
            })?;
        match protocol {
            ToolProtocol::Json => {
                if !output.status.success() {
                    return Err("Agent tool execution failed.");
                }
                if output.truncated {
                    return Err("Agent tool returned invalid output.");
                }
                result::parse(&output.stdout, &output_dir)
                    .map_err(|_| "Agent tool returned invalid output.")
            }
            ToolProtocol::Plain => Ok(result::plain(&output)),
        }
    }
    .await;
    let cleaned = directory.close();
    if cancellation.is_cancelled() {
        return ToolResult::error("Agent tool call was cancelled.");
    }
    if cleaned.is_err() {
        return ToolResult::error("Agent tool cleanup failed.");
    }
    result.unwrap_or_else(ToolResult::error)
}

/// Single-pass expansion: substituted values are never interpreted as templates.
/// Doubled braces represent literal braces in a plain argv template.
pub(crate) fn plain_argument(
    template: &str,
    mut value: impl FnMut(&str) -> Result<String, String>,
) -> Result<String, String> {
    let mut result = String::new();
    let mut rest = template;
    while !rest.is_empty() {
        if rest.starts_with("{{") || rest.starts_with("}}") {
            result.push(rest.chars().next().unwrap());
            rest = &rest[2..];
        } else if let Some(tail) = rest.strip_prefix('{') {
            let (name, tail) = tail
                .split_once('}')
                .ok_or("Unclosed plain tool placeholder.")?;
            if name.is_empty() || name.contains('{') {
                return Err("Invalid plain tool placeholder.".into());
            }
            result.push_str(&value(name)?);
            rest = tail;
        } else {
            let character = rest.chars().next().unwrap();
            if character == '}' {
                return Err("Unmatched plain tool closing brace.".into());
            }
            result.push(character);
            rest = &rest[character.len_utf8()..];
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
