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
    pub args: Option<Vec<String>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub description: Option<String>,
}

pub use artifactize_tools::Builtin;

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
                artifactize_tools::builtin::validate_args(
                    tool.builtin,
                    tool.args.as_deref(),
                    false,
                )?;
                if let Some(args) = &tool.args {
                    validate_agent_target(tool.builtin, args)?;
                }
                Ok(())
            }
        }
    }
    pub fn input_schema(&self) -> Value {
        match self {
            Self::Command(tool) => tool.input_schema.clone(),
            Self::Builtin(tool) => match &tool.args {
                Some(args) => crate::tools::builtin::fixed_schema(tool.builtin, args),
                None => crate::tools::builtin::input_schema(tool.builtin),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum HumanTool {
    Command(HumanCommandTool),
    Builtin(HumanBuiltinTool),
}

impl<'de> Deserialize<'de> for HumanTool {
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
pub struct HumanCommandTool {
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

#[derive(Debug, Clone, Serialize)]
pub struct HumanBuiltinTool {
    pub builtin: Builtin,
    pub description: String,
    pub kind: HumanToolKind,
    pub args: Vec<String>,
}

impl<'de> Deserialize<'de> for HumanBuiltinTool {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Declaration {
            builtin: Builtin,
            #[serde(default, deserialize_with = "present")]
            description: Option<String>,
            #[serde(default, deserialize_with = "present")]
            kind: Option<HumanToolKind>,
            args: Vec<String>,
        }
        let declaration = Declaration::deserialize(deserializer)?;
        artifactize_tools::builtin::validate_args(
            declaration.builtin,
            Some(&declaration.args),
            true,
        )
        .map_err(D::Error::custom)?;
        let kind = if declaration.builtin == Builtin::Open {
            HumanToolKind::Launch
        } else {
            HumanToolKind::Output
        };
        if declaration.kind.is_some_and(|declared| declared != kind) {
            return Err(D::Error::custom(
                "Human builtin kind disagrees with its tool.",
            ));
        }
        Ok(Self {
            builtin: declaration.builtin,
            description: declaration.description.unwrap_or_else(|| {
                crate::tools::builtin::fixed_description(declaration.builtin, &declaration.args)
                    .into()
            }),
            kind,
            args: declaration.args,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HumanToolKind {
    Launch,
    Output,
}

impl HumanTool {
    pub fn description(&self) -> &str {
        match self {
            Self::Command(tool) => &tool.description,
            Self::Builtin(tool) => &tool.description,
        }
    }
    pub fn kind(&self) -> HumanToolKind {
        match self {
            Self::Command(tool) => tool.kind,
            Self::Builtin(tool) => tool.kind,
        }
    }
    pub fn args(&self) -> &[String] {
        match self {
            Self::Command(tool) => &tool.args,
            Self::Builtin(tool) => &tool.args,
        }
    }
    pub(super) fn validate(&self) -> Result<(), String> {
        description(self.description())?;
        if let Self::Command(tool) = self {
            script(&tool.command, &tool.args)?;
            if tool.command.contains(['{', '}']) {
                return Err("Tool command must not contain placeholders.".into());
            }
        }
        crate::scope::validate_human_args(self.args()).map_err(|error| error.to_string())?;
        if let Self::Builtin(tool) = self {
            if tool.builtin != Builtin::Help
                && !(tool.builtin == Builtin::Open
                    && artifactize_tools::builtin::is_url(&tool.args[0]))
                && !tool.args[0].starts_with('{')
            {
                artifactize_tools::scope::logical_path(&tool.args[0])
                    .map_err(|error| error.to_string())?;
            }
            if tool.builtin == Builtin::Help && tool.args.iter().any(|arg| arg.contains(['{', '}']))
            {
                return Err(
                    "Help args are literal program/subcommand names, not path placeholders.".into(),
                );
            }
        }
        Ok(())
    }
}

fn validate_agent_target(builtin: Builtin, args: &[String]) -> Result<(), String> {
    if builtin == Builtin::Help {
        if args.iter().any(|arg| arg.contains(['{', '}'])) {
            return Err("Help args are literal program/subcommand names.".into());
        }
        return Ok(());
    }
    artifactize_tools::scope::logical_path(&args[0]).map_err(|error| error.to_string())
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
