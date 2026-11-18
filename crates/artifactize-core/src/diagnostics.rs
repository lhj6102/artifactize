//! Backend readiness and tool checks.

mod doctor;
pub use doctor::{CheckStatus, DoctorReport, doctor};

use std::path::Path;

use serde::Serialize;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::{
    config::{Profile, RepoConfig},
    tools::{self, human},
    workspace,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Audience {
    Agent,
    Human,
}

use crate::{
    config::ToolName,
    types::{ArtifactName, EvalId},
};

/// Named arguments supplied by the owner, validated against the selected tool's schema.
#[derive(Debug, Clone)]
pub struct ToolArguments(serde_json::Map<String, serde_json::Value>);

impl std::str::FromStr for ToolArguments {
    type Err = String;
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match serde_json::from_str(text).map_err(|error| format!("Invalid --args JSON: {error}"))? {
            serde_json::Value::Object(object) => Ok(Self(object)),
            _ => Err("--args must be a JSON object.".into()),
        }
    }
}

#[derive(Debug)]
pub enum ToolCheckOptions {
    Inspect {
        scope: InspectionScope,
        tool: Option<ToolName>,
    },
    Execute {
        scope: ExecutionScope,
        tool: ToolName,
    },
}

#[derive(Debug)]
pub enum InspectionScope {
    Eval(EvalId),
    Catalog {
        artifact: Option<ArtifactName>,
        audience: Option<Audience>,
    },
}

#[derive(Debug)]
pub enum ExecutionScope {
    Eval {
        id: EvalId,
        args: Option<ToolArguments>,
    },
    Agent {
        artifact: ArtifactName,
        args: Option<ToolArguments>,
    },
    Human {
        artifact: ArtifactName,
    },
}

impl ToolCheckOptions {
    /// Parse CLI or MCP flag combinations once, before reading project configuration.
    pub fn from_flags(
        eval: Option<EvalId>,
        artifact: Option<ArtifactName>,
        audience: Option<Audience>,
        tool: Option<ToolName>,
        execute: bool,
        args: Option<ToolArguments>,
    ) -> Result<Self, String> {
        if eval.is_some() && (artifact.is_some() || audience.is_some()) {
            return Err("An eval selector determines scope and audience; do not combine it with --artifact or --audience.".into());
        }
        if execute
            && (tool.is_none() || (eval.is_none() && (artifact.is_none() || audience.is_none())))
        {
            return Err("--execute requires --tool, with an eval selector or with --artifact and --audience.".into());
        }
        if args.is_some() && (!execute || audience == Some(Audience::Human)) {
            return Err(AGENT_ARGS.into());
        }
        Ok(if execute {
            let tool = tool.expect("execution requires a tool");
            let scope = if let Some(id) = eval {
                ExecutionScope::Eval { id, args }
            } else {
                let artifact = artifact.expect("execution requires an eval or an Artifact");
                match audience.expect("Artifact execution requires an audience") {
                    Audience::Agent => ExecutionScope::Agent { artifact, args },
                    Audience::Human => ExecutionScope::Human { artifact },
                }
            };
            Self::Execute { scope, tool }
        } else {
            Self::Inspect {
                scope: eval.map_or(
                    InspectionScope::Catalog { artifact, audience },
                    InspectionScope::Eval,
                ),
                tool,
            }
        })
    }
}

const AGENT_ARGS: &str =
    "--args requires Agent tool execution; Human tools take no free arguments.";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCheckReport {
    pub ok: bool,
    pub scopes: Vec<ToolCheckScope>,
    pub checks: Vec<ToolCheck>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<ToolCheckResult>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCheckScope {
    pub eval_id: Option<crate::config::EvalId>,
    pub artifact_id: Option<crate::config::ArtifactName>,
    pub audience: Audience,
    pub tools: Vec<tools::ToolDefinition>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolCheckStage {
    Declarations,
    Preflight,
    Execute,
}

#[derive(Debug)]
pub struct ToolCheck {
    pub stage: ToolCheckStage,
    pub tool: Option<crate::config::ToolName>,
    pub result: Result<(), String>,
}

impl Serialize for ToolCheck {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("ToolCheck", 4)?;
        state.serialize_field("stage", &self.stage)?;
        state.serialize_field("tool", &self.tool)?;
        state.serialize_field("ok", &self.result.is_ok())?;
        state.serialize_field("message", &self.result.as_ref().err())?;
        state.end()
    }
}
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum ToolCheckResult {
    Agent(tools::ToolResult),
    Human(human::ToolResult),
}

impl ToolCheckReport {
    fn check(
        &mut self,
        stage: ToolCheckStage,
        name: Option<&crate::config::ToolName>,
        result: Result<(), String>,
    ) {
        self.ok &= result.is_ok();
        self.checks.push(ToolCheck {
            stage,
            tool: name.cloned(),
            result,
        });
    }
}

enum Catalog<'a> {
    Agent(tools::Registry<'a>),
    Human(human::Registry<'a>),
}

impl Catalog<'_> {
    fn definitions(&self) -> Vec<tools::ToolDefinition> {
        match self {
            Self::Agent(registry) => registry.list().cloned().collect(),
            Self::Human(registry) => registry
                .list()
                .map(|tool| tools::ToolDefinition {
                    name: tool.name.clone(),
                    artifact_id: tool.artifact_id.clone(),
                    description: tool.description.clone(),
                    input_schema: json!({
                        "type":"object",
                        "properties":{},
                        "additionalProperties":false,
                    }),
                })
                .collect(),
        }
    }
    fn preflight(&self, name: &crate::config::ToolName) -> Result<(), String> {
        match self {
            Self::Agent(r) => r.preflight(name),
            Self::Human(r) => r.preflight(name),
        }
    }
}

pub async fn check_tools(
    repo: &Path,
    state: Option<&Path>,
    options: &ToolCheckOptions,
    cancellation: CancellationToken,
) -> Result<ToolCheckReport, String> {
    let mut report = ToolCheckReport {
        ok: true,
        scopes: Vec::new(),
        checks: Vec::new(),
        result: None,
    };
    let config = match crate::config::read_workspace_config(repo) {
        Ok(config) => config,
        Err(error) => {
            report.check(ToolCheckStage::Declarations, None, Err(error.to_string()));
            return Ok(report);
        }
    };
    let (selected, artifact, audience, tool, executing) = match options {
        ToolCheckOptions::Inspect { scope, tool } => match scope {
            InspectionScope::Eval(id) => (Some(id), None, None, tool.as_ref(), None),
            InspectionScope::Catalog { artifact, audience } => {
                (None, artifact.as_ref(), *audience, tool.as_ref(), None)
            }
        },
        ToolCheckOptions::Execute { scope, tool } => match scope {
            ExecutionScope::Eval { id, args } => {
                (Some(id), None, None, Some(tool), Some(args.as_ref()))
            }
            ExecutionScope::Agent { artifact, args } => (
                None,
                Some(artifact),
                Some(Audience::Agent),
                Some(tool),
                Some(args.as_ref()),
            ),
            ExecutionScope::Human { artifact } => (
                None,
                Some(artifact),
                Some(Audience::Human),
                Some(tool),
                Some(None),
            ),
        },
    };
    if let Some(id) = selected
        && !config.evals.iter().any(|eval| {
            &eval.id == id
                && matches!(
                    eval.declaration.profile(),
                    Profile::Agent { .. } | Profile::Human {}
                )
        })
    {
        return Err("Select a declared Agent or Human eval.".into());
    }
    if executing.flatten().is_some()
        && let Some(id) = selected
        && config
            .evals
            .iter()
            .any(|eval| &eval.id == id && matches!(eval.declaration.profile(), Profile::Human {}))
    {
        return Err(AGENT_ARGS.into());
    }
    if let Some(id) = artifact
        && !config.artifacts.contains_key(id)
    {
        return Err(format!("Unknown Artifact: {id}"));
    }
    let mut catalogs = Vec::new();
    // An eval selector keeps the eval's own scope, also for one selected tool.
    if artifact.is_some() || audience.is_some() || (tool.is_some() && selected.is_none()) {
        let artifacts: Vec<_> =
            artifact.map_or_else(|| config.artifacts.keys().collect(), |id| vec![id]);
        for artifact in artifacts {
            for kind in [Audience::Agent, Audience::Human] {
                if audience.is_some_and(|a| a != kind) {
                    continue;
                }
                let catalog = match kind {
                    Audience::Agent => {
                        tools::Registry::for_artifact(&config, artifact).map(Catalog::Agent)
                    }
                    Audience::Human => {
                        human::Registry::for_artifact(&config, artifact).map(Catalog::Human)
                    }
                };
                catalogs.push((None, Some(artifact), kind, catalog));
            }
        }
    } else {
        for eval in &config.evals {
            if selected.is_some_and(|id| id != &eval.id) {
                continue;
            }
            let (audience, catalog) = match eval.declaration.profile() {
                Profile::Agent { .. } => (
                    Audience::Agent,
                    tools::Registry::new(&config, &eval.id).map(Catalog::Agent),
                ),
                Profile::Human { .. } => (
                    Audience::Human,
                    human::Registry::new(&config, &eval.id).map(Catalog::Human),
                ),
                Profile::Runtime { .. } => continue,
            };
            catalogs.push((Some(&eval.id), None, audience, catalog));
        }
    }
    for (eval, artifact, audience, catalog) in catalogs {
        let catalog = match catalog {
            Ok(catalog) => catalog,
            Err(error) => {
                report.check(ToolCheckStage::Preflight, None, Err(error));
                continue;
            }
        };
        let mut definitions = catalog.definitions();
        if let Some(name) = tool {
            let published = definitions.iter().any(|d| d.name == *name);
            definitions.retain(|d| {
                artifact.is_none_or(|id| &d.artifact_id == id)
                    && (d.name == *name
                        || (!published
                            && artifact.is_some()
                            && d.name == format!("{name}_{}", d.artifact_id)))
            });
            if definitions.is_empty() {
                report.check(
                    ToolCheckStage::Preflight,
                    Some(name),
                    Err(if eval.is_some() {
                        "The selected tool is not published to this eval.".into()
                    } else {
                        "The selected tool is not registered for this Artifact and audience.".into()
                    }),
                );
            }
        }
        for definition in &definitions {
            report.check(
                ToolCheckStage::Preflight,
                Some(&definition.name),
                catalog.preflight(&definition.name),
            );
        }
        report.scopes.push(ToolCheckScope {
            eval_id: eval.cloned(),
            artifact_id: artifact.map(|id| config.artifacts[id].name.clone()),
            audience,
            tools: definitions.clone(),
        });
        if let Some(args) = executing
            && report.ok
            && definitions.len() == 1
        {
            let name = &definitions[0].name;
            let result =
                execute_tool(&config, state, &catalog, name, args, cancellation.clone()).await;
            match result {
                Ok(result) => {
                    report.check(
                        ToolCheckStage::Execute,
                        Some(name),
                        if result.is_error() {
                            Err("The tool returned an error; no review result was created.".into())
                        } else {
                            Ok(())
                        },
                    );
                    report.result = Some(result);
                }
                Err(error) => report.check(ToolCheckStage::Execute, Some(name), Err(error)),
            }
        }
    }
    Ok(report)
}

async fn execute_tool(
    config: &RepoConfig,
    state: Option<&Path>,
    catalog: &Catalog<'_>,
    name: &crate::config::ToolName,
    args: Option<&ToolArguments>,
    cancellation: CancellationToken,
) -> Result<ToolCheckResult, String> {
    match catalog {
        Catalog::Human(registry) => Ok(ToolCheckResult::Human(
            registry.call(name, cancellation).await,
        )),
        Catalog::Agent(registry) => {
            let state = crate::store::state_dir(state)?;
            let root =
                workspace::prepare_directory(&state, &config.root).map_err(|e| e.to_string())?;
            let directory = tempfile::Builder::new()
                .prefix("tools-check-")
                .tempdir_in(root)
                .map_err(|e| e.to_string())?;
            let args =
                serde_json::Value::Object(args.map(|args| args.0.clone()).unwrap_or_default());
            let sessions = registry.sessions(&state, Some(name), cancellation.clone());
            let registry = registry.with_session_scope(&sessions)?;
            let result = registry
                .call(name, args, directory.path(), cancellation)
                .await;
            sessions.close().await?;
            directory
                .close()
                .map_err(|_| "Tool-check cleanup failed.".to_owned())?;
            Ok(ToolCheckResult::Agent(result))
        }
    }
}

impl ToolCheckResult {
    fn is_error(&self) -> bool {
        match self {
            Self::Agent(result) => result.is_error(),
            Self::Human(result) => result.is_error(),
        }
    }
}
