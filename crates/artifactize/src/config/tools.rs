use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Bound serialized declarations before validating or sending tool schemas to a provider.
const MAX_DECLARATION_BYTES: usize = 8 * 1024 * 1024;
/// Keep descriptions usable in provider tool listings, measured as JSON client UTF-16 units.
const MAX_DESCRIPTION_CHARS: usize = 4000;

use super::validation::{paths, present, script, text, timeout};

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum AgentTool {
    Command(CommandTool),
    Builtin(BuiltinTool),
}

impl<'de> Deserialize<'de> for AgentTool {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let value = Value::deserialize(deserializer)?;
        if value.get("builtin").is_some() {
            serde_json::from_value(value).map(Self::Builtin)
        } else {
            serde_json::from_value(value).map(Self::Command)
        }
        .map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    rename_all(serialize = "camelCase", deserialize = "snake_case"),
    deny_unknown_fields
)]
pub struct CommandTool {
    pub description: String,
    #[serde(default = "empty_schema")]
    pub input_schema: Value,
    pub protocol: ToolProtocol,
    pub command: String,
    pub args: Vec<String>,
    #[serde(
        default,
        deserialize_with = "timeout",
        serialize_with = "super::validation::milliseconds::serialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub timeout_ms: Option<std::time::Duration>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub execution_paths: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolProtocol {
    Json,
    Plain,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuiltinTool {
    pub builtin: Builtin,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Builtin {
    Read,
    List,
    Glob,
    Grep,
    ViewImage,
}

impl AgentTool {
    pub(super) fn validate(&self) -> Result<(), String> {
        if serde_json::to_vec(self).map_err(|e| e.to_string())?.len() > MAX_DECLARATION_BYTES {
            return Err("Tool declaration exceeds 8 MiB.".into());
        }
        match self {
            Self::Command(tool) => {
                description(&tool.description)?;
                script(&tool.command, &tool.args)?;
                if tool.command.contains(['{', '}']) {
                    return Err("Tool command must not contain placeholders.".into());
                }
                paths(&tool.execution_paths, "execution_paths")?;
                crate::tools::schema::compile(&tool.input_schema)?;
                if tool.protocol == ToolProtocol::Plain {
                    for arg in &tool.args {
                        crate::tools::plain_argument(arg, |name| {
                            if tool.input_schema["properties"].get(name).is_none() {
                                return Err(format!("Unknown plain tool placeholder: {name}"));
                            }
                            Ok(String::new())
                        })?;
                    }
                }
                Ok(())
            }
            Self::Builtin(tool) => {
                if let Some(value) = &tool.description {
                    description(value)?;
                }
                Ok(())
            }
        }
    }

    pub fn input_schema(&self) -> Value {
        match self {
            Self::Command(tool) => tool.input_schema.clone(),
            Self::Builtin(tool) => crate::tools::builtin::input_schema(tool.builtin),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    rename_all(serialize = "camelCase", deserialize = "snake_case"),
    deny_unknown_fields
)]
pub struct HumanTool {
    pub description: String,
    pub kind: HumanToolKind,
    pub command: String,
    pub args: Vec<String>,
    #[serde(
        default,
        deserialize_with = "timeout",
        serialize_with = "super::validation::milliseconds::serialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub timeout_ms: Option<std::time::Duration>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HumanToolKind {
    Launch,
    Output,
}

impl HumanTool {
    pub(super) fn validate(&self) -> Result<(), String> {
        description(&self.description)?;
        script(&self.command, &self.args)?;
        if self.command.contains(['{', '}']) {
            return Err("Tool command must not contain placeholders.".into());
        }
        crate::scope::validate_human_args(&self.args).map_err(|e| e.to_string())
    }
}

fn empty_schema() -> Value {
    serde_json::json!({"type":"object", "additionalProperties":false})
}

pub(super) fn description(value: &str) -> Result<(), String> {
    text(value, "Tool description")?;
    if value.encode_utf16().count() > MAX_DESCRIPTION_CHARS
        || value.replace("{artifactName}", "").contains(['{', '}'])
    {
        return Err(
            "Tool description is limited to 4000 characters and supports only {artifactName}."
                .into(),
        );
    }
    Ok(())
}
