//! Artifact declarations, their fingerprint inputs, and TOML parsing with source locations.

use super::{
    AgentTool, ArtifactName, BuiltinTool, CommandTool, EvalDeclaration, HumanTool, LogicalPath,
    MAX_DECLARED_ITEMS, ToolOperationName, ToolSession, artifact_name,
    eval::declared_evals,
    identifier,
    location::{self, Location},
    schemas,
    validation::{path, paths, present, script, tags, text, timeout},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

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
    /// Artifacts this one is made of besides those its folder holds.
    #[serde(default, deserialize_with = "present")]
    pub parts: Option<Vec<ArtifactName>>,
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
            eval.share(schemas);
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
        if let Some(parts) = &self.parts {
            let parts_location = location.child("parts");
            if parts.is_empty()
                || parts.len() > MAX_DECLARED_ITEMS
                || parts.iter().collect::<BTreeSet<_>>().len() != parts.len()
            {
                return Err(parts_location.error("parts must name 1–64 unique Artifacts."));
            }
            if parts.contains(&self.name) {
                return Err(
                    parts_location.error(format!("{} cannot be a part of itself.", self.name))
                );
            }
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
            }
        }
        Ok(())
    }
}

/// Validate an Artifact declaration without opening scripts or declared inputs.
pub fn parse_declaration(source: &str) -> Result<ArtifactDeclaration, String> {
    let table = toml::de::DeTable::parse(source).map_err(|error| error.to_string())?;
    parse_declaration_table(source, &mut schemas::Schemas::default(), None, table)
}

pub(super) fn parse_declaration_table(
    source: &str,
    schemas: &mut schemas::Schemas,
    name: Option<&ArtifactName>,
    mut table: toml::Spanned<toml::de::DeTable<'_>>,
) -> Result<ArtifactDeclaration, String> {
    if let Some(name) = name {
        table.get_mut().remove("path");
        table.get_mut().remove("annotations");
        table.get_mut().insert(
            toml::Spanned::new(0..0, std::borrow::Cow::Borrowed("name")),
            toml::Spanned::new(
                0..0,
                toml::de::DeValue::String(std::borrow::Cow::Owned(name.to_string())),
            ),
        );
    }
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
