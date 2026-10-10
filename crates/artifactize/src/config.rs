//! Inert discovery and strict declarations.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use crate::types::{ArtifactName, EvalId};
mod identities;
pub use identities::{
    ChildPrefix, EndpointId, HubEpoch, LogicalPath, ModelId, MountAlias, ProfileVariantName,
    ToolName, ToolOperationName,
};
use serde_json::{Map, Value};
use thiserror::Error;

mod discovery;
mod location;
pub use discovery::read_workspace_config;
use discovery::{logical_join, read_regular_text};
mod stored;
mod tools;
pub(crate) mod validation;
pub use stored::{Field, StoredPayload, StoredProfile};

pub use tools::{
    AgentTool, Builtin, BuiltinTool, CommandTool, HumanBuiltinTool, HumanCommandTool, HumanTool,
    HumanToolKind, ToolProtocol,
};

use location::Location;
pub(crate) use validation::identifier;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    // Preserve lexicographic wire-name ordering in maps that used String keys.
    Anthropic,
    Codex,
    Openai,
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

impl std::fmt::Display for Backend {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Openai => "openai",
            Self::Anthropic => "anthropic",
            Self::Codex => "codex",
        })
    }
}

impl Backend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::Codex => "codex",
            Self::Openai => "openai",
        }
    }
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
    Dependency {
        #[serde(rename(serialize = "dependsOn", deserialize = "depends_on"))]
        depends_on: Vec<ArtifactName>,
    },
    Runtime {
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
                if let Some(reasoning) = reasoning {
                    location
                        .child("reasoning")
                        .check(backend.validate_reasoning(reasoning.as_str()))?;
                }
                Ok(())
            }
            Self::Human {} => Ok(()),
            Self::Dependency { depends_on } => {
                if depends_on.is_empty()
                    || depends_on.len() > MAX_DECLARED_ITEMS
                    || depends_on.iter().collect::<BTreeSet<_>>().len() != depends_on.len()
                {
                    return Err(location.child("depends_on").error(
                        "Dependency profile depends_on must contain 1–64 unique Artifact names.",
                    ));
                }
                for name in depends_on {
                    location
                        .child("depends_on")
                        .check(identifier(name, "Dependency Artifact name"))?;
                }
                Ok(())
            }
            Self::Runtime { command, args, .. } => location.check(script(command, args)),
        }
    }

    pub fn kind(&self) -> ProfileKind {
        match self {
            Self::Agent { .. } => ProfileKind::Agent,
            Self::Human {} => ProfileKind::Human,
            Self::Dependency { .. } => ProfileKind::Dependency,
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
    /// Dependency evals: derived from current required Artifact evidence.
    Dependency,
}

impl ProfileKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Runtime => "runtime",
            Self::Agent => "agent",
            Self::Human => "human",
            Self::Dependency => "dependency",
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
    #[serde(flatten)]
    body: EvalBody,
}

/// Dependency declarations have no payload, schemas or execution variants.
/// Private construction parses the cross-field invariant once at the configuration edge.
#[derive(Debug, Clone)]
enum EvalBody {
    Dependency {
        profile: Profile,
    },
    Executable {
        profile: Profile,
        profile_variants: BTreeMap<ProfileVariantName, Profile>,
        payload: EvalPayload,
        pass_schema: Option<Map<String, Value>>,
        fail_schema: Option<Map<String, Value>>,
    },
}

impl EvalDeclaration {
    pub fn profile(&self) -> &Profile {
        match &self.body {
            EvalBody::Dependency { profile } | EvalBody::Executable { profile, .. } => profile,
        }
    }
    pub fn payload(&self) -> Option<&EvalPayload> {
        match &self.body {
            EvalBody::Dependency { .. } => None,
            EvalBody::Executable { payload, .. } => Some(payload),
        }
    }
    pub fn profile_variants(&self) -> &BTreeMap<ProfileVariantName, Profile> {
        static EMPTY: std::sync::OnceLock<BTreeMap<ProfileVariantName, Profile>> =
            std::sync::OnceLock::new();
        match &self.body {
            EvalBody::Dependency { .. } => EMPTY.get_or_init(BTreeMap::new),
            EvalBody::Executable {
                profile_variants, ..
            } => profile_variants,
        }
    }
    pub fn pass_schema(&self) -> Option<&Map<String, Value>> {
        match &self.body {
            EvalBody::Dependency { .. } => None,
            EvalBody::Executable { pass_schema, .. } => pass_schema.as_ref(),
        }
    }
    pub fn fail_schema(&self) -> Option<&Map<String, Value>> {
        match &self.body {
            EvalBody::Dependency { .. } => None,
            EvalBody::Executable { fail_schema, .. } => fail_schema.as_ref(),
        }
    }
    #[cfg(test)]
    pub(crate) fn profile_mut(&mut self) -> &mut Profile {
        match &mut self.body {
            EvalBody::Dependency { profile } | EvalBody::Executable { profile, .. } => profile,
        }
    }
    #[cfg(test)]
    pub(crate) fn pass_schema_mut(&mut self) -> &mut Option<Map<String, Value>> {
        match &mut self.body {
            EvalBody::Executable { pass_schema, .. } => pass_schema,
            _ => panic!("dependency schema is impossible"),
        }
    }
    #[cfg(test)]
    pub(crate) fn fail_schema_mut(&mut self) -> &mut Option<Map<String, Value>> {
        match &mut self.body {
            EvalBody::Executable { fail_schema, .. } => fail_schema,
            _ => panic!("dependency schema is impossible"),
        }
    }
    /// Only a declared, same-kind executable variant can replace the active profile.
    pub(crate) fn select_profile(&mut self, name: &ProfileVariantName) -> Result<(), String> {
        let EvalBody::Executable {
            profile,
            profile_variants,
            ..
        } = &mut self.body
        else {
            return Err("Dependency Evals cannot select profile variants.".into());
        };
        *profile = profile_variants
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
        if matches!(fields.profile, Profile::Dependency { .. }) {
            if fields.payload.is_some()
                || fields.pass_schema.is_some()
                || fields.fail_schema.is_some()
                || fields.profile_variants.is_some()
            {
                return Err(
                    "Dependency Evals cannot declare payload, pass_schema, fail_schema or profile_variants."
                        .into(),
                );
            }
        } else if fields.payload.is_none() {
            return Err("Eval payload is required for runtime, agent and human profiles.".into());
        }
        let body = if matches!(fields.profile, Profile::Dependency { .. }) {
            EvalBody::Dependency {
                profile: fields.profile,
            }
        } else {
            EvalBody::Executable {
                profile: fields.profile,
                profile_variants: fields.profile_variants.unwrap_or_default(),
                payload: fields.payload.expect("executable payload checked above"),
                pass_schema: fields.pass_schema,
                fail_schema: fields.fail_schema,
            }
        };
        Ok(Self {
            id,
            title: fields.title,
            body,
        })
    }
}

impl EvalDeclaration {
    fn validate(&self, location: &Location<'_, '_>) -> Result<(), String> {
        location
            .child("title")
            .check(text(&self.title, "Eval title"))?;
        if let Some(payload) = self.payload() {
            location
                .child("payload")
                .child("instruction")
                .check(text(&payload.instruction, "Eval payload.instruction"))?;
        }
        self.profile().validate(&location.child("profile"))?;
        for (key, schema) in [
            ("pass_schema", self.pass_schema()),
            ("fail_schema", self.fail_schema()),
        ] {
            if let Some(schema) = schema {
                location
                    .child(key)
                    .check(crate::agent::verdict::validate_schema(schema))?;
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
    pub agent_tools: BTreeMap<ToolOperationName, AgentTool>,
    #[serde(default)]
    pub human_tools: BTreeMap<ToolOperationName, HumanTool>,
}

impl Views {
    fn validate(&self, location: &Location<'_, '_>) -> Result<(), String> {
        for (name, tool) in &self.agent_tools {
            let tools = location.child("agent_tools");
            tools.key(name).check(identifier(name, "Agent tool name"))?;
            tools
                .child(name)
                .check(tool.validate().map_err(|e| format!("Tool {name}: {e}")))?;
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
    Script {
        command: String,
        args: Vec<crate::scope::Argument>,
        /// Owner-relative paths that must exist on every call; never hashed.
        files: Vec<LogicalPath>,
        timeout_ms: Option<std::time::Duration>,
    },
    Artifactsum {
        files: Vec<LogicalPath>,
        ignore: Vec<String>,
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
    ignore: Vec<String>,
}

fn owner_root() -> Vec<LogicalPath> {
    vec![".".parse().expect("owner root logical path")]
}

impl Default for Fingerprint {
    fn default() -> Self {
        Self::Artifactsum {
            files: owner_root(),
            ignore: Vec::new(),
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
                    "fingerprint.dependencies was removed in 0.5: a fingerprint covers only its own Artifact, and an eval's reuse key adds the fingerprints of its mounts, children and referenced Artifacts. Remove the field.",
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
            Fingerprint::Script {
                command,
                args,
                files,
                timeout_ms,
            } => serde_json::json!({
                "script": {
                    "command": command,
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
    fn validate(&self, location: &Location<'_, '_>) -> Result<(), String> {
        match self {
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
            Self::Artifactsum { files, ignore, .. } => {
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
                location
                    .child("ignore")
                    .check(crate::cache::ignore_patterns(ignore))
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
#[serde(
    rename_all(serialize = "camelCase", deserialize = "snake_case"),
    deny_unknown_fields
)]
pub struct ReviewPolicy {
    #[serde(default, deserialize_with = "present")]
    pub dependency_gates: Option<DependencyGates>,
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
    #[serde(default)]
    pub mounts: BTreeMap<MountAlias, ArtifactName>,
    #[serde(default, deserialize_with = "present")]
    pub basis: Option<bool>,
    #[serde(
        default = "default_fingerprint",
        deserialize_with = "declared_fingerprint"
    )]
    pub fingerprint: Option<Fingerprint>,
    #[serde(default, deserialize_with = "present")]
    pub review_policy: Option<ReviewPolicy>,
}

impl ArtifactDeclaration {
    fn validate(&self, location: &Location<'_, '_>) -> Result<(), String> {
        location
            .child("name")
            .check(identifier(&self.name, "Artifact name"))?;
        location.child("tags").check(tags(&self.tags))?;
        for eval in &self.evals {
            eval.validate(&location.child("evals").key(&eval.id))
                .map_err(|message| format!("Eval {}: {message}", eval.id))?;
        }
        if self.basis == Some(true) && !self.evals.is_empty() {
            return Err(location
                .child("basis")
                .error(format!("Basis Artifact {} cannot own Evals.", self.name)));
        }
        self.views.validate(&location.child("views"))?;
        for (alias, target) in &self.mounts {
            let mounts = location.child("mounts");
            mounts.key(alias).check(identifier(alias, "Mount alias"))?;
            mounts
                .child(alias)
                .check(identifier(target, "Mount target"))?;
        }
        if let Some(fingerprint) = &self.fingerprint {
            fingerprint.validate(&location.child("fingerprint"))?;
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
    let declaration =
        ArtifactDeclaration::deserialize(toml::de::ValueDeserializer::from(document.clone()))
            .map_err(|mut error| {
                error.set_input(Some(source));
                error.to_string()
            })?;
    declaration.validate(&location)?;
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
            .check(identifier(name, "Artifact name"))?;
    }
    if let Some(mounts) = document
        .get_ref()
        .get("mounts")
        .and_then(|value| value.get_ref().as_table())
    {
        for (alias, target) in mounts {
            location
                .child("mounts")
                .key(alias.get_ref())
                .check(identifier(alias.get_ref(), "Mount alias"))?;
            if let DeValue::String(target) = target.get_ref() {
                location
                    .child("mounts")
                    .child(alias.get_ref())
                    .check(identifier(target, "Mount target"))?;
            }
        }
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

/// Child paths or mount aliases with their target Artifacts as tool-side IDs. Declarations
/// validated both, so a failure here is a bug.
fn tool_ids<K: Ord, T: Ord + AsRef<str>>(
    targets: &BTreeMap<T, ArtifactName>,
    key: impl Fn(&str) -> Result<K, artifactize_tools::scope::ScopeError>,
) -> BTreeMap<K, artifactize_tools::scope::ArtifactId> {
    targets
        .iter()
        .map(|(path, name)| {
            (
                key(path.as_ref()).expect("validated child prefix or mount alias"),
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
    pub children: BTreeMap<ChildPrefix, ArtifactName>,
    pub name: ArtifactName,
    pub tags: Vec<String>,
    pub views: Views,
    pub mounts: BTreeMap<MountAlias, ArtifactName>,
    pub basis: Option<bool>,
    pub fingerprint: Option<Fingerprint>,
    pub review_policy: Option<ReviewPolicy>,
}

impl Artifact {
    pub(crate) fn tool_scope(&self) -> artifactize_tools::scope::Artifact {
        artifactize_tools::scope::Artifact {
            path: self.path.clone(),
            kind: self.kind,
            name: self.name.to_string(),
            children: tool_ids(&self.children, |path| {
                artifactize_tools::scope::ChildPrefix::new(path)
            }),
            mounts: tool_ids(&self.mounts, |alias| {
                artifactize_tools::scope::MountAlias::new(alias)
            }),
        }
    }

    /// Runtime commands and relative declarations use the containing folder.
    pub fn folder(&self) -> &Path {
        match self.kind {
            ArtifactKind::Folder => &self.path,
            ArtifactKind::File => self.path.parent().unwrap_or(Path::new("")),
        }
    }

    pub fn file_name(&self) -> Option<&str> {
        (self.kind == ArtifactKind::File)
            .then(|| self.path.file_name().and_then(|name| name.to_str()))
            .flatten()
    }

    pub fn declaration_path(&self) -> PathBuf {
        match self.kind {
            ArtifactKind::Folder => logical_join(&self.path, std::ffi::OsStr::new(CONFIG_FILE)),
            ArtifactKind::File => {
                let mut path = self.path.as_os_str().to_owned();
                path.push(".artf");
                PathBuf::from(path)
            }
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Eval {
    /// Workspace-qualified id; declaration.id remains the owner's local id.
    pub id: EvalId,
    pub target: ArtifactName,
    pub references: BTreeMap<String, ArtifactName>,
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
}

#[derive(Debug)]
pub struct RepoConfig {
    pub root: PathBuf,
    pub artifacts: BTreeMap<ArtifactName, Artifact>,
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

#[cfg(test)]
mod tests;

impl Serialize for EvalBody {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("profile", self.profile())?;
        match self {
            Self::Dependency { .. } => {
                map.serialize_entry("passSchema", &Option::<()>::None)?;
                map.serialize_entry("failSchema", &Option::<()>::None)?;
            }
            Self::Executable {
                profile_variants,
                payload,
                pass_schema,
                fail_schema,
                ..
            } => {
                if !profile_variants.is_empty() {
                    map.serialize_entry("profileVariants", profile_variants)?;
                }
                map.serialize_entry("payload", payload)?;
                map.serialize_entry("passSchema", pass_schema)?;
                map.serialize_entry("failSchema", fail_schema)?;
            }
        }
        map.end()
    }
}
impl EvalBody {
    fn profile(&self) -> &Profile {
        match self {
            Self::Dependency { profile } | Self::Executable { profile, .. } => profile,
        }
    }
}
