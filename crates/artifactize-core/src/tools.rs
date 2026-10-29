//! Scoped audience-specific tools and language-neutral command protocols.

pub use artifactize_tools::builtin;
pub mod human;
#[cfg(test)]
mod image;
pub mod pins;
mod result;
pub mod sessions;
pub(crate) use artifactize_tools::schema;

pub use artifactize_tools::{Content, ToolResult};

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
    scope::{self, ArtifactId, Scope},
    workspace,
};

/// A command tool gets two minutes unless its owner declares a deadline.
const DEFAULT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);
/// Capture enough for an 8 MiB normalized multimodal result plus its wire envelope,
/// but bound raw tool stdout/stderr; truncated JSON is rejected rather than silently reused.
const OUTPUT_LIMIT: usize = 16 * 1024 * 1024;
/// Version 1 identifies the JSON command-tool stdin envelope carrying context and args;
/// owner tools can distinguish future envelopes independently of fingerprint or state formats.
const JSON_TOOL_INPUT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDefinition {
    pub name: crate::config::ToolName,
    pub artifact_id: crate::config::ArtifactName,
    pub description: String,
    pub input_schema: Value,
}

struct RegisteredTool<'a> {
    definition: ToolDefinition,
    declaration: &'a AgentTool,
    validator: jsonschema::Validator,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolContext {
    artifact_id: crate::config::ArtifactName,
    #[serde(with = "crate::platform::path_serde")]
    artifact_path: PathBuf,
    scope: BTreeMap<crate::config::ArtifactName, ContextArtifact>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    execution_paths: BTreeMap<crate::config::LogicalPath, ContextPath>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::platform::path_serde::option"
    )]
    output_dir: Option<PathBuf>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::platform::path_serde::option"
    )]
    tmp_dir: Option<PathBuf>,
}
#[derive(Serialize)]
struct ContextArtifact {
    #[serde(with = "crate::platform::path_serde")]
    path: PathBuf,
    kind: crate::config::ArtifactKind,
    nested: BTreeMap<crate::config::ChildPrefix, crate::config::ArtifactName>,
}
#[derive(Serialize)]
struct ContextPath(#[serde(with = "crate::platform::path_serde")] PathBuf);

struct Invocation {
    cwd: PathBuf,
    program: OsString,
    argv: Vec<OsString>,
    context: ToolContext,
    protocol: ToolProtocol,
    timeout_ms: std::time::Duration,
}

/// Borrows the resolved declarations; discovery and listing never execute owner code.
/// Calls own their temporary directories, leaving the caller's output root intact.
pub struct Registry<'a> {
    config: &'a RepoConfig,
    scope: Scope<'a>,
    tools: BTreeMap<crate::config::ToolName, RegisteredTool<'a>>,
    sessions: Option<&'a sessions::Sessions>,
}

impl<'a> Registry<'a> {
    pub fn new(config: &'a RepoConfig, eval_id: &str) -> Result<Self, String> {
        let eval = config
            .evals
            .iter()
            .find(|eval| eval.id == eval_id)
            .ok_or_else(|| format!("Unknown eval: {eval_id}"))?;
        if !matches!(eval.declaration.profile(), Profile::Agent { .. }) {
            return Err("Agent tools require an Agent eval.".into());
        }
        Self::with_scope(
            config,
            scope::eval_scope(config, eval).map_err(|e| e.to_string())?,
        )
    }

    pub fn for_artifact(config: &'a RepoConfig, artifact_id: &str) -> Result<Self, String> {
        let artifact_id: crate::config::ArtifactName = artifact_id.parse()?;
        Self::with_scope(
            config,
            scope::artifact_scope(config, &[&artifact_id]).map_err(|e| e.to_string())?,
        )
    }

    fn with_scope(config: &'a RepoConfig, scope: Scope<'a>) -> Result<Self, String> {
        let mut tools = BTreeMap::new();
        let mut session_names = BTreeMap::new();
        for (id, artifact) in &scope.artifacts {
            for (operation, declaration) in &artifact.views.agent_tools {
                if let AgentTool::Command(command) = declaration
                    && let Some(session) = &command.session
                {
                    let key = format!("{session}_{}", id.tool_suffix());
                    let identity = (id, session);
                    if session_names
                        .insert(key.clone(), identity)
                        .is_some_and(|previous| previous != identity)
                    {
                        return Err(format!("Artifact tool session names collide: {key}"));
                    }
                }
                let name: crate::config::ToolName = format!("{operation}_{}", id.tool_suffix())
                    .parse()
                    .expect("validated tool and Artifact names");
                let description = match declaration {
                    AgentTool::Command(tool) => tool.description.clone(),
                    AgentTool::Builtin(tool) => {
                        tool.description
                            .clone()
                            .unwrap_or_else(|| match &tool.args {
                                Some(args) => builtin::fixed_description(tool.builtin, args).into(),
                                None => builtin::description(tool.builtin).into(),
                            })
                    }
                }
                .replace("{artifactName}", id);
                let input_schema = declaration.input_schema();
                let validator = schema::compile(&input_schema)?;
                let entry = RegisteredTool {
                    definition: ToolDefinition {
                        name: name.clone(),
                        artifact_id: artifact.name.clone(),
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
            sessions: None,
        })
    }

    pub(crate) fn with_session_scope<'b>(
        &'b self,
        sessions: &'b sessions::Sessions,
    ) -> Result<Registry<'b>, String> {
        Registry::with_scope(self.config, self.scope.clone())
            .map(|registry| registry.with_sessions(sessions))
    }

    pub fn with_sessions(mut self, sessions: &'a sessions::Sessions) -> Self {
        self.sessions = Some(sessions);
        self
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
            if let Some(name) = &command.session {
                let session = &self.scope.artifacts[owner].views.agent_sessions[name];
                for command in [&session.start, &session.stop] {
                    preflight_executable(&self.config.root, &self.scope, owner, &command.command)?;
                    scope::resolve_argv(self.config, &self.scope, owner, &command.args)
                        .map_err(|error| error.to_string())?;
                    for path in &command.execution_paths {
                        scope::scoped_path(&self.config.root, Path::new(path))
                            .map_err(|error| error.to_string())?;
                    }
                }
            }
            if matches!(command.protocol, ToolProtocol::Json) {
                scope::resolve_argv(self.config, &self.scope, owner, &command.args)
                    .map_err(|e| e.to_string())?;
            }
            for path in &command.execution_paths {
                scope::scoped_path(&self.config.root, Path::new(path))
                    .map_err(|e| e.to_string())?;
            }
        }
        if let AgentTool::Builtin(declaration) = tool.declaration
            && let Some(args) = &declaration.args
        {
            let owner = ArtifactId::new(owner.as_str()).map_err(|error| error.to_string())?;
            builtin::validate_target(
                declaration.builtin,
                args,
                &self.config.root,
                &self.scope.tool_scope(),
                &owner,
            )?;
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
        for artifact in self.scope.artifacts.values() {
            if let Err(error) = scope::validate_file_target(&self.config.root, artifact) {
                return ToolResult::error(error.to_string());
            }
        }
        let command = match tool.declaration {
            AgentTool::Command(command) => command,
            AgentTool::Builtin(tool_declaration) => {
                if tool_declaration.builtin == crate::config::Builtin::Graph {
                    return match crate::graph_structure::read(&self.config.root) {
                        Ok(graph) => ToolResult::success(Content::Text { text: graph.tree() }),
                        Err(error) => ToolResult::error(error),
                    };
                }
                if let Some(fixed_args) = &tool_declaration.args {
                    let owner = scope::tool_id(&tool.definition.artifact_id);
                    // An Agent's help program gets only the explicit standalone variables.
                    let launcher = crate::process::Launcher {
                        env: artifactize_tools::launch::passed_environment(),
                    };
                    let scope = self.scope.tool_scope();
                    let context = builtin::Context {
                        root: &self.config.root,
                        scope: &scope,
                        owner: &owner,
                        launcher: &launcher,
                    };
                    return builtin::call_fixed(
                        tool_declaration.builtin,
                        fixed_args,
                        args,
                        context,
                        &cancellation,
                    )
                    .await;
                }
                let input = match builtin::Input::parse(tool_declaration.builtin, args) {
                    Ok(input) => input,
                    Err(error) => return ToolResult::error(error),
                };
                let root = self.config.root.clone();
                let owner = scope::tool_id(&tool.definition.artifact_id);
                let scope = self.scope.tool_scope();
                let cancellation = cancellation.child_token();
                let _cancel_on_drop = cancellation.clone().drop_guard();
                return crate::task::joined(
                    tokio::task::spawn_blocking(move || {
                        builtin::call(input, &root, &scope, &owner, &cancellation)
                    })
                    .await,
                )
                .unwrap_or_else(|_| ToolResult::error("Built-in Agent tool execution failed."));
            }
        };
        let environment = if let Some(name) = &command.session {
            let key = sessions::session_key(name, &tool.definition.artifact_id);
            let Some(sessions) = self.sessions else {
                return ToolResult::error(format!("The tool session `{name}` ended."));
            };
            match sessions.call(&key).await {
                Ok(call) => Some(call),
                Err(error) => return ToolResult::error(error),
            }
        } else {
            None
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
        let executor = if environment.is_some() {
            match sessions::executor() {
                Ok(executor) => executor.clone(),
                Err(_) => return ToolResult::error("Agent tool execution failed."),
            }
        } else {
            tokio::runtime::Handle::current()
        };
        crate::task::joined(
            executor
                .spawn(async move {
                    invoke(
                        invocation,
                        args,
                        &workspace,
                        &output_root,
                        cancellation,
                        environment,
                    )
                    .await
                })
                .await,
        )
        .unwrap_or_else(|_| ToolResult::error("Agent tool execution failed."))
    }

    fn prepare_session(
        &self,
        owner: &crate::config::ArtifactName,
        command: &crate::config::SessionCommand,
    ) -> Result<Invocation, String> {
        let tool = CommandTool {
            session: None,
            description: String::new(),
            input_schema: std::sync::Arc::new(json!({"type":"object"})),
            protocol: ToolProtocol::Json,
            command: command.command.clone(),
            args: command.args.clone(),
            timeout_ms: None,
            execution_paths: command.execution_paths.clone(),
            plain_args: Vec::new(),
        };
        self.prepare(owner, &tool, &json!({}))
            .map_err(|()| "Tool session preparation failed.".into())
    }

    fn prepare(
        &self,
        owner: &crate::config::ArtifactName,
        tool: &CommandTool,
        args: &Value,
    ) -> Result<Invocation, ()> {
        let cwd = scope::scoped_path(&self.config.root, self.config.artifacts[owner].folder())
            .map_err(|_| ())?;
        let program = executable(&self.config.root, &self.scope, owner, &tool.command)?;
        let argv = match tool.protocol {
            ToolProtocol::Json => {
                scope::resolve_argv(self.config, &self.scope, owner, &tool.args).map_err(|_| ())?
            }
            ToolProtocol::Plain => tool
                .plain_args
                .iter()
                .map(|arg| {
                    arg.expand(|name| {
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
        let scoped = self
            .scope
            .artifacts
            .values()
            .map(|artifact| {
                let path = scope::scoped_path(&self.config.root, artifact.referenced_path())
                    .map_err(|_| ())?;
                Ok((
                    artifact.name.clone(),
                    ContextArtifact {
                        path,
                        kind: artifact.kind,
                        nested: artifact.nested.clone(),
                    },
                ))
            })
            .collect::<Result<BTreeMap<_, _>, ()>>()?;
        let execution_paths = tool
            .execution_paths
            .iter()
            .map(|path| {
                let resolved =
                    scope::scoped_path(&self.config.root, Path::new(path)).map_err(|_| ())?;
                Ok((path.clone(), ContextPath(resolved)))
            })
            .collect::<Result<_, ()>>()?;
        let owner = &self.scope.artifacts[owner].name;
        let context = ToolContext {
            artifact_id: owner.clone(),
            artifact_path: scoped[owner].path.clone(),
            scope: scoped,
            execution_paths,
            output_dir: None,
            tmp_dir: None,
        };
        Ok(Invocation {
            cwd,
            program,
            argv: argv.into_iter().map(Into::into).collect(),
            context,
            protocol: tool.protocol,
            timeout_ms: tool.timeout_ms.unwrap_or(DEFAULT_TIMEOUT),
        })
    }
}

fn executable(
    root: &Path,
    scope: &Scope<'_>,
    owner: &crate::config::ArtifactName,
    command: &str,
) -> Result<OsString, ()> {
    scope::executable(root, scope, owner, command).map_err(|_| ())
}

fn preflight_executable(
    root: &Path,
    scope: &Scope<'_>,
    owner: &crate::config::ArtifactName,
    command: &str,
) -> Result<(), String> {
    let program = executable(root, scope, owner, command)
        .map_err(|_| "Tool executable path is unavailable or outside scope.".to_owned())?;
    let cwd = scope::scoped_path(root, scope.artifacts[owner].folder())
        .map_err(|error| error.to_string())?;
    crate::platform::program::resolve(&program, &cwd)
        .map(|_| ())
        .map_err(|_| {
            format!(
                "Tool executable is unavailable: {}",
                crate::platform::path_text(Path::new(command))
            )
        })
}

async fn invoke(
    invocation: Invocation,
    args: Value,
    workspace: &Path,
    output_root: &Path,
    cancellation: CancellationToken,
    environment: Option<sessions::ownership::Call>,
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
        cwd,
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
        command.cwd = cwd;
        if let Some(environment) = &environment {
            command.inherit_session(&environment.environment);
        }
        let output_dir = command.directory().join("output");
        context.output_dir = Some(output_dir.clone());
        context.tmp_dir = Some(command.directory().join("tmp"));
        let input = match protocol {
            ToolProtocol::Json => {
                json!({"version":JSON_TOOL_INPUT_VERSION,"context":context,"args":args})
                    .to_string()
                    .into_bytes()
            }
            ToolProtocol::Plain => Vec::new(),
        };
        let output = match environment {
            Some(call) => {
                call.run(
                    command.into_process(),
                    input,
                    OUTPUT_LIMIT,
                    cancellation.clone(),
                )
                .await
            }
            None => {
                command
                    .tool_output(input, OUTPUT_LIMIT, cancellation.clone())
                    .await
            }
        }
        .map_err(|error| {
            error.argument_refusal().unwrap_or_else(|| match error {
                crate::process::Error::Timeout => "Agent tool timed out.".into(),
                crate::process::Error::Cancelled => "Agent tool call was cancelled.".into(),
                _ => "Agent tool execution failed.".into(),
            })
        })?;
        match protocol {
            ToolProtocol::Json => {
                if !output.status.success() {
                    return Err("Agent tool execution failed.".to_owned());
                }
                if output.truncated {
                    return Err("Agent tool returned invalid output.".to_owned());
                }
                result::parse(&output.stdout, &output_dir)
                    .map_err(|_| "Agent tool returned invalid output.".to_owned())
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

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "tools/builtin/tests.rs"]
mod builtin_tests;
