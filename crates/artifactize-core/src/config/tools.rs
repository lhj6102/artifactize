use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    deny_unknown_fields,
    try_from = "CommandToolInput"
)]
pub struct CommandTool {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<super::ToolOperationName>,
    pub description: String,
    /// Shared with every declaration of an equal schema in the same read.
    #[serde(default = "empty_schema")]
    pub input_schema: Arc<Value>,
    pub protocol: ToolProtocol,
    #[serde(serialize_with = "crate::platform::serialize_program")]
    pub command: String,
    pub args: Vec<crate::scope::Argument>,
    #[serde(
        default,
        deserialize_with = "timeout",
        serialize_with = "super::validation::milliseconds::serialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub timeout_ms: Option<std::time::Duration>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub execution_paths: Vec<super::LogicalPath>,
    #[serde(skip)]
    pub(crate) plain_args: Vec<super::plain::PlainArgument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    rename_all(serialize = "camelCase", deserialize = "snake_case"),
    deny_unknown_fields
)]
struct CommandToolInput {
    #[serde(default)]
    pub session: Option<super::ToolOperationName>,
    pub description: String,
    #[serde(default = "empty_schema")]
    pub input_schema: Value,
    pub protocol: ToolProtocol,
    pub command: String,
    pub args: Vec<crate::scope::Argument>,
    #[serde(
        default,
        deserialize_with = "timeout",
        serialize_with = "super::validation::milliseconds::serialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub timeout_ms: Option<std::time::Duration>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub execution_paths: Vec<super::LogicalPath>,
}

impl TryFrom<CommandToolInput> for CommandTool {
    type Error = String;
    fn try_from(input: CommandToolInput) -> Result<Self, Self::Error> {
        let plain_args = if input.protocol == ToolProtocol::Plain {
            input
                .args
                .iter()
                .map(|arg| {
                    super::plain::PlainArgument::parse(&arg.to_string(), &input.input_schema)
                })
                .collect::<Result<_, _>>()?
        } else {
            Vec::new()
        };
        Ok(Self {
            session: input.session,
            description: input.description,
            input_schema: Arc::new(input.input_schema),
            protocol: input.protocol,
            command: input.command,
            args: input.args,
            timeout_ms: input.timeout_ms,
            execution_paths: input.execution_paths,
            plain_args,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolSession {
    pub start: SessionCommand,
    pub stop: SessionCommand,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    rename_all(serialize = "camelCase", deserialize = "snake_case"),
    deny_unknown_fields
)]
pub struct SessionCommand {
    #[serde(serialize_with = "crate::platform::serialize_program")]
    pub command: String,
    pub args: Vec<crate::scope::Argument>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub execution_paths: Vec<super::LogicalPath>,
}

impl SessionCommand {
    pub(super) fn validate(&self) -> Result<(), String> {
        script(&self.command, &self.args)?;
        if self.command.contains(['{', '}']) {
            return Err("Session command must not contain placeholders.".into());
        }
        paths(&self.execution_paths, "execution_paths")
    }
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
    /// Its description and input schema are bounded, as a provider receives them.
    pub(super) fn validate(&self, schemas: &mut super::schemas::Schemas) -> Result<(), String> {
        match self {
            Self::Command(tool) => {
                description(&tool.description)?;
                script(&tool.command, &tool.args)?;
                if tool.command.contains(['{', '}']) {
                    return Err("Tool command must not contain placeholders.".into());
                }
                paths(&tool.execution_paths, "execution_paths")?;
                schemas.check(&tool.input_schema, |schema| {
                    crate::tools::schema::compile(schema).map(drop)
                })
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
            Self::Command(tool) => Value::clone(&tool.input_schema),
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
    #[serde(serialize_with = "crate::platform::serialize_program")]
    pub command: String,
    #[serde(deserialize_with = "crate::scope::human_arguments")]
    pub args: Vec<crate::scope::Argument>,
    #[serde(
        default,
        deserialize_with = "timeout",
        serialize_with = "super::validation::milliseconds::serialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub timeout_ms: Option<std::time::Duration>,
}

#[derive(Debug, Clone)]
pub struct HumanBuiltinTool {
    pub builtin: Builtin,
    pub description: String,
    pub args: Vec<crate::scope::Argument>,
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
            #[serde(deserialize_with = "crate::scope::human_arguments")]
            args: Vec<crate::scope::Argument>,
        }
        let declaration = Declaration::deserialize(deserializer)?;
        let args: Vec<String> = declaration.args.iter().map(ToString::to_string).collect();
        artifactize_tools::builtin::validate_args(declaration.builtin, Some(&args), true)
            .map_err(D::Error::custom)?;
        let kind = if matches!(declaration.builtin, Builtin::Open | Builtin::Graph) {
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
                crate::tools::builtin::fixed_description(declaration.builtin, &args).into()
            }),
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
            Self::Builtin(tool) => tool.kind(),
        }
    }
    pub(super) fn validate(&self) -> Result<(), String> {
        description(self.description())?;
        match self {
            Self::Command(tool) => {
                script(&tool.command, &tool.args)?;
                if tool.command.contains(['{', '}']) {
                    return Err("Tool command must not contain placeholders.".into());
                }
                for argument in &tool.args {
                    crate::scope::validate_human_argument(argument)
                        .map_err(|error| error.to_string())?;
                }
            }
            Self::Builtin(tool) => {
                for argument in &tool.args {
                    crate::scope::validate_human_argument(argument)
                        .map_err(|error| error.to_string())?;
                }
                if !matches!(tool.builtin, Builtin::Help | Builtin::Graph)
                    && let crate::scope::Argument::Literal(target) = &tool.args[0]
                    && !(tool.builtin == Builtin::Open
                        && artifactize_tools::builtin::is_url(target))
                {
                    artifactize_tools::scope::logical_path(target)
                        .map_err(|error| error.to_string())?;
                }
                if tool.builtin == Builtin::Help
                    && tool
                        .args
                        .iter()
                        .any(|arg| matches!(arg, crate::scope::Argument::Reference { .. }))
                {
                    return Err(
                        "Help args are literal program/subcommand names, not path placeholders."
                            .into(),
                    );
                }
            }
        }
        Ok(())
    }
}

fn validate_agent_target(builtin: Builtin, args: &[String]) -> Result<(), String> {
    if builtin == Builtin::Graph {
        return Ok(());
    }
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

impl HumanBuiltinTool {
    pub fn kind(&self) -> HumanToolKind {
        if matches!(self.builtin, Builtin::Open | Builtin::Graph) {
            HumanToolKind::Launch
        } else {
            HumanToolKind::Output
        }
    }
}
impl Serialize for HumanBuiltinTool {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Stored<'a> {
            builtin: Builtin,
            description: &'a str,
            kind: HumanToolKind,
            args: &'a [crate::scope::Argument],
        }
        Stored {
            builtin: self.builtin,
            description: &self.description,
            kind: self.kind(),
            args: &self.args,
        }
        .serialize(serializer)
    }
}
