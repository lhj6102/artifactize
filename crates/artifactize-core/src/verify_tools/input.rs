//! Parse MCP arguments once at the transport boundary.
mod inspect;
mod verify;

pub(super) use inspect::{Inspection, RUN_LIST_LIMIT, Read};
pub(super) use verify::{DEFAULT_WAIT_SECONDS, MAX_WAIT_SECONDS, Verify};

use super::{
    error::ArgumentsError,
    tool::{PluginTool, ToolId},
};
use crate::{platform, project::selection::Selection, types::RunId};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Map, Value};
use std::path::PathBuf;
use time::OffsetDateTime;

/// Require at least one ID so a list selector always names a target.
pub(super) const MIN_SCOPE_IDS: usize = 1;
/// Bound model-driven array envelopes without changing core selection expansion.
pub(super) const SCOPE_IDS: usize = 64;

pub struct Call {
    pub project: Option<PathBuf>,
    pub(super) command: Command,
}

pub(super) enum Command {
    Verify(Verify),
    Inspect(Inspection),
    Plugin(Plugin),
}

impl Call {
    pub fn parse(
        name: &str,
        mut arguments: Map<String, Value>,
        as_of: OffsetDateTime,
    ) -> Result<Self, ArgumentsError> {
        let project = match arguments.remove("project") {
            None => None,
            Some(Value::String(folder)) => {
                Some(platform::absolute_folder(&folder).ok_or(ArgumentsError::ProjectRelative)?)
            }
            Some(_) => return Err(ArgumentsError::ProjectType),
        };
        let id = ToolId::parse(name).ok_or_else(|| ArgumentsError::UnknownTool(name.to_owned()))?;
        let command = match id {
            ToolId::Verify(tool) => Command::Verify(Verify::parse(tool, arguments)?),
            ToolId::Read(tool) => Command::Inspect(Inspection::parse(tool, arguments, as_of)?),
            ToolId::Plugin(PluginTool::Add) => Command::Plugin(Plugin::Add(decode(arguments)?)),
            ToolId::Plugin(PluginTool::Remove) => {
                Command::Plugin(Plugin::Remove(decode(arguments)?))
            }
        };
        Ok(Self { project, command })
    }
}

pub(super) enum Plugin {
    Add(PluginAdd),
    Remove(PluginRemove),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PluginAdd {
    pub path: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PluginRemove {
    #[serde(rename = "type")]
    pub name: crate::config::ArtifactType,
}

fn decode<T: DeserializeOwned>(arguments: Map<String, Value>) -> Result<T, ArgumentsError> {
    serde_json::from_value(Value::Object(arguments)).map_err(ArgumentsError::Decode)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Run {
    run: RunId,
}

#[derive(Deserialize)]
#[serde(try_from = "Selection")]
pub(super) struct Scope(Selection);

impl Scope {
    fn new(selection: Selection) -> Result<Self, String> {
        let count = match &selection {
            Selection::Artifacts { artifact_ids } => Some(artifact_ids.len()),
            Selection::Evals { eval_ids } => Some(eval_ids.len()),
            _ => None,
        };
        if count.is_some_and(|count| !(MIN_SCOPE_IDS..=SCOPE_IDS).contains(&count)) {
            return Err(format!(
                "scope arrays must contain {MIN_SCOPE_IDS}–{SCOPE_IDS} IDs"
            ));
        }
        Ok(Self(selection))
    }

    pub(super) fn selection(&self) -> &Selection {
        &self.0
    }
}

impl TryFrom<Selection> for Scope {
    type Error = String;

    fn try_from(selection: Selection) -> Result<Self, Self::Error> {
        Self::new(selection)
    }
}

impl Default for Scope {
    fn default() -> Self {
        Self(Selection::All)
    }
}
