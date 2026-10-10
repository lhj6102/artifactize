//! Inert discovery and strict declarations.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use crate::types::{ArtifactName, EvalId};
use serde_json::{Map, Value};
use thiserror::Error;

mod location;
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

    /// Position cross-Artifact validation failures at their declaration's typed key.
    pub(crate) fn declaration(
        path: impl Into<PathBuf>,
        keys: &[&str],
        message: impl ToString,
    ) -> Self {
        let path = path.into();
        let message = message.to_string();
        let message = fs::read_to_string(&path).map_or(message.clone(), |source| {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum Profile {
    Agent {
        backend: Backend,
        model: String,
        #[serde(default, deserialize_with = "present")]
        reasoning: Option<String>,
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
        depends_on: Vec<String>,
    },
    Runtime {
        command: String,
        args: Vec<String>,
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
                        .check(backend.validate_reasoning(reasoning))?;
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
    pub profile: Profile,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub profile_variants: BTreeMap<String, Profile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<EvalPayload>,
    #[serde(default, deserialize_with = "present")]
    pub pass_schema: Option<Map<String, Value>>,
    #[serde(default, deserialize_with = "present")]
    pub fail_schema: Option<Map<String, Value>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
struct EvalFields {
    title: String,
    profile: Profile,
    #[serde(default, deserialize_with = "present")]
    profile_variants: Option<BTreeMap<String, Profile>>,
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
        Ok(Self {
            id,
            title: fields.title,
            profile: fields.profile,
            profile_variants: fields.profile_variants.unwrap_or_default(),
            payload: fields.payload,
            pass_schema: fields.pass_schema,
            fail_schema: fields.fail_schema,
        })
    }
}

impl EvalDeclaration {
    fn validate(&self, location: &Location<'_, '_>) -> Result<(), String> {
        location
            .child("title")
            .check(text(&self.title, "Eval title"))?;
        if let Some(payload) = &self.payload {
            location
                .child("payload")
                .child("instruction")
                .check(text(&payload.instruction, "Eval payload.instruction"))?;
        }
        self.profile.validate(&location.child("profile"))?;
        for (key, schema) in [
            ("pass_schema", &self.pass_schema),
            ("fail_schema", &self.fail_schema),
        ] {
            if let Some(schema) = schema {
                location
                    .child(key)
                    .check(crate::agent::verdict::validate_schema(schema))?;
            }
        }
        if self.profile_variants.len() > MAX_DECLARED_ITEMS {
            return Err(location
                .child("profile_variants")
                .error("profile_variants must contain at most 64 named profiles."));
        }
        for (name, profile) in &self.profile_variants {
            let variant = location.child("profile_variants").child(name);
            location
                .child("profile_variants")
                .key(name)
                .check(identifier(name, "Profile variant name"))?;
            profile.validate(&variant)?;
            if std::mem::discriminant(profile) != std::mem::discriminant(&self.profile) {
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
    pub agent_tools: BTreeMap<String, AgentTool>,
    #[serde(default)]
    pub human_tools: BTreeMap<String, HumanTool>,
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
        args: Vec<String>,
        /// Owner-relative paths that must exist on every call; never hashed.
        files: Vec<String>,
        timeout_ms: Option<std::time::Duration>,
    },
    Artifactsum {
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
#[serde(rename_all = "snake_case", deny_unknown_fields)]
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
struct ArtifactsumForm {
    #[serde(default = "owner_root")]
    files: Vec<String>,
    #[serde(default)]
    ignore: Vec<String>,
}

fn owner_root() -> Vec<String> {
    vec![".".into()]
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
    pub name: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, deserialize_with = "declared_evals")]
    pub evals: Vec<EvalDeclaration>,
    #[serde(default)]
    pub views: Views,
    #[serde(default)]
    pub mounts: BTreeMap<String, String>,
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
fn tool_ids<K: Ord>(
    targets: &BTreeMap<String, ArtifactName>,
    key: impl Fn(&str) -> Result<K, artifactize_tools::scope::ScopeError>,
) -> BTreeMap<K, artifactize_tools::scope::ArtifactId> {
    targets
        .iter()
        .map(|(path, name)| {
            (
                key(path).expect("validated child prefix or mount alias"),
                crate::scope::tool_id(name),
            )
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub path: PathBuf,
    pub kind: ArtifactKind,
    pub children: BTreeMap<String, ArtifactName>,
    pub name: ArtifactName,
    pub tags: Vec<String>,
    pub views: Views,
    pub mounts: BTreeMap<String, ArtifactName>,
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

/// Discover regular markers in deterministic path order, without following directory links.
pub fn read_workspace_config(repo: &Path) -> Result<RepoConfig, ConfigError> {
    let root =
        crate::platform::canonicalize(repo).map_err(|error| ConfigError::new(repo, error))?;
    let mut config = RepoConfig {
        root,
        artifacts: BTreeMap::new(),
        evals: Vec::new(),
        relations: Vec::new(),
    };
    let ignored = discovery_ignore(&config.root)?;
    let mut pending = vec![(PathBuf::new(), None::<String>)];
    while let Some((relative, mut owner)) = pending.pop() {
        let directory = config.root.join(&relative);
        let entries = fs::read_dir(&directory)
            .and_then(|entries| entries.collect::<Result<Vec<_>, _>>())
            .map_err(|error| ConfigError::new(&directory, error))?;
        let mut entries = entries;
        entries.sort_by_key(|entry| entry.file_name());
        if let Some(legacy) = entries
            .iter()
            .find(|entry| entry.file_name() == "artifactize.json")
        {
            let file = legacy.path();
            let kind = legacy
                .file_type()
                .map_err(|error| ConfigError::new(&file, error))?;
            if !ignored.matched(&file, kind.is_dir()).is_ignore() && !kind.is_dir() {
                return Err(ConfigError::new(
                    file,
                    "artifactize.json is no longer read; declare this Artifact in index.artf (TOML).",
                ));
            }
        }
        if let Some(marker) = entries
            .iter()
            .find(|entry| entry.file_name() == CONFIG_FILE)
        {
            let file = marker.path();
            let kind = marker
                .file_type()
                .map_err(|error| ConfigError::new(&file, error))?;
            if !kind.is_file() {
                return Err(ConfigError::new(file, "index.artf must be a regular file."));
            }
            let source =
                fs::read_to_string(&file).map_err(|error| ConfigError::new(&file, error))?;
            let declaration =
                parse_declaration(&source).map_err(|error| ConfigError::new(&file, error))?;
            if !relative.as_os_str().is_empty() && declaration.review_policy.is_some() {
                return Err(ConfigError::declaration(
                    &file,
                    &["review_policy"],
                    "review_policy belongs only to the repository root index.artf.",
                ));
            }
            let name = declaration.name.clone();
            if let Some(parent) = &owner {
                let parent = config.artifacts.get_mut(parent).unwrap();
                let child = relative.strip_prefix(&parent.path).unwrap();
                let child = child
                    .to_str()
                    .ok_or_else(|| ConfigError::new(&file, "Artifact paths must be UTF-8."))?;
                let name = name
                    .parse()
                    .map_err(|error: String| ConfigError::declaration(&file, &["name"], error))?;
                parent.children.insert(child.to_owned(), name);
            }
            insert_artifact(
                &mut config,
                relative.clone(),
                ArtifactKind::Folder,
                declaration,
                &file,
            )?;
            owner = Some(name);
        }
        for marker in &entries {
            if !ignored.matched(marker.path(), false).is_ignore()
                && marker
                    .file_name()
                    .as_encoded_bytes()
                    .ends_with(b".artf.artf")
            {
                return Err(ConfigError::new(
                    marker.path(),
                    "File Artifact target must not be a declaration (.artf).",
                ));
            }
        }
        for marker in &entries {
            if ignored.matched(marker.path(), false).is_ignore() {
                continue;
            }
            let filename = marker.file_name();
            if !filename.as_encoded_bytes().ends_with(b".artf") {
                continue;
            }
            let Some(filename) = filename.to_str() else {
                return Err(ConfigError::new(
                    marker.path(),
                    "Artifact paths must be UTF-8.",
                ));
            };
            let Some(target) = filename.strip_suffix(".artf") else {
                continue;
            };
            if filename == CONFIG_FILE {
                continue;
            }
            let file = marker.path();
            if !marker
                .file_type()
                .map_err(|error| ConfigError::new(&file, error))?
                .is_file()
            {
                return Err(ConfigError::new(
                    &file,
                    "File Artifact declaration must be a regular file.",
                ));
            }
            if target.is_empty() || target.ends_with(".artf") {
                return Err(ConfigError::new(
                    &file,
                    "File Artifact target must not be a declaration (.artf).",
                ));
            }
            path(target).map_err(|error| ConfigError::new(&file, error))?;
            if target.contains(':') {
                return Err(ConfigError::new(
                    &file,
                    "File Artifact target name must not contain ':'; scoped tool paths do not support colons.",
                ));
            }
            let target_path = directory.join(target);
            let metadata = fs::symlink_metadata(&target_path).map_err(|error| {
                ConfigError::new(
                    &file,
                    if error.kind() == std::io::ErrorKind::NotFound {
                        format!("File Artifact target {target} is missing.")
                    } else {
                        format!("Cannot inspect File Artifact target {target}: {error}.")
                    },
                )
            })?;
            if !metadata.is_file() || metadata.is_symlink() {
                return Err(ConfigError::new(
                    &file,
                    format!(
                        "File Artifact target {target} must be a regular file, not a symlink, directory or special file."
                    ),
                ));
            }
            let source =
                fs::read_to_string(&file).map_err(|error| ConfigError::new(&file, error))?;
            let mut declaration =
                parse_declaration(&source).map_err(|error| ConfigError::new(&file, error))?;
            if declaration.review_policy.is_some() {
                return Err(ConfigError::declaration(
                    &file,
                    &["review_policy"],
                    "review_policy belongs only to the repository root index.artf.",
                ));
            }
            let value = toml::de::DeTable::parse(&source)
                .map_err(|error| ConfigError::new(&file, error))?;
            if value.get_ref().get("fingerprint").is_none() {
                declaration.fingerprint = Some(Fingerprint::Artifactsum {
                    files: vec![target.into()],
                    ignore: Vec::new(),
                });
            }
            if let Some(Fingerprint::Artifactsum { files, .. }) = &declaration.fingerprint {
                if value
                    .get_ref()
                    .get("fingerprint")
                    .and_then(|value| value.get_ref().get("ignore"))
                    .is_some()
                {
                    return Err(ConfigError::declaration(
                        &file,
                        &["fingerprint", "ignore"],
                        "fingerprint.ignore is not supported for a file Artifact.",
                    ));
                }
                if files.iter().any(|input| input != target) {
                    return Err(ConfigError::declaration(
                        &file,
                        &["fingerprint", "files"],
                        format!(
                            "fingerprint.files for a file Artifact may name only its target {target}."
                        ),
                    ));
                }
            }
            insert_artifact(
                &mut config,
                logical_join(&relative, std::ffi::OsStr::new(target)),
                ArtifactKind::File,
                declaration,
                &file,
            )?;
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
                pending.push((logical_join(&relative, &entry.file_name()), owner.clone()));
            }
        }
    }
    if config.artifacts.is_empty() {
        return Err(ConfigError::new(
            &config.root,
            "Workspace must contain at least one .artf Artifact.",
        ));
    }
    crate::scope::resolve_config(&mut config)?;
    Ok(config)
}

fn insert_artifact(
    config: &mut RepoConfig,
    path: PathBuf,
    kind: ArtifactKind,
    declaration: ArtifactDeclaration,
    file: &Path,
) -> Result<(), ConfigError> {
    let ArtifactDeclaration {
        name,
        tags,
        evals,
        views,
        mounts,
        basis,
        fingerprint,
        review_policy,
    } = declaration;
    let name: ArtifactName = name
        .parse()
        .map_err(|error: String| ConfigError::declaration(file, &["name"], error))?;
    if config.artifacts.contains_key(&name) {
        return Err(ConfigError::declaration(
            file,
            &["name"],
            format!("Duplicate Artifact name: {name}."),
        ));
    }
    for declaration in evals {
        config.evals.push(Eval {
            id: format!("{name}/{}", declaration.id)
                .parse()
                .expect("an Artifact name and a local Eval id form an Eval id"),
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
            path,
            kind,
            children: BTreeMap::new(),
            name,
            tags,
            views,
            mounts: mounts
                .into_iter()
                .map(|(alias, target)| {
                    target
                        .parse()
                        .map(|target| (alias, target))
                        .map_err(|error: String| ConfigError::declaration(file, &["mounts"], error))
                })
                .collect::<Result<_, _>>()?,
            basis,
            fingerprint,
            review_policy,
        },
    );
    Ok(())
}

/// The folders the workspace root's `.artfignore` (gitignore syntax) keeps out of
/// discovery, such as test fixtures or example projects with their own `index.artf`.
fn discovery_ignore(root: &Path) -> Result<ignore::gitignore::Gitignore, ConfigError> {
    let legacy = root.join(".artifactizeignore");
    if fs::symlink_metadata(&legacy).is_ok() {
        return Err(ConfigError::new(
            legacy,
            ".artifactizeignore was renamed to .artfignore.",
        ));
    }
    let file = root.join(IGNORE_FILE);
    let mut builder = ignore::gitignore::GitignoreBuilder::new(root);
    builder
        .case_insensitive(false)
        .expect("case-sensitive discovery");
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
