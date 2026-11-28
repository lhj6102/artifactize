//! Protocol-neutral implementation of the public Verify and inspection tools.
mod catalog;
mod error;
mod input;
mod page;
mod read;
mod tool;

pub use catalog::{Hints, Tool, tools};
pub use error::{ArgumentsError, Error};
pub use input::Call;
pub use page::{CursorError, Page};

use std::{future::Future, time::Duration};

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::{project::VerifyOptions, runs, types::RunId};
use input::{Command, DEFAULT_WAIT_SECONDS, Plugin, Verify};

/// Includes a zero-bound initial read and failure recovery; no MCP call reaches 45 s.
const WAIT_CALL_LIMIT: Duration = Duration::from_secs(44);

#[derive(Serialize)]
#[serde(untagged)]
pub enum Output {
    Observation(runs::RunObservation),
    Result(Box<runs::RunResultPage>),
    Cancellation(runs::CancelAcknowledgement),
    Inspection(Page),
    Plugin(PluginResult),
}

#[derive(Serialize)]
#[serde(tag = "action", rename_all = "lowercase")]
pub enum PluginResult {
    Added(crate::config::plugins::PluginInfo),
    Removed {
        #[serde(rename = "namespace")]
        name: crate::config::PluginName,
    },
}

/// Chat does not expose project-level plugin installation or removal.
pub(crate) fn chat_tool(name: &str) -> bool {
    matches!(
        tool::ToolId::parse(name),
        Some(tool::ToolId::Verify(_) | tool::ToolId::Read(_))
    )
}

impl Call {
    /// Execute the same operation in an MCP transport or an in-process Chat session.
    /// Transports resolve their own scope before invoking this; a Chat scopes it to one worktree.
    pub async fn execute<F: Future<Output = Result<(), String>>>(
        self,
        context: &runs::Context,
        cancel: CancellationToken,
        progress: impl FnMut(runs::RunEvent) -> F,
    ) -> Result<Output, Error> {
        let command = match self.command {
            Command::Verify(command) => command,
            Command::Plugin(plugin) => {
                use crate::config::plugins;
                let result = match plugin {
                    Plugin::Add(args) => PluginResult::Added(
                        plugins::add(context.repo(), &args.path).map_err(Error::Config)?,
                    ),
                    Plugin::Remove(args) => {
                        plugins::remove(context.repo(), &args.name).map_err(Error::Config)?;
                        PluginResult::Removed { name: args.name }
                    }
                };
                return Ok(Output::Plugin(result));
            }
            Command::Inspect(inspection) => {
                return Ok(Output::Inspection(
                    read::inspect(context, inspection).await?,
                ));
            }
        };
        Ok(match command {
            Verify::Start(args) => {
                let options = VerifyOptions {
                    jobs: args.jobs.map(|limit| limit.get() as usize),
                    modes: runs::RunModes {
                        speculate: args.speculate,
                    },
                    comment: args.comment,
                    ..Default::default()
                };
                Output::Observation(
                    runs::start_with_args_hash(
                        context,
                        args.scope.selection(),
                        &options,
                        args.request_key,
                        args.request_args_hash,
                    )
                    .await?,
                )
            }
            Verify::Status(run) => Output::Observation(runs::status(context, &run).await?),
            Verify::Wait(args) => Output::Observation(
                tokio::time::timeout(
                    WAIT_CALL_LIMIT,
                    runs::wait(context, &args.run, Some(args.duration()), cancel),
                )
                .await
                .map_err(|_| Error::Timeout)??,
            ),
            Verify::Watch(run) => Output::Observation(
                tokio::time::timeout(
                    WAIT_CALL_LIMIT,
                    runs::watch(
                        context,
                        &run,
                        Some(Duration::from_secs(DEFAULT_WAIT_SECONDS)),
                        cancel,
                        progress,
                    ),
                )
                .await
                .map_err(|_| Error::Timeout)??,
            ),
            Verify::Result(args) => Output::Result(Box::new(
                runs::result(context, &args.run, args.cursor).await?,
            )),
            Verify::Cancel(run) => Output::Cancellation(runs::cancel(context, &run).await?),
        })
    }

    /// Whether success reports the identity of the Run this tool started, not an inspected Run.
    pub fn started_run(&self) -> bool {
        matches!(self.command, Command::Verify(Verify::Start(_)))
    }

    /// Remap an ACP agent-supplied `verify_start` `request_key` (validated from its original
    /// unredacted value before SHA-256 hashing so a valid key containing a known secret still
    /// maps cleanly into the disjoint `chat/<sha256[..32]>` namespace) and attach the
    /// precomputed canonical `request_args_hash`.
    pub(crate) fn remap_acp_chat_key(
        &mut self,
        session_id: &crate::types::SessionId,
        send: usize,
        raw_agent_key: Option<String>,
        args_hash: String,
    ) -> Result<(), String> {
        if let Command::Verify(Verify::Start(args)) = &mut self.command {
            let agent_key = match raw_agent_key {
                Some(raw) => Some(raw.parse::<crate::types::RequestKey>()?),
                None => args.request_key.take(),
            };
            if let Some(agent_key) = agent_key {
                args.request_key = Some(crate::acp::verify_bridge::internal_chat_request_key(
                    session_id,
                    send,
                    agent_key.as_str(),
                )?);
                args.request_args_hash = Some(args_hash);
            }
        }
        Ok(())
    }

    /// Format only the validated, typed fields of this call for public display in
    /// `Block::Tool.arguments`, never forwarding raw JSON payloads (`C-ACP-EDGE-ISOLATION`).
    pub(crate) fn display_arguments(&self) -> String {
        use input::Read;
        use serde_json::json;

        let value = match &self.command {
            Command::Verify(Verify::Start(args)) => json!({
                "scope": args.scope.selection(),
                "speculate": args.speculate,
                "jobs": args.jobs,
                "comment": args.comment,
            }),
            Command::Verify(Verify::Status(run) | Verify::Watch(run) | Verify::Cancel(run)) => {
                json!({ "run": run })
            }
            Command::Verify(Verify::Wait(args)) => json!({
                "run": args.run,
                "seconds": args.duration().as_secs(),
            }),
            Command::Verify(Verify::Result(args)) => json!({ "run": args.run }),
            Command::Inspect(inspection) => match &inspection.read {
                Read::Status(status) => json!({ "scope": status.scope.selection() }),
                Read::Run(run) => json!({ "run": run }),
                Read::Request(id) => json!({ "id": id }),
                Read::Models(provider) => json!({ "provider": provider }),
                Read::Runs(runs) => json!({ "limit": runs.limit(), "offset": runs.offset }),
                _ => json!({}),
            },
            Command::Plugin(_) => json!({}),
        };
        value.to_string()
    }
}

impl Output {
    pub fn observed_run(&self) -> Option<&RunId> {
        match self {
            Self::Observation(observation) => Some(&observation.run),
            _ => None,
        }
    }
}
