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

#[derive(Debug, Default, clap::Args)]
pub struct ToolCheckOptions {
    /// Select one Agent or Human eval's admitted scope (same as --eval).
    #[arg(conflicts_with = "eval")]
    pub selector: Option<crate::types::EvalId>,
    /// Select one Agent or Human eval's admitted scope.
    #[arg(long, value_name = "ID")]
    pub eval: Option<crate::types::EvalId>,
    /// Check tools declared for one Artifact.
    #[arg(long, value_name = "ID")]
    pub artifact: Option<crate::types::ArtifactName>,
    /// Restrict --artifact to Agent or Human tools.
    #[arg(long, value_enum)]
    pub audience: Option<Audience>,
    /// Check one tool by short or published name.
    #[arg(long, value_name = "NAME")]
    pub tool: Option<crate::config::ToolName>,
    /// Invoke exactly one explicitly selected tool; never create review evidence.
    #[arg(long)]
    pub execute: bool,
    /// JSON arguments for an executed Agent tool.
    #[arg(long, value_name = "JSON", value_parser = json_arguments)]
    pub args: Option<serde_json::Value>,
}

impl ToolCheckOptions {
    pub fn validate(&self) -> Result<(), String> {
        if (self.eval.is_some() || self.selector.is_some())
            && (self.artifact.is_some()
                || self.audience.is_some()
                || self.tool.is_some()
                || self.execute)
        {
            return Err(
                "An eval selector determines scope and audience; do not combine it with --artifact, --audience, --tool or --execute."
                    .into(),
            );
        }
        if self.execute
            && (self.artifact.is_none() || self.audience.is_none() || self.tool.is_none())
        {
            return Err("--execute requires --artifact, --audience and --tool.".into());
        }
        if self.args.is_some() && (!self.execute || self.audience == Some(Audience::Human)) {
            return Err(
                "--args requires Agent tool execution; Human tools take no free arguments.".into(),
            );
        }
        Ok(())
    }
}

/// `--args`, parsed once where it is given.
fn json_arguments(text: &str) -> Result<serde_json::Value, String> {
    serde_json::from_str(text).map_err(|e| format!("Invalid --args JSON: {e}"))
}

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
#[derive(Debug, Serialize)]
pub struct ToolCheck {
    pub stage: String,
    pub tool: Option<String>,
    pub ok: bool,
    pub message: Option<String>,
}
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum ToolCheckResult {
    Agent(tools::ToolResult),
    Human(human::ToolResult),
}

impl ToolCheckReport {
    fn check(&mut self, stage: &str, name: Option<&str>, result: Result<(), String>) {
        self.ok &= result.is_ok();
        self.checks.push(ToolCheck {
            stage: stage.into(),
            tool: name.map(str::to_owned),
            ok: result.is_ok(),
            message: result.err(),
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
    fn preflight(&self, name: &str) -> Result<(), String> {
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
    options.validate()?;
    let mut report = ToolCheckReport {
        ok: true,
        scopes: Vec::new(),
        checks: Vec::new(),
        result: None,
    };
    let config = match crate::config::read_workspace_config(repo) {
        Ok(config) => config,
        Err(error) => {
            report.check("declarations", None, Err(error.to_string()));
            return Ok(report);
        }
    };
    let selected = options.eval.as_ref().or(options.selector.as_ref());
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
    if let Some(id) = &options.artifact
        && !config.artifacts.contains_key(id)
    {
        return Err(format!("Unknown Artifact: {id}"));
    }
    let mut catalogs = Vec::new();
    if options.artifact.is_some() || options.audience.is_some() || options.tool.is_some() {
        let artifacts: Vec<_> = options.artifact.as_ref().map_or_else(
            || config.artifacts.keys().map(|name| name.as_str()).collect(),
            |id| vec![id.as_str()],
        );
        for artifact in artifacts {
            for audience in [Audience::Agent, Audience::Human] {
                if options.audience.is_some_and(|a| a != audience) {
                    continue;
                }
                let catalog = match audience {
                    Audience::Agent => {
                        tools::Registry::for_artifact(&config, artifact).map(Catalog::Agent)
                    }
                    Audience::Human => {
                        human::Registry::for_artifact(&config, artifact).map(Catalog::Human)
                    }
                };
                catalogs.push((None, Some(artifact), audience, catalog));
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
                Profile::Runtime { .. } | Profile::Dependency { .. } => continue,
            };
            catalogs.push((Some(eval.id.as_str()), None, audience, catalog));
        }
    }
    for (eval, artifact, audience, catalog) in catalogs {
        let catalog = match catalog {
            Ok(catalog) => catalog,
            Err(error) => {
                report.check("preflight", None, Err(error));
                continue;
            }
        };
        let mut definitions = catalog.definitions();
        if let Some(name) = &options.tool {
            let published = definitions.iter().any(|d| d.name == *name);
            definitions.retain(|d| {
                artifact.is_none_or(|id| d.artifact_id == id)
                    && (d.name == *name
                        || (!published
                            && artifact.is_some_and(|id| d.name == format!("{name}_{id}"))))
            });
            if definitions.is_empty() {
                report.check(
                    "preflight",
                    Some(name),
                    Err(
                        "The selected tool is not registered for this Artifact and audience."
                            .into(),
                    ),
                );
            }
        }
        for definition in &definitions {
            report.check(
                "preflight",
                Some(&definition.name),
                catalog.preflight(&definition.name),
            );
        }
        report.scopes.push(ToolCheckScope {
            eval_id: eval.map(|id| id.parse().expect("configured Eval id")),
            artifact_id: artifact.map(|id| config.artifacts[id].name.clone()),
            audience,
            tools: definitions.clone(),
        });
        if options.execute && report.ok && definitions.len() == 1 {
            let name = &definitions[0].name;
            let result = execute_tool(
                &config,
                state,
                &catalog,
                name,
                options,
                cancellation.clone(),
            )
            .await;
            match result {
                Ok(result) => {
                    report.check(
                        "execute",
                        Some(name),
                        if result.is_error() {
                            Err("The tool returned an error; no review result was created.".into())
                        } else {
                            Ok(())
                        },
                    );
                    report.result = Some(result);
                }
                Err(error) => report.check("execute", Some(name), Err(error)),
            }
        }
    }
    Ok(report)
}

async fn execute_tool(
    config: &RepoConfig,
    state: Option<&Path>,
    catalog: &Catalog<'_>,
    name: &str,
    options: &ToolCheckOptions,
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
            let args = options.args.clone().unwrap_or(json!({}));
            let result = registry
                .call(name, args, directory.path(), cancellation)
                .await;
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
            Self::Agent(result) => result.is_error,
            Self::Human(result) => result.is_error,
        }
    }
}
