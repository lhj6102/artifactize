//! Inert discovery, strict declarations, and family expansion.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

pub mod families;
mod tools;
mod validation;

pub use tools::{AgentTool, Builtin, BuiltinTool, CommandTool, ToolProtocol};

pub(crate) use validation::identifier;
use validation::{paths, positive_integer, present, script, text, timeout};

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
    pub(crate) fn new(path: impl Into<PathBuf>, message: impl ToString) -> Self {
        Self {
            path: path.into(),
            message: message.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Script {
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Openai,
    Anthropic,
    Chatgpt,
    Claude,
}

impl Backend {
    pub fn validate_reasoning(self, reasoning: &str) -> Result<(), String> {
        let supported = match self {
            Self::Openai | Self::Chatgpt => matches!(
                reasoning,
                "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
            ),
            Self::Anthropic | Self::Claude => {
                matches!(reasoning, "low" | "medium" | "high" | "max")
            }
        };
        if supported {
            Ok(())
        } else {
            Err(format!(
                "Unsupported reasoning {reasoning:?} for {self:?}; effort is never remapped."
            ))
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum Profile {
    Agent {
        backend: Backend,
        model: String,
        #[serde(default, deserialize_with = "present")]
        reasoning: Option<String>,
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
                backend,
                model,
                reasoning,
                ..
            } => {
                text(model, "Agent profile model")?;
                if let Some(reasoning) = reasoning {
                    backend.validate_reasoning(reasoning)?;
                }
                Ok(())
            }
            Self::Human {} => Ok(()),
            Self::Runtime { command, args, .. } => script(command, args),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvalDeclaration {
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
}

impl EvalDeclaration {
    fn validate(&self) -> Result<(), String> {
        identifier(&self.id, "Eval id")?;
        text(&self.title, "Eval title")?;
        let instruction = self.payload.get("instruction").and_then(Value::as_str);
        text(instruction.unwrap_or_default(), "Eval payload.instruction")?;
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
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolMetadata {
    pub description: String,
    /// Kept as owner-authored JSON; tool schema validation belongs to the tool host.
    pub input_schema: Map<String, Value>,
    pub result_kinds: Vec<ResultKind>,
    #[serde(default, deserialize_with = "present")]
    pub artifact_kind: Option<ArtifactKind>,
    #[serde(default, deserialize_with = "timeout")]
    pub timeout_ms: Option<u32>,
    #[serde(default)]
    pub execution_paths: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResultKind {
    Text,
    Json,
    Image,
    Launch,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactKind {
    Directory,
    Any,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct View {
    pub metadata: ToolMetadata,
    pub script: Script,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Views {
    #[serde(default)]
    pub agent_tools: BTreeMap<String, AgentTool>,
    #[serde(default)]
    pub human_tools: BTreeMap<String, View>,
}

impl Views {
    fn validate(&self) -> Result<(), String> {
        for (name, tool) in &self.agent_tools {
            identifier(name, "Agent tool name")?;
            tool.validate().map_err(|e| format!("Tool {name}: {e}"))?;
        }
        for (name, view) in &self.human_tools {
            identifier(name, "View name")?;
            script(&view.script.command, &view.script.args)?;
            let metadata = &view.metadata;
            tools::description(&metadata.description)?;
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Stale {
    Identity {
        script: Script,
        #[serde(default)]
        inputs: Vec<String>,
        #[serde(rename = "timeoutMs", default, deserialize_with = "timeout")]
        timeout_ms: Option<u32>,
    },
}

impl Stale {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Identity {
                script: definition,
                inputs,
                ..
            } => {
                script(&definition.command, &definition.args)?;
                paths(inputs, "stale.inputs")
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DependencyGates {
    Green,
    Ignore,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewPolicy {
    #[serde(default, deserialize_with = "present")]
    pub dependency_gates: Option<DependencyGates>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactDeclaration {
    pub name: String,
    #[serde(default)]
    pub evals: Vec<EvalDeclaration>,
    #[serde(default)]
    pub views: Views,
    #[serde(default)]
    pub mounts: BTreeMap<String, String>,
    #[serde(default, deserialize_with = "present")]
    pub basis: Option<bool>,
    #[serde(default, deserialize_with = "present")]
    pub stale: Option<Stale>,
    #[serde(default, deserialize_with = "present")]
    pub review_policy: Option<ReviewPolicy>,
}

impl ArtifactDeclaration {
    fn validate(&self) -> Result<(), String> {
        identifier(&self.name, "Artifact name")?;
        let mut ids = BTreeSet::new();
        for eval in &self.evals {
            eval.validate()
                .map_err(|message| format!("Eval {}: {message}", eval.id))?;
            if !ids.insert(&eval.id) {
                return Err(format!(
                    "Duplicate local Eval in {}: {}",
                    self.name, eval.id
                ));
            }
        }
        if self.basis == Some(true) && !self.evals.is_empty() {
            return Err(format!("Basis Artifact {} cannot own Evals.", self.name));
        }
        self.views.validate()?;
        for (alias, target) in &self.mounts {
            identifier(alias, "Mount alias")?;
            identifier(target, "Mount target")?;
        }
        if let Some(stale) = &self.stale {
            stale.validate()?;
        }
        Ok(())
    }
}

/// Validate an ordinary Artifact declaration without opening scripts or declared inputs.
/// Family templates require workspace discovery to resolve their instance list and material.
pub fn parse_declaration(json: &str) -> Result<ArtifactDeclaration, String> {
    let value: Value = serde_json::from_str(json).map_err(|error| error.to_string())?;
    ordinary_declaration(value)
}

fn ordinary_declaration(value: Value) -> Result<ArtifactDeclaration, String> {
    if ["views", "evals"]
        .iter()
        .any(|key| value.get(key).is_some_and(families::parameterized))
    {
        return Err("$param references are allowed only in an Artifact family declaration.".into());
    }
    validated_declaration(value)
}

fn validated_declaration(value: Value) -> Result<ArtifactDeclaration, String> {
    let declaration: ArtifactDeclaration =
        serde_json::from_value(value).map_err(|error| error.to_string())?;
    declaration.validate()?;
    Ok(declaration)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub family: Option<families::FamilyMembership>,
    pub path: PathBuf,
    pub children: BTreeMap<String, String>,
    pub name: String,
    pub views: Views,
    pub mounts: BTreeMap<String, String>,
    pub basis: Option<bool>,
    pub stale: Option<Stale>,
    pub review_policy: Option<ReviewPolicy>,
}

#[derive(Debug, Serialize)]
pub struct Eval {
    /// Workspace-qualified identity; declaration.id remains the owner's local id.
    pub id: String,
    pub target: String,
    pub references: BTreeMap<String, String>,
    pub deps: Vec<String>,
    pub declaration: EvalDeclaration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewRequirement {
    Basis,
    Unreviewed,
    Evals,
}

#[derive(Debug)]
pub struct RepoConfig {
    pub root: PathBuf,
    pub artifacts: BTreeMap<String, Artifact>,
    /// Reserved family names mapped to shared physical folders; not Artifacts.
    pub families: BTreeMap<String, PathBuf>,
    pub evals: Vec<Eval>,
    pub relations: Vec<crate::scope::Relation>,
}

impl RepoConfig {
    /// Local obligations only; a basis does not satisfy its dependency scope.
    pub fn review_requirement(&self, artifact_id: &str) -> Option<ReviewRequirement> {
        let artifact = self.artifacts.get(artifact_id)?;
        Some(if artifact.basis == Some(true) {
            ReviewRequirement::Basis
        } else if self.evals.iter().any(|eval| eval.target == artifact_id) {
            ReviewRequirement::Evals
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
        families: BTreeMap::new(),
        evals: Vec::new(),
        relations: Vec::new(),
    };
    let mut pending = vec![(PathBuf::new(), None::<String>, None::<String>)];
    while let Some((relative, mut owner, mut family)) = pending.pop() {
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
            if let Some(family) = &family {
                return Err(ConfigError::new(
                    &file,
                    format!(
                        "Artifact family {family} cannot contain nested {CONFIG_FILE} markers."
                    ),
                ));
            }
            let json = fs::read_to_string(&file).map_err(|error| ConfigError::new(&file, error))?;
            let value: Value =
                serde_json::from_str(&json).map_err(|error| ConfigError::new(&file, error))?;
            let members = if value
                .as_object()
                .is_some_and(|object| object.contains_key("family"))
            {
                let expanded =
                    families::expand(&config.root, &relative, value.as_object().unwrap().clone())
                        .map_err(|error| ConfigError::new(&file, error))?;
                let name = &expanded[0].1.name;
                if config.artifacts.contains_key(name) || config.families.contains_key(name) {
                    return Err(ConfigError::new(
                        &file,
                        format!("Duplicate Artifact name: {name}"),
                    ));
                }
                config.families.insert(name.clone(), relative.clone());
                family = Some(name.clone());
                expanded
                    .into_iter()
                    .map(|(declaration, membership)| (declaration, Some(membership)))
                    .collect()
            } else {
                vec![(
                    ordinary_declaration(value).map_err(|error| ConfigError::new(&file, error))?,
                    None,
                )]
            };
            let parent = owner.clone();
            for (declaration, membership) in members {
                if !relative.as_os_str().is_empty() && declaration.review_policy.is_some() {
                    return Err(ConfigError::new(
                        &file,
                        "reviewPolicy belongs only to the repository root artifactize.json.",
                    ));
                }
                let ArtifactDeclaration {
                    name,
                    evals,
                    views,
                    mounts,
                    basis,
                    stale,
                    review_policy,
                } = declaration;
                if config.artifacts.contains_key(&name) || config.families.contains_key(&name) {
                    return Err(ConfigError::new(
                        &file,
                        format!("Duplicate Artifact name: {name}"),
                    ));
                }
                if let Some(parent) = &parent {
                    let parent = config.artifacts.get_mut(parent).unwrap();
                    let child = relative.strip_prefix(&parent.path).unwrap();
                    let child = if membership.is_some() {
                        child.join(&name)
                    } else {
                        child.to_owned()
                    };
                    let child = child
                        .to_str()
                        .ok_or_else(|| ConfigError::new(&file, "Artifact paths must be UTF-8."))?;
                    parent.children.insert(child.to_owned(), name.clone());
                }
                if membership.is_none() {
                    owner = Some(name.clone());
                }
                for declaration in evals {
                    config.evals.push(Eval {
                        id: format!("{name}/{}", declaration.id),
                        target: name.clone(),
                        references: BTreeMap::new(),
                        deps: Vec::new(),
                        declaration,
                    });
                }
                config.artifacts.insert(
                    name.clone(),
                    Artifact {
                        family: membership,
                        path: relative.clone(),
                        children: BTreeMap::new(),
                        name,
                        views,
                        mounts,
                        basis,
                        stale,
                        review_policy,
                    },
                );
            }
        }
        for entry in entries.into_iter().rev() {
            let kind = entry
                .file_type()
                .map_err(|error| ConfigError::new(entry.path(), error))?;
            if kind.is_dir() && entry.file_name() != ".git" && entry.file_name() != "node_modules" {
                pending.push((
                    relative.join(entry.file_name()),
                    owner.clone(),
                    family.clone(),
                ));
            }
        }
    }
    if config.artifacts.is_empty() {
        return Err(ConfigError::new(
            &config.root,
            "Workspace must contain at least one artifactize.json Artifact.",
        ));
    }
    crate::scope::resolve_config(&mut config)?;
    Ok(config)
}

#[cfg(test)]
mod tests;
