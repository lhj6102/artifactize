//! Inspection argument envelopes and paging inputs.
use super::{Run, Scope, decode};
use crate::verify_tools::{error::ArgumentsError, page, tool::ReadTool};
use crate::{
    config::{Backend, ToolName},
    diagnostics::{self, ToolCheckOptions},
    inspect::{LogOptions, LogQuery, session},
    types::{ArtifactName, EvalId, RequestId, ReuseKey, RunId},
};
use serde::Deserialize;
use serde_json::{Map, Value};
use time::OffsetDateTime;

/// Limit each Run-list query to an agreed maximum record group; offset selects older groups.
pub(crate) const RUN_LIST_LIMIT: u32 = 20;

pub(crate) struct Inspection {
    pub read: Read,
    pub page: page::Query,
}

pub(crate) enum Read {
    Status(Status),
    Graph(GraphInput),
    Logs(LogInput),
    Config,
    Runs(Runs),
    Run(RunId),
    Requests(Requests),
    Request(RequestId),
    Session(Session),
    Tools(ToolCheckOptions),
    Doctor,
    Models(Backend),
    Remote,
    Cache(Cache),
    Plugins,
}

impl Inspection {
    pub(super) fn parse(
        tool: ReadTool,
        mut arguments: Map<String, Value>,
        as_of: OffsetDateTime,
    ) -> Result<Self, ArgumentsError> {
        let cursor = arguments
            .remove("cursor")
            .map(|value| {
                value
                    .as_str()
                    .ok_or(ArgumentsError::CursorType)?
                    .parse()
                    .map_err(ArgumentsError::Cursor)
            })
            .transpose()?;
        let key = page::QueryKey::new(
            &serde_json::to_vec(&(tool.name(), &arguments)).expect("JSON tool arguments serialize"),
        );
        let read = match tool {
            ReadTool::Status => Read::Status(decode(arguments)?),
            ReadTool::Graph => Read::Graph(decode(arguments)?),
            ReadTool::Logs => {
                let LogWire { id, options } = decode(arguments)?;
                let query = options.try_into().map_err(ArgumentsError::LogOptions)?;
                Read::Logs(LogInput { id, query })
            }
            ReadTool::Config => empty(arguments, Read::Config)?,
            ReadTool::Runs => Read::Runs(decode(arguments)?),
            ReadTool::Run => Read::Run(decode::<Run>(arguments)?.run),
            ReadTool::Requests => Read::Requests(decode(arguments)?),
            ReadTool::Request => Read::Request(decode::<Request>(arguments)?.id),
            ReadTool::Session => Read::Session(decode(arguments)?),
            ReadTool::Tools => Read::Tools(
                decode::<Tools>(arguments)?
                    .options()
                    .map_err(ArgumentsError::ToolOptions)?,
            ),
            ReadTool::Doctor => empty(arguments, Read::Doctor)?,
            ReadTool::Models => Read::Models(decode::<Models>(arguments)?.provider),
            ReadTool::Remote => empty(arguments, Read::Remote)?,
            ReadTool::Cache => Read::Cache(decode(arguments)?),
            ReadTool::Plugins => empty(arguments, Read::Plugins)?,
        };
        Ok(Self {
            read,
            page: page::Query::new(key, cursor, as_of),
        })
    }
}

fn empty(arguments: Map<String, Value>, command: Read) -> Result<Read, ArgumentsError> {
    let _: Empty = decode(arguments)?;
    Ok(command)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Status {
    #[serde(default)]
    pub scope: Scope,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Runs {
    limit: Option<RunLimit>,
    pub offset: Option<u32>,
}

impl Runs {
    pub(crate) fn limit(&self) -> u32 {
        self.limit.as_ref().map_or(RUN_LIST_LIMIT, |limit| limit.0)
    }
}

#[derive(Deserialize)]
#[serde(try_from = "u32")]
struct RunLimit(u32);

impl TryFrom<u32> for RunLimit {
    type Error = String;

    fn try_from(limit: u32) -> Result<Self, Self::Error> {
        if limit > RUN_LIST_LIMIT {
            return Err(format!(
                "limit must be at most {RUN_LIST_LIMIT}; use offset for older Runs."
            ));
        }
        Ok(Self(limit))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Requests {
    pub run: Option<RunId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    id: RequestId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Session {
    pub reference: session::Selector,
    #[serde(default)]
    pub summary: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Tools {
    eval: Option<EvalId>,
    artifact: Option<ArtifactName>,
    tool: Option<ToolName>,
    audience: Option<Audience>,
}

impl Tools {
    fn options(self) -> Result<ToolCheckOptions, String> {
        ToolCheckOptions::from_flags(
            self.eval,
            self.artifact,
            self.audience.map(Into::into),
            self.tool,
            false,
            None,
        )
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Audience {
    Agent,
    Human,
}

impl From<Audience> for diagnostics::Audience {
    fn from(value: Audience) -> Self {
        match value {
            Audience::Agent => Self::Agent,
            Audience::Human => Self::Human,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Models {
    provider: Backend,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Cache {
    pub key: ReuseKey,
    #[serde(default)]
    pub history: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GraphInput {
    #[serde(default)]
    pub scope: Scope,
    #[serde(default)]
    pub impact: bool,
}

#[derive(Deserialize)]
struct LogWire {
    id: RequestId,
    #[serde(flatten)]
    options: LogOptions,
}

pub(crate) struct LogInput {
    pub id: RequestId,
    pub query: LogQuery,
}
