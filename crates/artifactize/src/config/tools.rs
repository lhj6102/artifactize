use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::validation::{paths, present, script, text, timeout};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AgentTool {
    Command(CommandTool),
    Builtin(BuiltinTool),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
        skip_serializing_if = "Option::is_none"
    )]
    pub timeout_ms: Option<u32>,
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
        if serde_json::to_vec(self).map_err(|e| e.to_string())?.len() > 8 * 1024 * 1024 {
            return Err("Tool declaration exceeds 8 MiB.".into());
        }
        match self {
            Self::Command(tool) => {
                description(&tool.description)?;
                script(&tool.command, &tool.args)?;
                if tool.command.contains(['{', '}']) {
                    return Err("Tool command must not contain placeholders.".into());
                }
                paths(&tool.execution_paths, "executionPaths")?;
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
            Self::Builtin(_) => serde_json::json!({"type":"object"}),
        }
    }
}

fn empty_schema() -> Value {
    serde_json::json!({"type":"object", "additionalProperties":false})
}

pub(super) fn description(value: &str) -> Result<(), String> {
    text(value, "Tool description")?;
    if value.encode_utf16().count() > 4000
        || value.replace("{artifactName}", "").contains(['{', '}'])
    {
        return Err(
            "Tool description is limited to 4000 characters and supports only {artifactName}."
                .into(),
        );
    }
    Ok(())
}
