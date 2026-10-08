//! Inert discovery, strict declarations, and family expansion.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

pub mod families;
mod stored;
mod tools;
pub(crate) mod validation;
pub use stored::{Field, StoredPayload, StoredProfile};

pub use tools::{
    AgentTool, Builtin, BuiltinTool, CommandTool, HumanTool, HumanToolKind, ToolProtocol,
};

pub(crate) use validation::identifier;
use validation::{path, paths, positive_integer, present, script, text, timeout};

/// Format version for artifactize configuration documents.
pub const CONFIG_FORMAT_VERSION: u32 = 1;
pub const CONFIG_FILE: &str = "artifactize.json";
/// Gitignore-style folder patterns, at the workspace root, that discovery skips.
/// Bound named variants and declared input lists to keep discovery and preparation finite.
const MAX_DECLARED_ITEMS: usize = 64;
pub const IGNORE_FILE: &str = ".artifactizeignore";

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Openai,
    Anthropic,
    Codex,
}

/// Backends that 0.5.0 removed; a declaration naming one fails with its replacements.
const REMOVED_BACKENDS: [&str; 2] = ["chatgpt", "claude"];

impl<'de> Deserialize<'de> for Backend {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let name = String::deserialize(deserializer)?;
        match name.as_str() {
            "openai" => Ok(Self::Openai),
            "anthropic" => Ok(Self::Anthropic),
            "codex" => Ok(Self::Codex),
            removed if REMOVED_BACKENDS.contains(&removed) => Err(D::Error::custom(format!(
                r#"backend "{removed}" was removed in 0.5.0; use "openai" or "anthropic" with an API key, or "codex" with a ChatGPT/Codex sign-in"#
            ))),
            other => Err(D::Error::unknown_variant(
                other,
                &["openai", "anthropic", "codex"],
            )),
        }
    }
}

impl Backend {
    pub fn validate_reasoning(self, reasoning: &str) -> Result<(), String> {
        let supported = match self {
            // Codex takes the Responses efforts as they are, as Pi's openai-codex does.
            Self::Openai | Self::Codex => matches!(
                reasoning,
                "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
            ),
            Self::Anthropic => matches!(reasoning, "low" | "medium" | "high" | "max"),
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
        #[serde(
            rename = "timeoutMs",
            default,
            deserialize_with = "timeout",
            serialize_with = "validation::milliseconds::serialize"
        )]
        timeout_ms: Option<std::time::Duration>,
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
        #[serde(
            rename = "timeoutMs",
            default,
            deserialize_with = "timeout",
            serialize_with = "validation::milliseconds::serialize"
        )]
        timeout_ms: Option<std::time::Duration>,
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

    pub fn kind(&self) -> ProfileKind {
        match self {
            Self::Agent { .. } => ProfileKind::Agent,
            Self::Human {} => ProfileKind::Human,
            Self::Runtime { .. } => ProfileKind::Runtime,
        }
    }
}

/// The kind of reviewer a profile names, without its execution options.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, clap::ValueEnum,
)]
#[serde(rename_all = "lowercase")]
pub enum ProfileKind {
    /// Runtime evals: a command's exit code.
    Runtime,
    /// Agent evals: a model's review.
    Agent,
    /// Human evals: a person's sign-off.
    Human,
}

impl ProfileKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Runtime => "runtime",
            Self::Agent => "agent",
            Self::Human => "human",
        }
    }
}

/// The required instruction is typed at the declaration and saved-request boundaries;
/// owner-defined context remains extensible JSON with the same object representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalPayload {
    pub instruction: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvalDeclaration {
    pub id: String,
    pub title: String,
    pub profile: Profile,
    #[serde(default)]
    pub profile_variants: BTreeMap<String, Profile>,
    pub payload: EvalPayload,
    #[serde(default, deserialize_with = "present")]
    pub pass_schema: Option<Map<String, Value>>,
    #[serde(default, deserialize_with = "present")]
    pub fail_schema: Option<Map<String, Value>>,
}

impl EvalDeclaration {
    fn validate(&self) -> Result<(), String> {
        identifier(&self.id, "Eval id")?;
        text(&self.title, "Eval title")?;
        text(&self.payload.instruction, "Eval payload.instruction")?;
        self.profile.validate()?;
        for schema in [&self.pass_schema, &self.fail_schema].into_iter().flatten() {
            crate::agent::verdict::validate_schema(schema)?;
        }
        if self.profile_variants.len() > MAX_DECLARED_ITEMS {
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Views {
    #[serde(default)]
    pub agent_tools: BTreeMap<String, AgentTool>,
    #[serde(default)]
    pub human_tools: BTreeMap<String, HumanTool>,
}

impl Views {
    fn validate(&self) -> Result<(), String> {
        for (name, tool) in &self.agent_tools {
            identifier(name, "Agent tool name")?;
            tool.validate().map_err(|e| format!("Tool {name}: {e}"))?;
        }
        for (name, tool) in &self.human_tools {
            identifier(name, "Human tool name")?;
            tool.validate().map_err(|e| format!("Tool {name}: {e}"))?;
        }
        Ok(())
    }
}

/// The developer's definition of what an Artifact's reviews depend on: a script's output or
/// the built-in hash of the Artifact's own files, used as is. The content form is the plain
/// object; the script form keeps its `script` wrapper.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "Map<String, Value>", into = "Value")]
pub enum Fingerprint {
    Script {
        command: String,
        args: Vec<String>,
        /// Owner-relative paths that must exist on every call; never hashed.
        files: Vec<String>,
        timeout_ms: Option<std::time::Duration>,
    },
    Content {
        files: Vec<String>,
        ignore: Vec<String>,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScriptForm {
    script: ScriptFields,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ScriptFields {
    command: String,
    args: Vec<String>,
    #[serde(default)]
    files: Vec<String>,
    #[serde(
        default,
        deserialize_with = "timeout",
        serialize_with = "validation::milliseconds::serialize"
    )]
    timeout_ms: Option<std::time::Duration>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ContentForm {
    #[serde(default = "owner_root")]
    files: Vec<String>,
    #[serde(default)]
    ignore: Vec<String>,
}

fn owner_root() -> Vec<String> {
    vec![".".into()]
}

impl TryFrom<Map<String, Value>> for Fingerprint {
    type Error = serde_json::Error;

    fn try_from(object: Map<String, Value>) -> Result<Self, Self::Error> {
        Ok(if object.contains_key("script") {
            let ScriptForm {
                script:
                    ScriptFields {
                        command,
                        args,
                        files,
                        timeout_ms,
                    },
            } = serde_json::from_value(Value::Object(object))?;
            Self::Script {
                command,
                args,
                files,
                timeout_ms,
            }
        } else {
            if object.contains_key("dependencies") {
                return Err(serde::de::Error::custom(
                    "fingerprint.dependencies was removed in 0.5: a fingerprint covers only its own Artifact, and an eval's reuse key adds the fingerprints of its mounts, children and referenced Artifacts. Remove the field.",
                ));
            }
            let ContentForm { files, ignore } = serde_json::from_value(Value::Object(object))?;
            Self::Content { files, ignore }
        })
    }
}

impl From<Fingerprint> for Value {
    fn from(fingerprint: Fingerprint) -> Self {
        match fingerprint {
            Fingerprint::Script {
                command,
                args,
                files,
                timeout_ms,
            } => serde_json::json!({"script": {
                "command": command, "args": args, "files": files, "timeoutMs": timeout_ms.map(|value| value.as_millis()),
            }}),
            Fingerprint::Content { files, ignore } => {
                serde_json::json!({"files": files, "ignore": ignore})
            }
        }
    }
}

impl Fingerprint {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Script {
                command,
                args,
                files,
                ..
            } => {
                script(command, args)?;
                paths(files, "fingerprint.script.files")
            }
            Self::Content { files, ignore, .. } => {
                if files.is_empty()
                    || files.len() > MAX_DECLARED_ITEMS
                    || files.iter().collect::<BTreeSet<_>>().len() != files.len()
                {
                    return Err(
                        "fingerprint.files must contain 1–64 unique owner-relative paths.".into(),
                    );
                }
                for file in files.iter().filter(|file| *file != ".") {
                    path(file).map_err(|message| format!("fingerprint.files: {message}"))?;
                }
                crate::cache::ignore_patterns(ignore)
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
    pub fingerprint: Option<Fingerprint>,
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
        if let Some(fingerprint) = &self.fingerprint {
            fingerprint.validate()?;
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
    // Both earlier names fail with the new shape instead of an unknown-field error.
    for key in ["staleKey", "stale"] {
        if value.get(key).is_some() {
            return Err(format!(
                r#"{key} was renamed to fingerprint: use "fingerprint": {{"files": ["."], "ignore": []}} or "fingerprint": {{"script": {{...}}}}."#
            ));
        }
    }
    for eval in value["evals"].as_array().into_iter().flatten() {
        if eval.get("resultCheck").is_some() {
            return Err(format!(
                "Eval {}: resultCheck was removed in 0.6.0; remove it. The review's tool calls are in its saved session (artifactize session show).",
                eval["id"].as_str().unwrap_or("?")
            ));
        }
    }
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
    pub fingerprint: Option<Fingerprint>,
    pub review_policy: Option<ReviewPolicy>,
}

#[derive(Debug, Serialize)]
pub struct Eval {
    /// Workspace-qualified id; declaration.id remains the owner's local id.
    pub id: String,
    pub target: String,
    pub references: BTreeMap<String, String>,
    pub deps: Vec<String>,
    pub declaration: EvalDeclaration,
    /// The `profileVariants` entry selected for this execution, if any: an execution option,
    /// recorded with results but never part of the saved declaration.
    #[serde(skip)]
    pub variant: Option<String>,
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
    let root =
        crate::platform::canonicalize(repo).map_err(|error| ConfigError::new(repo, error))?;
    let mut config = RepoConfig {
        root,
        artifacts: BTreeMap::new(),
        families: BTreeMap::new(),
        evals: Vec::new(),
        relations: Vec::new(),
    };
    let ignored = discovery_ignore(&config.root)?;
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
                    fingerprint,
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
                        logical_join(child, name.as_ref())
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
                        variant: None,
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
                        fingerprint,
                        review_policy,
                    },
                );
            }
        }
        for entry in entries.into_iter().rev() {
            let kind = entry
                .file_type()
                .map_err(|error| ConfigError::new(entry.path(), error))?;
            if kind.is_dir()
                && entry.file_name() != ".git"
                && entry.file_name() != "node_modules"
                && !ignored.matched(entry.path(), true).is_ignore()
            {
                pending.push((
                    logical_join(&relative, &entry.file_name()),
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

/// The folders the workspace root's `.artifactizeignore` (gitignore syntax) keeps out of
/// discovery, such as test fixtures or example projects with their own `artifactize.json`.
fn discovery_ignore(root: &Path) -> Result<ignore::gitignore::Gitignore, ConfigError> {
    let file = root.join(IGNORE_FILE);
    let mut builder = ignore::gitignore::GitignoreBuilder::new(root);
    // A regular file only: discovery never follows links.
    if fs::symlink_metadata(&file).is_ok_and(|metadata| metadata.is_file())
        && let Some(error) = builder.add(&file)
    {
        return Err(ConfigError::new(&file, error));
    }
    builder
        .build()
        .map_err(|error| ConfigError::new(&file, error))
}

/// A logical path below the workspace root: components joined with `/` on every platform,
/// the form Artifact paths take in scopes, references and output.
fn logical_join(parent: &Path, name: &std::ffi::OsStr) -> PathBuf {
    if parent.as_os_str().is_empty() {
        return PathBuf::from(name);
    }
    let mut path = parent.as_os_str().to_owned();
    path.push("/");
    path.push(name);
    PathBuf::from(path)
}

#[cfg(test)]
mod tests;
