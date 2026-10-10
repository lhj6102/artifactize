//! Predefined reviewer commands, deliberately outside Agent runtime isolation.

use std::{collections::BTreeMap, ffi::OsString, path::PathBuf};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::{
    config::{HumanTool, HumanToolKind, Profile, RepoConfig},
    process, runtime,
    scope::{self, Scope},
};

use super::{DEFAULT_TIMEOUT, executable};

/// Keep Human tool transcripts readable and bounded in CLI/TUI output; truncation is explicit.
const TEXT_LIMIT: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDefinition {
    pub name: crate::config::ToolName,
    pub artifact_id: crate::config::ArtifactName,
    pub description: String,
    pub kind: HumanToolKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum Content {
    Text { text: String },
    Launch { launched: bool },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolResult {
    pub content: Vec<Content>,
    pub is_error: bool,
}

impl ToolResult {
    fn error(message: impl Into<String>) -> Self {
        Self {
            content: vec![Content::Text {
                text: message.into(),
            }],
            is_error: true,
        }
    }
}

/// What a registered tool runs: the repository, resolved program, argv and working directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLine {
    pub repo: PathBuf,
    pub kind: HumanToolKind,
    pub program: OsString,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

struct RegisteredTool<'a> {
    definition: ToolDefinition,
    declaration: &'a HumanTool,
}

/// Human catalog only. Claimant authorization belongs to the request lifecycle.
pub struct Registry<'a> {
    config: &'a RepoConfig,
    scope: Scope<'a>,
    tools: BTreeMap<crate::config::ToolName, RegisteredTool<'a>>,
}

impl<'a> Registry<'a> {
    pub fn new(config: &'a RepoConfig, eval_id: &str) -> Result<Self, String> {
        let eval = config
            .evals
            .iter()
            .find(|eval| eval.id == eval_id)
            .ok_or_else(|| format!("Unknown eval: {eval_id}"))?;
        if !matches!(eval.declaration.profile(), Profile::Human {}) {
            return Err("Human tools require a Human eval.".into());
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
            for (operation, declaration) in &artifact.views.human_tools {
                let name: crate::config::ToolName = format!("{operation}_{id}")
                    .parse()
                    .expect("validated tool and Artifact names");
                let entry = RegisteredTool {
                    definition: ToolDefinition {
                        name: name.clone(),
                        artifact_id: artifact.name.clone(),
                        description: declaration.description().replace("{artifactName}", id),
                        kind: declaration.kind(),
                    },
                    declaration,
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

    pub fn is_command(&self, name: &str) -> bool {
        self.tools
            .get(name)
            .is_some_and(|tool| matches!(tool.declaration, HumanTool::Command(_)))
    }

    pub fn preflight(&self, name: &str) -> Result<(), String> {
        let tool = self
            .tools
            .get(name)
            .ok_or("Unknown registered Human tool.")?;
        let owner = &tool.definition.artifact_id;
        self.scope
            .resolve_input(&self.config.root, owner, "")
            .map_err(|e| e.to_string())?;
        match tool.declaration {
            HumanTool::Command(declaration) => {
                super::preflight_executable(
                    &self.config.root,
                    &self.scope,
                    owner,
                    &declaration.command,
                )?;
                scope::resolve_human_argv(self.config, &self.scope, owner, &declaration.args)
                    .map_err(|error| error.to_string())?;
            }
            HumanTool::Builtin(declaration) => {
                scope::builtin_args(self.config, &self.scope, owner, declaration)
                    .map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    }

    /// Resolve the command line a call would run, without running it.
    pub fn command(&self, name: &str) -> Result<CommandLine, String> {
        let tool = self
            .tools
            .get(name)
            .ok_or("Unknown registered Human tool.")?;
        let owner = &tool.definition.artifact_id;
        let HumanTool::Command(declaration) = tool.declaration else {
            return Err("Builtin Human tools have no executable command line.".into());
        };
        for artifact in self.scope.artifacts.values() {
            scope::validate_file_target(&self.config.root, artifact)
                .map_err(|error| error.to_string())?;
        }
        let cwd = scope::scoped_path(&self.config.root, self.config.artifacts[owner].folder())
            .map_err(|e| e.to_string())?;
        let program = executable(&self.config.root, &self.scope, owner, &declaration.command)
            .map_err(|()| "Tool executable path is unavailable or outside scope.".to_owned())?;
        let args = scope::resolve_human_argv(self.config, &self.scope, owner, &declaration.args)
            .map_err(|e| e.to_string())?;
        Ok(CommandLine {
            repo: self.config.root.clone(),
            kind: declaration.kind,
            program,
            args,
            cwd,
        })
    }

    pub async fn call(&self, name: &str, cancellation: CancellationToken) -> ToolResult {
        let Some(tool) = self.tools.get(name) else {
            return ToolResult::error("Unknown registered Human tool.");
        };
        if cancellation.is_cancelled() {
            return ToolResult::error("Human tool call was cancelled.");
        }
        if let HumanTool::Builtin(declaration) = tool.declaration {
            return self
                .call_builtin(&tool.definition.artifact_id, declaration, cancellation)
                .await;
        }
        let HumanTool::Command(declaration) = tool.declaration else {
            unreachable!()
        };
        let Ok(CommandLine {
            program, args, cwd, ..
        }) = self.command(name)
        else {
            return ToolResult::error("Human tool preparation failed.");
        };
        if cancellation.is_cancelled() {
            return ToolResult::error("Human tool call was cancelled.");
        }
        match declaration.kind {
            HumanToolKind::Launch => match process::launch_detached(&program, &args, &cwd) {
                Ok(()) => ToolResult {
                    content: vec![Content::Launch { launched: true }],
                    is_error: false,
                },
                Err(error) => ToolResult::error(
                    error
                        .argument_refusal()
                        .unwrap_or_else(|| "Human tool could not be launched.".into()),
                ),
            },
            HumanToolKind::Output => {
                let command = process::Command {
                    program,
                    args: args.into_iter().map(Into::into).collect(),
                    cwd,
                    env: crate::platform::environment::snapshot(),
                    timeout: declaration.timeout_ms.unwrap_or(DEFAULT_TIMEOUT),
                };
                match process::run(command, cancellation, |_| async { Ok(()) }).await {
                    Ok(output) => output_result(&output),
                    Err(process::Error::Timeout) => ToolResult::error("Human tool timed out."),
                    Err(process::Error::Cancelled) => {
                        ToolResult::error("Human tool call was cancelled.")
                    }
                    Err(error) => ToolResult::error(
                        error
                            .argument_refusal()
                            .unwrap_or_else(|| "Human tool execution failed.".into()),
                    ),
                }
            }
        }
    }
    async fn call_builtin(
        &self,
        owner: &str,
        declaration: &crate::config::HumanBuiltinTool,
        cancellation: CancellationToken,
    ) -> ToolResult {
        for artifact in self.scope.artifacts.values() {
            if let Err(error) = scope::validate_file_target(&self.config.root, artifact) {
                return ToolResult::error(error.to_string());
            }
        }
        let (owner, args) = match scope::builtin_args(self.config, &self.scope, owner, declaration)
        {
            Ok(args) => args,
            Err(error) => return ToolResult::error(error.to_string()),
        };
        let owner = match scope::ArtifactId::new(owner) {
            Ok(owner) => owner,
            Err(error) => return ToolResult::error(error.to_string()),
        };
        let tool_scope = self.scope.tool_scope();
        if declaration.builtin == crate::config::Builtin::Open {
            let target = if artifactize_tools::builtin::is_url(&args[0]) {
                std::ffi::OsString::from(&args[0])
            } else {
                match tool_scope.resolve_input(&self.config.root, &owner, &args[0]) {
                    Ok(path) => path.into_os_string(),
                    Err(error) => return ToolResult::error(error.to_string()),
                }
            };
            return tokio::select! {
                _ = cancellation.cancelled() => ToolResult::error("Human tool call was cancelled."),
                result = artifactize_tools::opener::open(std::ffi::OsStr::new(&target)) => match result {
                    Ok(()) => ToolResult { content: vec![Content::Launch { launched: true }], is_error: false },
                    Err(error) => ToolResult::error(error.to_string()),
                }
            };
        }
        // A Human tool runs as the reviewer, with the reviewer's environment, as output
        // tools do.
        let launcher = process::Launcher {
            env: crate::platform::environment::snapshot(),
        };
        let context = artifactize_tools::builtin::Context {
            root: &self.config.root,
            scope: &tool_scope,
            owner: &owner,
            launcher: &launcher,
        };
        let result = artifactize_tools::builtin::call_fixed(
            declaration.builtin,
            &args,
            serde_json::json!({}),
            context,
            &cancellation,
        )
        .await;
        let output = result
            .content
            .into_iter()
            .map(|content| match content {
                artifactize_tools::Content::Text { text } => text,
                artifactize_tools::Content::Json { data } => {
                    serde_json::to_string_pretty(&data).expect("builtin JSON")
                }
                artifactize_tools::Content::Image { .. } => {
                    unreachable!("Human builtins never produce images")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        ToolResult {
            content: vec![Content::Text {
                text: text(output.as_bytes(), String::new(), false),
            }],
            is_error: result.is_error,
        }
    }
}

fn output_result(output: &process::Output) -> ToolResult {
    let failed = !output.status.success();
    let stdout = if failed {
        format!("Human tool exited unsuccessfully ({}).\n", output.status)
    } else {
        String::new()
    };
    let mut content = vec![Content::Text {
        text: text(&output.stdout, stdout, output.truncated),
    }];
    if !output.stderr.is_empty() {
        content.push(Content::Text {
            text: text(&output.stderr, "stderr:\n".into(), output.truncated),
        });
    }
    ToolResult {
        content,
        is_error: failed,
    }
}

fn text(bytes: &[u8], mut prefix: String, truncated: bool) -> String {
    prefix
        .push_str(&String::from_utf8(runtime::clean_output(bytes)).expect("clean output is UTF-8"));
    if prefix.len() > TEXT_LIMIT || truncated {
        let suffix = "\n[output truncated]";
        let mut end = (TEXT_LIMIT - suffix.len()).min(prefix.len());
        while !prefix.is_char_boundary(end) {
            end -= 1;
        }
        prefix.truncate(end);
        prefix.push_str(suffix);
    }
    prefix
}
