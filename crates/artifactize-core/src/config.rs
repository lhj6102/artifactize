//! Inert discovery and strict declarations.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use crate::types::{ArtifactName, EvalId};
mod identities;
pub use identities::{
    ChildPrefix, EndpointId, ExecutionPinName, HubEpoch, LogicalPath, ModelId, ProfileVariantName,
    ReferenceName, ToolName, ToolOperationName,
};
use serde_json::{Map, Value};
use thiserror::Error;

mod discovery;
mod location;
mod mutex;
pub use discovery::read_workspace_config;
pub(crate) use discovery::read_workspace_declarations;
use discovery::{logical_join, read_regular_text};
mod site;
pub use mutex::MutexTag;
pub(crate) use site::read_artifact_scope;
pub use site::{ScopeReadError, Site, read_sites};
use site::{declaration_of, folder_of};
mod plain;
mod schemas;
mod stored;
mod tools;
pub(crate) mod validation;
pub use stored::{Field, StoredPayload, StoredProfile};
pub use validation::MAX_TIMEOUT_MS;

pub use tools::{
    AgentTool, Builtin, BuiltinTool, CommandTool, HumanBuiltinTool, HumanCommandTool, HumanTool,
    HumanToolKind, SessionCommand, ToolProtocol, ToolSession,
};

use location::Location;
pub use validation::SPACE_SEPARATOR;
pub(crate) use validation::{artifact_name, identifier};
use validation::{path, paths, positive_integer, present, script, tags, text, timeout};

/// Format version for artifactize configuration documents.
pub const CONFIG_FORMAT_VERSION: u32 = 1;
pub const CONFIG_FILE: &str = "index.artf";
/// Gitignore-style folder patterns, at the workspace root, that discovery skips.
/// Bound named variants and declared input lists to keep discovery and preparation finite.
const MAX_DECLARED_ITEMS: usize = 64;
pub const IGNORE_FILE: &str = ".artfignore";

#[derive(Debug, Error)]
#[error("{}: {message}", crate::platform::path_text(.path))]
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

    /// Position cross-Artifact validation failures at their declaration's typed key.
    pub(crate) fn declaration(
        path: impl Into<PathBuf>,
        keys: &[&str],
        message: impl ToString,
    ) -> Self {
        let path = path.into();
        let message = message.to_string();
        let message = read_regular_text(&path).map_or(message.clone(), |source| {
            location::error_at(&source, keys, &message)
        });
        Self { path, message }
    }
}

/// A built-in provider: one artifactize reaches with a sign-in or an API key and no other setup.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    clap::ValueEnum,
)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    /// The ChatGPT/Codex account (artifactize login codex).
    Codex,
    /// The OpenAI API (OPENAI_API_KEY or a saved key).
    Openai,
    /// The Anthropic API (ANTHROPIC_API_KEY or a saved key).
    Anthropic,
    /// The Gemini API (GEMINI_API_KEY or a saved key).
    Gemini,
}

impl Provider {
    /// In the order Settings lists them.
    pub const ALL: [Self; 4] = [Self::Codex, Self::Openai, Self::Anthropic, Self::Gemini];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
            Self::Openai => "openai",
        }
    }

    fn named(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|provider| provider.as_str() == name)
    }
}

impl std::fmt::Display for Provider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Backends that 0.5.0 removed; a declaration naming one fails with its replacements.
const REMOVED_BACKENDS: [&str; 2] = ["chatgpt", "claude"];

/// What an Agent profile's `backend` names: a built-in [`Provider`], or a custom provider
/// set up on the machine that runs the eval. The project only declares the name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Backend(String);

impl Backend {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The built-in provider this names, or `None` for a custom provider.
    pub fn builtin(&self) -> Option<Provider> {
        Provider::named(&self.0)
    }
}

impl From<Provider> for Backend {
    fn from(provider: Provider) -> Self {
        Self(provider.as_str().to_owned())
    }
}

impl std::str::FromStr for Backend {
    type Err = String;
    fn from_str(name: &str) -> Result<Self, Self::Err> {
        if REMOVED_BACKENDS.contains(&name) {
            return Err(format!(
                r#"backend "{name}" was removed in 0.5.0; use "openai", "anthropic" or "gemini" with an API key, or "codex" with a ChatGPT/Codex sign-in"#
            ));
        }
        identifier(name, "backend")?;
        Ok(Self(name.to_owned()))
    }
}

impl<'de> Deserialize<'de> for Backend {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

impl std::fmt::Display for Backend {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Declared effort is exact: unsupported backend efforts fail rather than remap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Reasoning {
    None,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}
impl Reasoning {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
            Self::Max => "max",
        }
    }
}
impl std::fmt::Display for Reasoning {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum Profile {
    Agent {
        backend: Backend,
        model: ModelId,
        #[serde(default, deserialize_with = "present")]
        reasoning: Option<Reasoning>,
        #[serde(
            rename(serialize = "timeoutMs", deserialize = "timeout_ms"),
            default,
            deserialize_with = "timeout",
            serialize_with = "validation::milliseconds::serialize"
        )]
        timeout_ms: Option<std::time::Duration>,
        #[serde(
            rename(serialize = "maxToolCalls", deserialize = "max_tool_calls"),
            default,
            deserialize_with = "positive_integer"
        )]
        max_tool_calls: Option<u64>,
        #[serde(
            rename(serialize = "maxTokens", deserialize = "max_tokens"),
            default,
            deserialize_with = "positive_integer"
        )]
        max_tokens: Option<u64>,
    },
    Human {},
    Runtime {
        #[serde(serialize_with = "crate::platform::serialize_program")]
        command: String,
        args: Vec<crate::scope::Argument>,
        #[serde(
            rename(serialize = "timeoutMs", deserialize = "timeout_ms"),
            default,
            deserialize_with = "timeout",
            serialize_with = "validation::milliseconds::serialize"
        )]
        timeout_ms: Option<std::time::Duration>,
    },
}

impl Profile {
    fn validate(&self, location: &Location<'_, '_>) -> Result<(), String> {
        match self {
            Self::Agent {
                backend,
                model,
                reasoning,
                ..
            } => {
                location
                    .child("model")
                    .check(text(model, "Agent profile model"))?;
                // A custom provider's API is known only where it runs, so its effort is
                // checked there.
                if let (Some(reasoning), Some(provider)) = (reasoning, backend.builtin()) {
                    location.child("reasoning").check(
                        crate::llm::Api::from(provider).validate_reasoning(reasoning.as_str()),
                    )?;
                }
                Ok(())
            }
            Self::Human {} => Ok(()),
            Self::Runtime { command, args, .. } => location.check(script(command, args)),
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

/// An Eval's id within its Artifact, such as `follows-style`: the key of its
/// `[evals.<id>]` table, validated when the declaration is read.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct LocalEvalId(String);

impl<'de> Deserialize<'de> for LocalEvalId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

impl std::str::FromStr for LocalEvalId {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        identifier(value, "Eval id")?;
        Ok(Self(value.to_owned()))
    }
}

impl LocalEvalId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl PartialEq<&str> for LocalEvalId {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl std::ops::Deref for LocalEvalId {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for LocalEvalId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvalDeclaration {
    pub id: LocalEvalId,
    pub title: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mutex: Vec<MutexTag>,
    #[serde(flatten)]
    body: EvalBody,
}

/// Every Eval has an instruction and a complete execution profile.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct EvalBody {
    profile: Profile,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    profile_variants: BTreeMap<ProfileVariantName, Profile>,
    payload: EvalPayload,
    /// Each shared with every equal verdict schema in the same read.
    pass_schema: Option<std::sync::Arc<Map<String, Value>>>,
    fail_schema: Option<std::sync::Arc<Map<String, Value>>>,
}

impl EvalDeclaration {
    pub fn profile(&self) -> &Profile {
        &self.body.profile
    }
    pub fn payload(&self) -> Option<&EvalPayload> {
        Some(&self.body.payload)
    }
    pub fn profile_variants(&self) -> &BTreeMap<ProfileVariantName, Profile> {
        &self.body.profile_variants
    }
    pub fn pass_schema(&self) -> Option<&Map<String, Value>> {
        self.body.pass_schema.as_deref()
    }
    pub fn fail_schema(&self) -> Option<&Map<String, Value>> {
        self.body.fail_schema.as_deref()
    }
    #[cfg(test)]
    pub(crate) fn profile_mut(&mut self) -> &mut Profile {
        &mut self.body.profile
    }
    #[cfg(test)]
    pub(crate) fn set_pass_schema(&mut self, schema: Option<Map<String, Value>>) {
        self.body.pass_schema = schema.map(std::sync::Arc::new);
    }
    #[cfg(test)]
    pub(crate) fn set_fail_schema(&mut self, schema: Option<Map<String, Value>>) {
        self.body.fail_schema = schema.map(std::sync::Arc::new);
    }
    pub(crate) fn select_profile(&mut self, name: &ProfileVariantName) -> Result<(), String> {
        self.body.profile = self
            .body
            .profile_variants
            .get(name)
            .ok_or_else(|| format!("Unknown profile variant: {name}"))?
            .clone();
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
struct EvalFields {
    title: String,
    #[serde(default, deserialize_with = "present")]
    mutex: Option<Vec<MutexTag>>,
    profile: Profile,
    #[serde(default, deserialize_with = "present")]
    profile_variants: Option<BTreeMap<ProfileVariantName, Profile>>,
    #[serde(default, deserialize_with = "present")]
    payload: Option<EvalPayload>,
    #[serde(default, deserialize_with = "present")]
    pass_schema: Option<Map<String, Value>>,
    #[serde(default, deserialize_with = "present")]
    fail_schema: Option<Map<String, Value>>,
}

impl EvalDeclaration {
    /// Check the fields of the `[evals.<id>]` table that every profile needs together.
    fn new(id: LocalEvalId, fields: EvalFields) -> Result<Self, String> {
        if fields.mutex.is_some()
            && !matches!(
                fields.profile.kind(),
                ProfileKind::Runtime | ProfileKind::Agent
            )
        {
            return Err("mutex is supported only on runtime and agent Evals.".into());
        }
        let body = EvalBody {
            profile: fields.profile,
            profile_variants: fields.profile_variants.unwrap_or_default(),
            payload: fields
                .payload
                .ok_or("Eval payload is required for runtime, agent and human profiles.")?,
            pass_schema: fields.pass_schema.map(std::sync::Arc::new),
            fail_schema: fields.fail_schema.map(std::sync::Arc::new),
        };
        Ok(Self {
            id,
            title: fields.title,
            mutex: fields.mutex.unwrap_or_default(),
            body,
        })
    }
}

impl EvalDeclaration {
    fn validate(
        &self,
        location: &Location<'_, '_>,
        schemas: &mut schemas::Schemas,
    ) -> Result<(), String> {
        location
            .child("title")
            .check(text(&self.title, "Eval title"))?;
        if let Some(payload) = self.payload() {
            location
                .child("payload")
                .child("instruction")
                .check(text(&payload.instruction, "Eval payload.instruction"))?;
        }
        location
            .child("mutex")
            .check(mutex::validate(&self.mutex))?;
        self.profile().validate(&location.child("profile"))?;
        for (key, schema) in [
            ("pass_schema", &self.body.pass_schema),
            ("fail_schema", &self.body.fail_schema),
        ] {
            if let Some(schema) = schema {
                location
                    .child(key)
                    .check(schemas.check(schema, crate::agent::verdict::validate_schema))?;
                // A person enters a Human result in a form: only shapes it can enter.
                if self.profile().kind() == ProfileKind::Human {
                    location
                        .child(key)
                        .check(crate::review::check_schema(schema))?;
                }
            }
        }
        if self.profile_variants().len() > MAX_DECLARED_ITEMS {
            return Err(location
                .child("profile_variants")
                .error("profile_variants must contain at most 64 named profiles."));
        }
        for (name, profile) in self.profile_variants() {
            let variant = location.child("profile_variants").child(name);
            location
                .child("profile_variants")
                .key(name)
                .check(identifier(name, "Profile variant name"))?;
            profile.validate(&variant)?;
            if std::mem::discriminant(profile) != std::mem::discriminant(self.profile()) {
                return Err(
                    variant.error("Profile variants must retain the declared reviewer kind.")
                );
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(
    rename_all(serialize = "camelCase", deserialize = "snake_case"),
    deny_unknown_fields
)]
pub struct Views {
    #[serde(default)]
    pub agent_sessions: BTreeMap<ToolOperationName, ToolSession>,
    #[serde(default)]
    pub agent_tools: BTreeMap<ToolOperationName, AgentTool>,
    #[serde(default)]
    pub human_tools: BTreeMap<ToolOperationName, HumanTool>,
}

impl Views {
    fn validate(
        &self,
        location: &Location<'_, '_>,
        schemas: &mut schemas::Schemas,
    ) -> Result<(), String> {
        for (name, session) in &self.agent_sessions {
            let sessions = location.child("agent_sessions");
            sessions
                .key(name)
                .check(identifier(name, "Tool session name"))?;
            sessions
                .child(name)
                .child("start")
                .check(session.start.validate())?;
            sessions
                .child(name)
                .child("stop")
                .check(session.stop.validate())?;
        }
        for (name, tool) in &self.agent_tools {
            let tools = location.child("agent_tools");
            if let AgentTool::Command(tool) = tool
                && let Some(session) = &tool.session
                && !self.agent_sessions.contains_key(session)
            {
                return Err(tools
                    .child(name)
                    .child("session")
                    .error(format!("Unknown tool session: {session}")));
            }
            tools.key(name).check(identifier(name, "Agent tool name"))?;
            tools.child(name).check(
                tool.validate(schemas)
                    .map_err(|e| format!("Tool {name}: {e}")),
            )?;
        }
        for (name, tool) in &self.human_tools {
            let tools = location.child("human_tools");
            tools.key(name).check(identifier(name, "Human tool name"))?;
            tools
                .child(name)
                .check(tool.validate().map_err(|e| format!("Tool {name}: {e}")))?;
        }
        Ok(())
    }
}

/// The developer's definition of what an Artifact's reviews depend on: a script's output or
/// artifactsum, the built-in hash of the Artifact's own files. Its form is the plain
/// object; the script form keeps its `script` wrapper.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "Map<String, Value>", into = "Value")]
pub enum Fingerprint {
    /// Assigned by discovery only; it cannot be declared.
    Graph,
    Script {
        command: String,
        args: Vec<crate::scope::Argument>,
        /// Owner-relative paths that must exist on every call; never hashed.
        files: Vec<LogicalPath>,
        timeout_ms: Option<std::time::Duration>,
    },
    Artifactsum {
        files: Vec<LogicalPath>,
        ignore: crate::cache::IgnorePatterns,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScriptForm {
    script: ScriptFields,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
struct ScriptFields {
    command: String,
    args: Vec<crate::scope::Argument>,
    #[serde(default)]
    files: Vec<LogicalPath>,
    #[serde(
        default,
        deserialize_with = "timeout",
        serialize_with = "validation::milliseconds::serialize"
    )]
    timeout_ms: Option<std::time::Duration>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactsumForm {
    #[serde(default = "owner_root")]
    files: Vec<LogicalPath>,
    #[serde(default)]
    ignore: crate::cache::IgnorePatterns,
}

fn owner_root() -> Vec<LogicalPath> {
    vec![".".parse().expect("owner root logical path")]
}

impl Default for Fingerprint {
    fn default() -> Self {
        Self::Artifactsum {
            files: owner_root(),
            ignore: Default::default(),
        }
    }
}

fn default_fingerprint() -> Option<Fingerprint> {
    Some(Fingerprint::default())
}

fn declared_fingerprint<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Fingerprint>, D::Error> {
    use serde::de::Error;
    match Value::deserialize(deserializer)? {
        Value::Bool(false) => Ok(None),
        Value::Object(object) => Fingerprint::try_from(object)
            .map(Some)
            .map_err(D::Error::custom),
        _ => Err(D::Error::custom(
            "fingerprint must be false or an object declaring artifactsum (files, ignore) or a script.",
        )),
    }
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
                    "fingerprint.dependencies was removed in 0.5: a fingerprint covers only its own Artifact, and an eval's reuse key adds the fingerprints of its children and referenced Artifacts. Remove the field.",
                ));
            }
            let ArtifactsumForm { files, ignore } = serde_json::from_value(Value::Object(object))?;
            Self::Artifactsum { files, ignore }
        })
    }
}

impl From<Fingerprint> for Value {
    fn from(fingerprint: Fingerprint) -> Self {
        match fingerprint {
            Fingerprint::Graph => serde_json::json!({"graph": true}),
            Fingerprint::Script {
                command,
                args,
                files,
                timeout_ms,
            } => serde_json::json!({
                "script": {
                    "command": crate::platform::program_text(&command),
                    "args": args,
                    "files": files,
                    "timeoutMs": timeout_ms.map(|value| value.as_millis()),
                },
            }),
            Fingerprint::Artifactsum { files, ignore } => {
                serde_json::json!({"files": files, "ignore": ignore})
            }
        }
    }
}

impl Fingerprint {
    pub fn kind(&self) -> crate::types::FingerprintKind {
        match self {
            Self::Graph => crate::types::FingerprintKind::Graph,
            Self::Artifactsum { .. } => crate::types::FingerprintKind::Artifactsum,
            Self::Script { .. } => crate::types::FingerprintKind::Script,
        }
    }

    fn validate(&self, location: &Location<'_, '_>) -> Result<(), String> {
        match self {
            Self::Graph => Ok(()),
            Self::Script {
                command,
                args,
                files,
                ..
            } => {
                location.child("script").check(script(command, args))?;
                location
                    .child("script")
                    .child("files")
                    .check(paths(files, "fingerprint.script.files"))
            }
            Self::Artifactsum { files, .. } => {
                if files.is_empty()
                    || files.len() > MAX_DECLARED_ITEMS
                    || files.iter().collect::<BTreeSet<_>>().len() != files.len()
                {
                    return Err(location.child("files").error(
                        "fingerprint.files must contain 1–64 unique owner-relative paths.",
                    ));
                }
                for file in files.iter().filter(|file| *file != ".") {
                    location.child("files").check(
                        path(file).map_err(|message| format!("fingerprint.files: {message}")),
                    )?;
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ArtifactDeclaration {
    pub name: ArtifactName,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, deserialize_with = "declared_evals")]
    pub evals: Vec<EvalDeclaration>,
    #[serde(default)]
    pub views: Views,
    #[serde(default, deserialize_with = "present")]
    pub basis: Option<bool>,
    #[serde(
        default = "default_fingerprint",
        deserialize_with = "declared_fingerprint"
    )]
    pub fingerprint: Option<Fingerprint>,
    /// An aspect's paths, relative to its declaration's folder.
    #[serde(default, deserialize_with = "present")]
    pub select: Option<Vec<LogicalPath>>,
}

impl ArtifactDeclaration {
    /// Keep each of its schemas once in `schemas`, shared with the equal ones already read.
    fn share(&mut self, schemas: &mut schemas::Schemas) {
        for tool in self.views.agent_tools.values_mut() {
            if let AgentTool::Command(tool) = tool {
                tool.input_schema = schemas.tool(&tool.input_schema);
            }
        }
        for eval in &mut self.evals {
            for schema in [&mut eval.body.pass_schema, &mut eval.body.fail_schema]
                .into_iter()
                .flatten()
            {
                *schema = schemas.verdict(schema);
            }
        }
    }

    fn validate(
        &self,
        location: &Location<'_, '_>,
        schemas: &mut schemas::Schemas,
    ) -> Result<(), String> {
        location
            .child("name")
            .check(artifact_name(&self.name, "Artifact name"))?;
        location.child("tags").check(tags(&self.tags))?;
        for eval in &self.evals {
            eval.validate(&location.child("evals").key(&eval.id), schemas)
                .map_err(|message| format!("Eval {}: {message}", eval.id))?;
        }
        if self.basis == Some(true) && !self.evals.is_empty() {
            return Err(location
                .child("basis")
                .error(format!("Basis Artifact {} cannot own Evals.", self.name)));
        }
        self.views.validate(&location.child("views"), schemas)?;
        if let Some(fingerprint) = &self.fingerprint {
            fingerprint.validate(&location.child("fingerprint"))?;
        }
        if let Some(select) = &self.select {
            let select_location = location.child("select");
            if select.is_empty()
                || select.len() > MAX_DECLARED_ITEMS
                || select.iter().collect::<BTreeSet<_>>().len() != select.len()
            {
                return Err(select_location
                    .error("select must contain 1–64 unique paths relative to the declaration."));
            }
            for selected in select {
                select_location
                    .check(path(selected).map_err(|message| format!("select: {message}")))?;
                if selected.ends_with(".artf") {
                    return Err(select_location.error(
                        "select: Artifact declarations (*.artf) are never part of an aspect.",
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Eval ids live in table keys, not fields; BTreeMap also fixes their discovery order.
fn declared_evals<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<EvalDeclaration>, D::Error> {
    use serde::de::Error;
    BTreeMap::<String, EvalFields>::deserialize(deserializer)?
        .into_iter()
        .map(|(id, fields)| {
            let id = id.parse().map_err(D::Error::custom)?;
            EvalDeclaration::new(id, fields).map_err(D::Error::custom)
        })
        .collect()
}

/// Validate an Artifact declaration without opening scripts or declared inputs.
pub fn parse_declaration(source: &str) -> Result<ArtifactDeclaration, String> {
    parse_declaration_in(source, &mut schemas::Schemas::default())
}

/// [`parse_declaration`] as one of a read's declarations, its schemas shared through `schemas`.
fn parse_declaration_in(
    source: &str,
    schemas: &mut schemas::Schemas,
) -> Result<ArtifactDeclaration, String> {
    let table = toml::de::DeTable::parse(source).map_err(|error| error.to_string())?;
    let span = table.span();
    let document = toml::Spanned::new(span, toml::de::DeValue::Table(table.into_inner()));
    let location = Location::root(source, &document);
    for (key, value) in document.get_ref().as_table().unwrap() {
        reject_non_json_values(value, key.get_ref().as_ref(), source)?;
    }
    if document.get_ref().get("family").is_some() {
        return Err(location
            .child("family")
            .error("family was removed in 0.9.0; declare each instance as its own Artifact."));
    }
    for key in ["stale_key", "stale"] {
        if document.get_ref().get(key).is_some() {
            return Err(location.child(key).error(format!(
                "{key} was renamed to fingerprint: use fingerprint = {{ files = [\".\"], ignore = [] }} or [fingerprint.script]."
            )));
        }
    }
    if let Some(evals) = document
        .get_ref()
        .get("evals")
        .and_then(|value| value.get_ref().as_table())
    {
        for (id, eval) in evals {
            if eval.get_ref().get("gate").is_some() {
                return Err(location
                    .child("evals")
                    .child(id.get_ref())
                    .child("gate")
                    .error(format!(
                        "Eval {}: gate was removed in 0.16.0; remove it. A Human sign-off never holds up other evals.",
                        id.get_ref()
                    )));
            }
            if eval.get_ref().get("result_check").is_some() {
                return Err(location
                    .child("evals")
                    .child(id.get_ref())
                    .child("result_check")
                    .error(format!(
                        "Eval {}: result_check was removed in 0.6.0; remove it. The review's tool calls are in its saved session (artifactize session show).",
                        id.get_ref()
                    )));
            }
        }
    }
    // TOML buffers enum/table-key deserialization. Check lexical fields at their
    // own source positions before constructing validated identities, not in discovery.
    validate_identity_locations(&location, &document)?;
    // These sum types use JSON maps internally; pin field errors to their original spans.
    if let Some(fingerprint) = document.get_ref().get("fingerprint")
        && let Some(fields) = fingerprint.get_ref().as_table()
    {
        let fingerprint_location = location.child("fingerprint");
        if fields.contains_key("script") {
            fingerprint_location.deserialize::<ScriptForm>()?;
        } else if !fields.contains_key("dependencies") {
            fingerprint_location.deserialize::<ArtifactsumForm>()?;
        }
    }
    if let Some(tools) = document
        .get_ref()
        .get("views")
        .and_then(|views| views.get_ref().get("agent_tools"))
        .and_then(|tools| tools.get_ref().as_table())
    {
        for (name, tool) in tools {
            if tool.get_ref().as_table().is_some() {
                let tool_location = location
                    .child("views")
                    .child("agent_tools")
                    .child(name.get_ref());
                if tool.get_ref().get("builtin").is_some() {
                    tool_location.deserialize::<BuiltinTool>()?;
                } else {
                    tool_location.deserialize::<CommandTool>()?;
                }
            }
        }
    }
    if let Some(tools) = document
        .get_ref()
        .get("views")
        .and_then(|views| views.get_ref().get("human_tools"))
        .and_then(|tools| tools.get_ref().as_table())
    {
        for (name, _) in tools {
            location
                .child("views")
                .child("human_tools")
                .child(name.get_ref())
                .deserialize::<HumanTool>()?;
        }
    }
    if let Some(sessions) = document
        .get_ref()
        .get("views")
        .and_then(|views| views.get_ref().get("agent_sessions"))
        .and_then(|sessions| sessions.get_ref().as_table())
    {
        for (name, _) in sessions {
            location
                .child("views")
                .child("agent_sessions")
                .child(name.get_ref())
                .deserialize::<ToolSession>()?;
        }
    }
    let mut declaration =
        ArtifactDeclaration::deserialize(toml::de::ValueDeserializer::from(document.clone()))
            .map_err(|mut error| {
                error.set_input(Some(source));
                error.to_string()
            })?;
    declaration.share(schemas);
    declaration.validate(&location, schemas)?;
    Ok(declaration)
}

fn validate_identity_locations(
    location: &Location<'_, '_>,
    document: &toml::Spanned<toml::de::DeValue<'_>>,
) -> Result<(), String> {
    use toml::de::DeValue;
    if let Some(name) = document.get_ref().get("name")
        && let DeValue::String(name) = name.get_ref()
    {
        location
            .child("name")
            .check(artifact_name(name, "Artifact name"))?;
    }
    if let Some(evals) = document
        .get_ref()
        .get("evals")
        .and_then(|value| value.get_ref().as_table())
    {
        for (id, eval) in evals {
            let eval_location = location.child("evals").key(id.get_ref());
            eval_location.check(identifier(id.get_ref(), "Eval id"))?;
            if let Some(model) = eval
                .get_ref()
                .get("profile")
                .and_then(|profile| profile.get_ref().get("model"))
                && let DeValue::String(model) = model.get_ref()
            {
                eval_location
                    .child("profile")
                    .child("model")
                    .check(text(model, "Agent profile model"))?;
            }
            if let Some(variants) = eval
                .get_ref()
                .get("profile_variants")
                .and_then(|value| value.get_ref().as_table())
            {
                for (name, profile) in variants {
                    let variant = eval_location.child("profile_variants").key(name.get_ref());
                    variant.check(identifier(name.get_ref(), "Profile variant name"))?;
                    if let Some(model) = profile.get_ref().get("model")
                        && let DeValue::String(model) = model.get_ref()
                    {
                        variant
                            .child("model")
                            .check(text(model, "Agent profile model"))?;
                    }
                }
            }
        }
    }
    Ok(())
}

/// Reject TOML values that would become lossy or implementation-specific owner JSON.
fn reject_non_json_values(
    value: &toml::Spanned<toml::de::DeValue<'_>>,
    path: &str,
    source: &str,
) -> Result<(), String> {
    use toml::de::DeValue;
    let diagnostic = |message| location::positioned(source, value.span().start, path, message);
    match value.get_ref() {
        DeValue::Datetime(_) => Err(diagnostic(
            "TOML date/time values have no JSON equivalent; use a quoted string.",
        )),
        DeValue::Float(number)
            if matches!(
                number.as_str(),
                "inf" | "+inf" | "-inf" | "nan" | "+nan" | "-nan"
            ) =>
        {
            Err(diagnostic(
                "TOML non-finite floats have no JSON equivalent.",
            ))
        }
        DeValue::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                reject_non_json_values(item, &format!("{path}[{index}]"), source)?;
            }
            Ok(())
        }
        DeValue::Table(table) => {
            for (key, value) in table {
                reject_non_json_values(value, &format!("{path}.{}", key.get_ref()), source)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

pub use artifactize_tools::scope::ArtifactKind;

/// Nested folder paths with their Artifacts as tool-side IDs. Discovery validated both, so a
/// failure here is a bug.
fn tool_nested(
    nested: &BTreeMap<ChildPrefix, ArtifactName>,
) -> BTreeMap<artifactize_tools::scope::ChildPrefix, artifactize_tools::scope::ArtifactId> {
    nested
        .iter()
        .map(|(path, name)| {
            (
                artifactize_tools::scope::ChildPrefix::new(path.as_ref())
                    .expect("validated child prefix"),
                crate::scope::tool_id(name),
            )
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    #[serde(with = "crate::platform::path_serde")]
    pub path: PathBuf,
    pub kind: ArtifactKind,
    /// Folder Artifacts declared in subfolders, by their path here. Their files are theirs,
    /// not this Artifact's; nesting is no relation between the two.
    pub nested: BTreeMap<ChildPrefix, ArtifactName>,
    /// An aspect's selected paths below its folder; empty for other kinds.
    pub select: Vec<LogicalPath>,
    pub name: ArtifactName,
    pub tags: Vec<String>,
    pub views: Views,
    pub basis: Option<bool>,
    pub fingerprint: Option<Fingerprint>,
}

impl Artifact {
    pub(crate) fn tool_scope(&self) -> artifactize_tools::scope::Artifact {
        artifactize_tools::scope::Artifact {
            path: self.path.clone(),
            kind: self.kind,
            name: self.name.to_string(),
            nested: tool_nested(&self.nested),
            select: self.select.iter().map(|path| path.to_string()).collect(),
        }
    }

    /// Runtime commands and relative declarations use the containing folder.
    pub fn folder(&self) -> &Path {
        folder_of(self.kind, &self.path)
    }

    /// What `{name}` stands for: the folder or file, or an aspect's folder, since an
    /// aspect's path is only its declaration's label.
    pub fn referenced_path(&self) -> &Path {
        if self.kind == ArtifactKind::Aspect {
            self.folder()
        } else {
            &self.path
        }
    }

    pub fn file_name(&self) -> Option<&str> {
        (self.kind == ArtifactKind::File)
            .then(|| self.path.file_name().and_then(|name| name.to_str()))
            .flatten()
    }

    pub fn declaration_path(&self) -> PathBuf {
        declaration_of(self.kind, &self.path)
    }

    /// Where it is declared.
    pub fn site(&self) -> Site {
        Site {
            kind: self.kind,
            path: self.path.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Eval {
    /// Workspace-qualified id; declaration.id remains the owner's local id.
    pub id: EvalId,
    pub target: ArtifactName,
    pub references: BTreeMap<crate::config::ReferenceName, ArtifactName>,
    pub deps: Vec<ArtifactName>,
    pub declaration: EvalDeclaration,
    /// The `profileVariants` entry selected for this execution, if any: an execution option,
    /// recorded with results but never part of the saved declaration.
    #[serde(skip)]
    pub variant: Option<ProfileVariantName>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewRequirement {
    Basis,
    Unreviewed,
    Evals,
    /// No evals of its own: its constituents' verdicts are its verdict.
    Constituents,
}

#[derive(Debug)]
pub struct RepoConfig {
    pub root: PathBuf,
    pub artifacts: BTreeMap<ArtifactName, Artifact>,
    pub evals: Vec<Eval>,
    pub relations: Vec<crate::scope::Relation>,
}

impl RepoConfig {
    /// The Artifact `name` is a constituent of: its nearest declared leading name, `game` for
    /// `game::combat::fireball` when `game::combat` is not declared.
    pub fn parent(&self, name: &ArtifactName) -> Option<&ArtifactName> {
        name.ancestors()
            .find_map(|ancestor| self.artifacts.get_key_value(ancestor))
            .map(|(id, _)| id)
    }

    /// Every Artifact `name` is composed of, directly or through its constituents: each
    /// name under `name::`, since `name` or one of them is its nearest declared leading name.
    pub fn descendants<'a>(
        &'a self,
        name: &'a ArtifactName,
    ) -> impl Iterator<Item = &'a ArtifactName> + 'a {
        let prefix = format!("{name}{SPACE_SEPARATOR}");
        self.artifacts
            .range::<str, _>((
                std::ops::Bound::Excluded(prefix.as_str()),
                std::ops::Bound::Unbounded,
            ))
            .map(|(id, _)| id)
            .take_while(move |id| id.as_str().starts_with(&prefix))
    }

    /// Local obligations only; a basis does not satisfy its dependency scope.
    pub fn review_requirement(&self, artifact_id: &str) -> Option<ReviewRequirement> {
        let artifact = self.artifacts.get(artifact_id)?;
        Some(if artifact.basis == Some(true) {
            ReviewRequirement::Basis
        } else if self.evals.iter().any(|eval| eval.target == artifact_id) {
            ReviewRequirement::Evals
        } else if self.descendants(&artifact.name).next().is_some() {
            ReviewRequirement::Constituents
        } else {
            ReviewRequirement::Unreviewed
        })
    }
}

#[cfg(test)]
mod tests;
