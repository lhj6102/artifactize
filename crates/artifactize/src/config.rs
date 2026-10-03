//! Inert discovery, strict declarations, defaults, and family expansion.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Map, Value};
use thiserror::Error;

pub mod families;
mod validation;

use validation::{identifier, paths, positive_integer, present, script, text, timeout};

/// Format version for artifactize configuration documents.
pub const CONFIG_FORMAT_VERSION: u32 = 1;
pub const CONFIG_FILE: &str = "artifactize.json";

#[derive(Debug, Error)]
#[error("{path}: {message}")]
pub struct ConfigError {
    pub path: PathBuf,
    pub message: String,
}

impl ConfigError {
    fn new(path: impl Into<PathBuf>, message: impl ToString) -> Self {
        Self {
            path: path.into(),
            message: message.to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Script {
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum Profile {
    Agent {
        provider: String,
        model: String,
        reasoning: String,
        #[serde(rename = "timeoutMs", default, deserialize_with = "timeout")]
        timeout_ms: Option<u32>,
        #[serde(
            rename = "maxToolCalls",
            default,
            deserialize_with = "positive_integer"
        )]
        max_tool_calls: Option<u64>,
        #[serde(rename = "maxTokens", default, deserialize_with = "positive_integer")]
        max_tokens: Option<u64>,
    },
    Human {},
    Runtime {
        command: String,
        args: Vec<String>,
        #[serde(rename = "timeoutMs", default, deserialize_with = "timeout")]
        timeout_ms: Option<u32>,
    },
}

impl Profile {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Agent {
                provider,
                model,
                reasoning,
                ..
            } => {
                text(provider, "Agent profile provider")?;
                text(model, "Agent profile model")?;
                text(reasoning, "Agent profile reasoning")
            }
            Self::Human {} => Ok(()),
            Self::Runtime { command, args, .. } => script(command, args),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResultCheck {
    pub script: Script,
    #[serde(default, deserialize_with = "timeout")]
    pub timeout_ms: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CriticDeclaration {
    pub id: String,
    pub title: String,
    pub profile: Profile,
    #[serde(default)]
    pub profile_variants: BTreeMap<String, Profile>,
    pub payload: Map<String, Value>,
    /// Schema semantics are validated by the result executor, not discovery.
    #[serde(default, deserialize_with = "present")]
    pub pass_schema: Option<Map<String, Value>>,
    #[serde(default, deserialize_with = "present")]
    pub fail_schema: Option<Map<String, Value>>,
    #[serde(default, deserialize_with = "present")]
    pub result_check: Option<ResultCheck>,
}

impl CriticDeclaration {
    fn validate(&self) -> Result<(), String> {
        identifier(&self.id, "Critic id")?;
        text(&self.title, "Critic title")?;
        let instruction = self.payload.get("instruction").and_then(Value::as_str);
        text(
            instruction.unwrap_or_default(),
            "Critic payload.instruction",
        )?;
        self.profile.validate()?;
        if self.profile_variants.len() > 64 {
            return Err("profileVariants must contain at most 64 named profiles.".into());
        }
        for (name, profile) in &self.profile_variants {
            identifier(name, "Profile variant name")?;
            profile.validate()?;
            if std::mem::discriminant(profile) != std::mem::discriminant(&self.profile) {
                return Err("Profile variants must retain the declared reviewer kind.".into());
            }
        }
        if let Some(check) = &self.result_check {
            if !matches!(self.profile, Profile::Agent { .. }) {
                return Err("resultCheck requires an Agent Critic.".into());
            }
            script(&check.script.command, &check.script.args)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolMetadata {
    pub description: String,
    /// Kept as owner-authored JSON; tool schema validation belongs to the tool host.
    pub input_schema: Map<String, Value>,
    pub result_kinds: Vec<ResultKind>,
    pub observation: Observation,
    #[serde(default, deserialize_with = "present")]
    pub artifact_kind: Option<ArtifactKind>,
    #[serde(default, deserialize_with = "timeout")]
    pub timeout_ms: Option<u32>,
    #[serde(default)]
    pub execution_paths: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResultKind {
    Text,
    Json,
    Image,
    Launch,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Observation {
    Content,
    None,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactKind {
    Directory,
    Any,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct View {
    pub metadata: ToolMetadata,
    pub script: Script,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Views {
    #[serde(default)]
    pub agent_tools: BTreeMap<String, View>,
    #[serde(default)]
    pub human_tools: BTreeMap<String, View>,
}

impl Views {
    fn validate(&self) -> Result<(), String> {
        for (name, view) in self.agent_tools.iter().chain(&self.human_tools) {
            identifier(name, "View name")?;
            script(&view.script.command, &view.script.args)?;
            let metadata = &view.metadata;
            text(&metadata.description, "Tool description")?;
            if metadata.description.encode_utf16().count() > 4000
                || metadata
                    .description
                    .replace("{artifactName}", "")
                    .contains(['{', '}'])
            {
                return Err("Tool description is limited to 4000 characters and supports only {artifactName}.".into());
            }
            if metadata.result_kinds.is_empty()
                || metadata.result_kinds.iter().collect::<BTreeSet<_>>().len()
                    != metadata.result_kinds.len()
            {
                return Err("Tool resultKinds must be nonempty and unique.".into());
            }
            paths(&metadata.execution_paths, "metadata.executionPaths")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Stale {
    Always {},
    FileHash {
        #[serde(default, deserialize_with = "present")]
        paths: Option<Vec<String>>,
    },
    Identity {
        script: Script,
        #[serde(default)]
        inputs: Vec<String>,
        #[serde(rename = "timeoutMs", default, deserialize_with = "timeout")]
        timeout_ms: Option<u32>,
        #[serde(default, deserialize_with = "positive_integer")]
        weight: Option<u64>,
    },
}

impl Stale {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Always {} => Ok(()),
            Self::FileHash { paths: Some(items) } => {
                if items.is_empty() {
                    return Err("stale.paths must be a nonempty array.".into());
                }
                for item in items {
                    validation::path(item)?;
                }
                Ok(())
            }
            Self::FileHash { paths: None } => Ok(()),
            Self::Identity {
                script: definition,
                inputs,
                weight,
                ..
            } => {
                script(&definition.command, &definition.args)?;
                let entry = if definition.command == "node" {
                    definition
                        .args
                        .first()
                        .map(String::as_str)
                        .unwrap_or_default()
                } else {
                    &definition.command
                };
                validation::path(entry)?;
                if entry.starts_with('-') {
                    return Err(
                        "Identity scripts require an owner-relative entry, not command flags."
                            .into(),
                    );
                }
                paths(inputs, "stale.inputs")?;
                if weight.is_some_and(|weight| weight > 100) {
                    return Err("Identity weight must be 1–100.".into());
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnvironmentRequirement {
    pub description: String,
    pub script: Script,
    #[serde(default, deserialize_with = "timeout")]
    pub timeout_ms: Option<u32>,
    #[serde(default)]
    pub inputs: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DependencyGates {
    Green,
    Ignore,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewPolicy {
    #[serde(default, deserialize_with = "present")]
    pub dependency_gates: Option<DependencyGates>,
    #[serde(default, deserialize_with = "positive_integer")]
    pub max_concurrent_executors: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactDeclaration {
    pub name: String,
    #[serde(default)]
    pub critics: Vec<CriticDeclaration>,
    #[serde(default)]
    pub views: Views,
    #[serde(default)]
    pub mounts: BTreeMap<String, String>,
    #[serde(default, deserialize_with = "present")]
    pub basis: Option<bool>,
    #[serde(default, deserialize_with = "present")]
    pub stale: Option<Stale>,
    #[serde(default)]
    pub env_requirements: BTreeMap<String, EnvironmentRequirement>,
    #[serde(default, deserialize_with = "present")]
    pub review_policy: Option<ReviewPolicy>,
}

impl ArtifactDeclaration {
    fn validate(&self) -> Result<(), String> {
        identifier(&self.name, "Artifact name")?;
        let mut ids = BTreeSet::new();
        for critic in &self.critics {
            critic
                .validate()
                .map_err(|message| format!("Critic {}: {message}", critic.id))?;
            if !ids.insert(&critic.id) {
                return Err(format!(
                    "Duplicate local Critic in {}: {}",
                    self.name, critic.id
                ));
            }
        }
        if self.basis == Some(true) && !self.critics.is_empty() {
            return Err(format!("Basis Artifact {} cannot own Critics.", self.name));
        }
        self.views.validate()?;
        for (alias, target) in &self.mounts {
            identifier(alias, "Mount alias")?;
            identifier(target, "Mount target")?;
        }
        if let Some(stale) = &self.stale {
            stale.validate()?;
        }
        if self.env_requirements.len() > 32 {
            return Err("envRequirements must contain at most 32 checks.".into());
        }
        for (name, requirement) in &self.env_requirements {
            identifier(name, "Environment check name")?;
            text(&requirement.description, "Environment check description")?;
            if requirement.description.encode_utf16().count() > 2000 {
                return Err(
                    "Environment check descriptions are limited to 2000 characters.".into(),
                );
            }
            script(&requirement.script.command, &requirement.script.args)?;
            paths(&requirement.inputs, "Environment check inputs")?;
        }
        Ok(())
    }
}

/// Validate declaration data only. No script or declared input is opened here.
pub fn parse_declaration(json: &str) -> Result<ArtifactDeclaration, String> {
    let value: Value = serde_json::from_str(json).map_err(|error| error.to_string())?;
    if value
        .as_object()
        .is_some_and(|object| object.contains_key("family"))
    {
        return Err("Artifact families are not supported yet (P2.1).".into());
    }
    let declaration: ArtifactDeclaration =
        serde_json::from_value(value).map_err(|error| error.to_string())?;
    declaration.validate()?;
    Ok(declaration)
}

#[derive(Debug)]
pub struct Artifact {
    pub path: PathBuf,
    pub name: String,
    pub views: Views,
    pub mounts: BTreeMap<String, String>,
    pub basis: Option<bool>,
    pub stale: Option<Stale>,
    pub env_requirements: BTreeMap<String, EnvironmentRequirement>,
    pub review_policy: Option<ReviewPolicy>,
}

#[derive(Debug)]
pub struct Critic {
    /// Workspace-qualified identity; declaration.id remains the owner's local id.
    pub id: String,
    pub target: String,
    pub declaration: CriticDeclaration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewRequirement {
    Basis,
    Unreviewed,
    Critics,
}

#[derive(Debug)]
pub struct RepoConfig {
    pub root: PathBuf,
    pub artifacts: BTreeMap<String, Artifact>,
    pub critics: Vec<Critic>,
}

impl RepoConfig {
    /// Local obligations only; a basis does not satisfy its dependency scope.
    pub fn review_requirement(&self, artifact_id: &str) -> Option<ReviewRequirement> {
        let artifact = self.artifacts.get(artifact_id)?;
        Some(if artifact.basis == Some(true) {
            ReviewRequirement::Basis
        } else if self
            .critics
            .iter()
            .any(|critic| critic.target == artifact_id)
        {
            ReviewRequirement::Critics
        } else {
            ReviewRequirement::Unreviewed
        })
    }
}

/// Discover regular markers in deterministic path order, without following directory links.
pub fn read_workspace_config(repo: &Path) -> Result<RepoConfig, ConfigError> {
    let root = fs::canonicalize(repo).map_err(|error| ConfigError::new(repo, error))?;
    let mut config = RepoConfig {
        root,
        artifacts: BTreeMap::new(),
        critics: Vec::new(),
    };
    let mut pending = vec![PathBuf::new()];
    while let Some(relative) = pending.pop() {
        let directory = config.root.join(&relative);
        let entries = fs::read_dir(&directory)
            .and_then(|entries| entries.collect::<Result<Vec<_>, _>>())
            .map_err(|error| ConfigError::new(&directory, error))?;
        let mut entries = entries;
        entries.sort_by_key(|entry| entry.file_name());
        if let Some(marker) = entries
            .iter()
            .find(|entry| entry.file_name() == CONFIG_FILE)
        {
            let file = marker.path();
            let kind = marker
                .file_type()
                .map_err(|error| ConfigError::new(&file, error))?;
            if !kind.is_file() {
                return Err(ConfigError::new(
                    file,
                    "artifactize.json must be a regular file.",
                ));
            }
            let json = fs::read_to_string(&file).map_err(|error| ConfigError::new(&file, error))?;
            let declaration =
                parse_declaration(&json).map_err(|error| ConfigError::new(&file, error))?;
            if !relative.as_os_str().is_empty() && declaration.review_policy.is_some() {
                return Err(ConfigError::new(
                    file,
                    "reviewPolicy belongs only to the repository root artifactize.json.",
                ));
            }
            let ArtifactDeclaration {
                name,
                critics,
                views,
                mounts,
                basis,
                stale,
                env_requirements,
                review_policy,
            } = declaration;
            if let Some(previous) = config.artifacts.get(&name) {
                return Err(ConfigError::new(
                    file,
                    format!(
                        "Duplicate Artifact name: {name} (also declared in {})",
                        previous.path.join(CONFIG_FILE).display()
                    ),
                ));
            }
            for declaration in critics {
                config.critics.push(Critic {
                    id: format!("{name}/{}", declaration.id),
                    target: name.clone(),
                    declaration,
                });
            }
            config.artifacts.insert(
                name.clone(),
                Artifact {
                    path: relative.clone(),
                    name,
                    views,
                    mounts,
                    basis,
                    stale,
                    env_requirements,
                    review_policy,
                },
            );
        }
        for entry in entries.into_iter().rev() {
            let kind = entry
                .file_type()
                .map_err(|error| ConfigError::new(entry.path(), error))?;
            if kind.is_dir() && entry.file_name() != ".git" && entry.file_name() != "node_modules" {
                pending.push(relative.join(entry.file_name()));
            }
        }
    }
    if config.artifacts.is_empty() {
        return Err(ConfigError::new(
            &config.root,
            "Workspace must contain at least one artifactize.json Artifact.",
        ));
    }
    Ok(config)
}

#[cfg(test)]
mod tests;
