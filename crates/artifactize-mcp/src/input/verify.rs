//! Verification argument envelopes and observation bounds.
use super::{Run, Scope, decode};
use crate::{error::ArgumentsError, tool::VerifyTool};
use artifactize_core::{
    runs::ResultCursor,
    types::{RequestKey, RunComment, RunId},
};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::{num::NonZeroU32, time::Duration};

/// Leave room under the host's 45-second deadline for transport serialization.
pub(crate) const MAX_WAIT_SECONDS: u64 = 40;
/// Observe for half a minute by default, reserving host-deadline headroom for routine calls.
pub(crate) const DEFAULT_WAIT_SECONDS: u64 = 30;

pub(crate) enum Verify {
    Start(Start),
    Status(RunId),
    Wait(Wait),
    Watch(RunId),
    Result(ResultInput),
    Cancel(RunId),
}

impl Verify {
    pub(super) fn parse(
        tool: VerifyTool,
        arguments: Map<String, Value>,
    ) -> Result<Self, ArgumentsError> {
        Ok(match tool {
            VerifyTool::Start => Self::Start(decode(arguments)?),
            VerifyTool::Status => Self::Status(decode::<Run>(arguments)?.run),
            VerifyTool::Wait => Self::Wait(decode(arguments)?),
            VerifyTool::Watch => Self::Watch(decode::<Run>(arguments)?.run),
            VerifyTool::Result => Self::Result(decode(arguments)?),
            VerifyTool::Cancel => Self::Cancel(decode::<Run>(arguments)?.run),
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Start {
    pub scope: Scope,
    pub request_key: Option<RequestKey>,
    pub comment: Option<RunComment>,
    pub jobs: Option<NonZeroU32>,
    #[serde(default)]
    pub speculate: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Wait {
    pub run: RunId,
    #[serde(rename = "seconds")]
    bound: Option<WaitBound>,
}

impl Wait {
    pub(crate) fn duration(&self) -> Duration {
        self.bound
            .as_ref()
            .map_or(Duration::from_secs(DEFAULT_WAIT_SECONDS), |bound| bound.0)
    }
}

#[derive(Deserialize)]
#[serde(try_from = "u64")]
struct WaitBound(Duration);

impl TryFrom<u64> for WaitBound {
    type Error = String;

    fn try_from(seconds: u64) -> Result<Self, Self::Error> {
        if seconds > MAX_WAIT_SECONDS {
            return Err(format!(
                "seconds must be an integer from 0 through {MAX_WAIT_SECONDS}."
            ));
        }
        Ok(Self(Duration::from_secs(seconds)))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResultInput {
    pub run: RunId,
    pub cursor: Option<ResultCursor>,
}
