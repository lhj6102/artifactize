//! Predefined reviewer commands, deliberately outside Agent runtime isolation.

use std::{collections::BTreeMap, time::Duration};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::{
    config::{HumanTool, HumanToolKind, Profile, RepoConfig},
    process, runtime,
    scope::{self, Scope},
};

use super::{DEFAULT_TIMEOUT_MS, executable};

const TEXT_LIMIT: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDefinition {
    pub name: String,
    pub artifact_id: String,
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

struct RegisteredTool<'a> {
    definition: ToolDefinition,
    declaration: &'a HumanTool,
}

/// Human catalog only. Claimant authorization belongs to the request lifecycle.
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
        if !matches!(eval.declaration.profile, Profile::Human {}) {
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
                let name = format!("{operation}_{id}");
                let entry = RegisteredTool {
                    definition: ToolDefinition {
                        name: name.clone(),
                        artifact_id: (*id).into(),
                        description: declaration.description.replace("{artifactName}", id),
                        kind: declaration.kind,
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

    pub fn preflight(&self, name: &str) -> Result<(), String> {
        let tool = self
            .tools
            .get(name)
            .ok_or("Unknown registered Human tool.")?;
        let owner = &tool.definition.artifact_id;
        self.scope
            .resolve_input(&self.config.root, owner, "")
            .map_err(|e| e.to_string())?;
        super::preflight_executable(
            &self.config.root,
            &self.scope,
            owner,
            &tool.declaration.command,
        )?;
        scope::resolve_human_argv(self.config, &self.scope, owner, &tool.declaration.args)
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn call(&self, name: &str, cancellation: CancellationToken) -> ToolResult {
        let Some(tool) = self.tools.get(name) else {
            return ToolResult::error("Unknown registered Human tool.");
        };
        if cancellation.is_cancelled() {
            return ToolResult::error("Human tool call was cancelled.");
        }
        let owner = &tool.definition.artifact_id;
        let declaration = tool.declaration;
        let prepare = || {
            let cwd = self
                .scope
                .resolve_input(&self.config.root, owner, "")
                .map_err(|_| ())?;
            let program = executable(&self.config.root, &self.scope, owner, &declaration.command)?;
            let args =
                scope::resolve_human_argv(self.config, &self.scope, owner, &declaration.args)
                    .map_err(|_| ())?;
            Ok::<_, ()>((cwd, program, args))
        };
        let Ok((cwd, program, args)) = prepare() else {
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
                Err(_) => ToolResult::error("Human tool could not be launched."),
            },
            HumanToolKind::Output => {
                let command = process::Command {
                    program,
                    args: args.into_iter().map(Into::into).collect(),
                    cwd,
                    env: std::env::vars_os().collect(),
                    timeout: Duration::from_millis(u64::from(
                        declaration.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS),
                    )),
                };
                match process::run(command, cancellation, |_| async { Ok(()) }).await {
                    Ok(output) => output_result(&output),
                    Err(process::Error::Timeout) => ToolResult::error("Human tool timed out."),
                    Err(process::Error::Cancelled) => {
                        ToolResult::error("Human tool call was cancelled.")
                    }
                    Err(_) => ToolResult::error("Human tool execution failed."),
                }
            }
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
