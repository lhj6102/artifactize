//! What an Artifact's evals may use (its composed tools) and what its reviews depend on (its
//! fingerprint), each with the checks that hold across its fields.

use super::{
    AgentTool, HumanTool, LogicalPath, MAX_DECLARED_ITEMS, ToolFqn, ToolSession, schemas,
    validation::{path, paths, script, timeout},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

/// An Artifact's tools by audience, each named by its FQN (`files.read`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(
    rename_all(serialize = "camelCase", deserialize = "snake_case"),
    deny_unknown_fields
)]
pub struct Views {
    #[serde(default)]
    pub agent_sessions: BTreeMap<ToolFqn, ToolSession>,
    #[serde(default)]
    pub agent_tools: BTreeMap<ToolFqn, AgentTool>,
    #[serde(default)]
    pub human_tools: BTreeMap<ToolFqn, HumanTool>,
}

impl Views {
    /// Every tool bounded as a provider or a person receives it, and every session a command
    /// tool joins declared beside it. A failure names the table and tool it concerns.
    pub(super) fn check(
        &self,
        schemas: &mut schemas::Schemas,
    ) -> Result<(), (&'static str, ToolFqn, String)> {
        for (name, session) in &self.agent_sessions {
            session
                .start
                .validate()
                .and_then(|()| session.stop.validate())
                .map_err(|message| ("agent_sessions", name.clone(), message))?;
        }
        for (name, tool) in &self.agent_tools {
            if let AgentTool::Command(command) = tool
                && let Some(session) = &command.session
                && !self.agent_sessions.contains_key(session)
            {
                return Err((
                    "agent_tools",
                    name.clone(),
                    format!("Agent tool {name}: unknown tool session {session}."),
                ));
            }
            tool.validate(schemas).map_err(|message| {
                (
                    "agent_tools",
                    name.clone(),
                    format!("Tool {name}: {message}"),
                )
            })?;
        }
        for (name, tool) in &self.human_tools {
            tool.validate().map_err(|message| {
                (
                    "human_tools",
                    name.clone(),
                    format!("Tool {name}: {message}"),
                )
            })?;
        }
        Ok(())
    }

    /// Keep each input schema once in `schemas`, shared with the equal ones already read.
    pub(super) fn share(&mut self, schemas: &mut schemas::Schemas) {
        for tool in self.agent_tools.values_mut() {
            if let AgentTool::Command(tool) = tool {
                tool.input_schema = schemas.tool(&tool.input_schema);
            }
        }
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

const REMOVED_DEPENDENCIES: &str = "fingerprint.dependencies was removed in 0.5: a fingerprint covers only its own Artifact, and an eval's reuse key adds the fingerprints of its children and referenced Artifacts. Remove the field.";

impl From<ScriptForm> for Fingerprint {
    fn from(form: ScriptForm) -> Self {
        let ScriptFields {
            command,
            args,
            files,
            timeout_ms,
        } = form.script;
        Self::Script {
            command,
            args,
            files,
            timeout_ms,
        }
    }
}

impl From<ArtifactsumForm> for Fingerprint {
    fn from(form: ArtifactsumForm) -> Self {
        let ArtifactsumForm { files, ignore } = form;
        Self::Artifactsum { files, ignore }
    }
}

impl TryFrom<Map<String, Value>> for Fingerprint {
    type Error = serde_json::Error;

    fn try_from(object: Map<String, Value>) -> Result<Self, Self::Error> {
        Ok(if object.contains_key("script") {
            serde_json::from_value::<ScriptForm>(Value::Object(object))?.into()
        } else {
            if object.contains_key("dependencies") {
                return Err(serde::de::Error::custom(REMOVED_DEPENDENCIES));
            }
            serde_json::from_value::<ArtifactsumForm>(Value::Object(object))?.into()
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

    /// Its own bounds: a script's command and files, or 1–64 unique artifactsum files.
    pub(super) fn check(&self) -> Result<(), String> {
        match self {
            Self::Graph => Ok(()),
            Self::Script {
                command,
                args,
                files,
                ..
            } => {
                script(command, args)?;
                paths(files, "fingerprint.script.files")
            }
            Self::Artifactsum { files, .. } => {
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
                Ok(())
            }
        }
    }
}
