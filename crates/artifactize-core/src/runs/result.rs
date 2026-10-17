//! Bounded pages derived from a saved Run/request/execution read snapshot, never a second store.
use super::{Context, State, observe};
use crate::types::{EvalId, RequestId, RequestStatus, RunId, Sha256Digest};
use serde::{Deserialize, Serialize, ser::SerializeMap};
/// Compact findings stay useful within MCP's response bound even with long owner summaries.
const SUMMARY_CHARS: usize = 512;
/// Shared page size; interfaces cannot accidentally emit an unbounded findings list.
const PAGE_SIZE: usize = 20;
/// Leave room for the envelope and MCP's structured-plus-text mirror under 64 KiB.
const FINDING_BYTES: usize = 24 * 1024;
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RunVerdict {
    Green,
    Red,
    Incomplete,
    SignoffsPending,
    Error,
    Cancelled,
}
impl RunVerdict {
    fn from_state(state: State) -> Option<Self> {
        match state {
            State::Green => Some(Self::Green),
            State::Red => Some(Self::Red),
            State::Incomplete => Some(Self::Incomplete),
            State::SignoffsPending => Some(Self::SignoffsPending),
            State::Error => Some(Self::Error),
            State::Cancelled => Some(Self::Cancelled),
            _ => None,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ResultCursor {
    run: RunId,
    revision: Sha256Digest,
    offset: usize,
}
impl TryFrom<String> for ResultCursor {
    type Error = String;
    fn try_from(value: String) -> Result<Self, String> {
        let mut parts = value.rsplitn(3, ':');
        let offset = parts
            .next()
            .and_then(|part| part.parse().ok())
            .ok_or("Invalid result cursor.")?;
        let revision = parts
            .next()
            .ok_or("Invalid result cursor.")?
            .parse::<Sha256Digest>()
            .map_err(|_| "Invalid result cursor.")?;
        let run = parts.next().ok_or("Invalid result cursor.")?.parse()?;
        Ok(Self {
            run,
            revision,
            offset,
        })
    }
}
impl From<ResultCursor> for String {
    fn from(cursor: ResultCursor) -> Self {
        format!("{}:{}:{}", cursor.run, cursor.revision, cursor.offset)
    }
}
impl std::str::FromStr for ResultCursor {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, String> {
        value.to_owned().try_into()
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub request: RequestId,
    pub eval: EvalId,
    pub state: RequestStatus,
    pub summary: String,
}
/// One result phase owns readiness and verdict. Only GREEN can carry recorded
/// satisfaction; older GREEN Runs without validation may still lack that proof.
#[derive(Debug, Clone, Copy)]
pub enum ResultState {
    Preparing,
    Running,
    Waiting,
    Cancelling,
    Green { satisfied: bool },
    Red,
    Incomplete,
    SignoffsPending,
    Error,
    Cancelled,
}
impl ResultState {
    fn from_observed(state: State, satisfied: bool) -> Self {
        match state {
            State::Preparing => Self::Preparing,
            State::Running => Self::Running,
            State::Waiting => Self::Waiting,
            State::Cancelling => Self::Cancelling,
            State::Green => Self::Green { satisfied },
            State::Red => Self::Red,
            State::Incomplete => Self::Incomplete,
            State::SignoffsPending => Self::SignoffsPending,
            State::Error => Self::Error,
            State::Cancelled => Self::Cancelled,
        }
    }
    pub fn state(self) -> State {
        match self {
            Self::Preparing => State::Preparing,
            Self::Running => State::Running,
            Self::Waiting => State::Waiting,
            Self::Cancelling => State::Cancelling,
            Self::Green { .. } => State::Green,
            Self::Red => State::Red,
            Self::Incomplete => State::Incomplete,
            Self::SignoffsPending => State::SignoffsPending,
            Self::Error => State::Error,
            Self::Cancelled => State::Cancelled,
        }
    }
}
#[derive(Debug, Clone)]
pub struct RunResultPage {
    pub run: RunId,
    pub phase: ResultState,
    pub findings: Vec<Finding>,
    pub next_cursor: Option<ResultCursor>,
}
impl RunResultPage {
    pub fn state(&self) -> State {
        self.phase.state()
    }
    pub fn ready(&self) -> bool {
        self.state().is_terminal()
    }
    pub fn verdict(&self) -> Option<RunVerdict> {
        RunVerdict::from_state(self.state())
    }
    pub fn satisfied(&self) -> bool {
        matches!(self.phase, ResultState::Green { satisfied: true })
    }
}
impl Serialize for RunResultPage {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Preserve the flat CLI/MCP wire format; all status flags are derived here.
        let mut output = serializer.serialize_map(None)?;
        output.serialize_entry("run", &self.run)?;
        output.serialize_entry("state", &self.state())?;
        output.serialize_entry("ready", &self.ready())?;
        output.serialize_entry("verdict", &self.verdict())?;
        output.serialize_entry("satisfied", &self.satisfied())?;
        output.serialize_entry("findings", &self.findings)?;
        output.serialize_entry("next_cursor", &self.next_cursor)?;
        output.end()
    }
}
pub(super) fn finding(request: &crate::store::Request) -> Finding {
    let summary = if let Some(error) = request.error() {
        error.to_owned()
    } else if let Some(crate::store::ExecutionResult::Runtime(result)) = request.result() {
        match result.exit_code.value() {
            Some(code) => format!("Runtime check exited {code}."),
            None => format!("Runtime check: {}.", request.status()),
        }
    } else {
        request.status().to_string()
    };
    Finding {
        request: request.id.clone(),
        eval: request.eval_id.clone(),
        state: request.status(),
        summary: summary.chars().take(SUMMARY_CHARS).collect(),
    }
}
pub async fn result(
    context: &Context,
    id: &RunId,
    cursor: Option<ResultCursor>,
) -> Result<RunResultPage, String> {
    let observation = observe::status(context, id).await?;
    if !observation.state.is_terminal() {
        if cursor.is_some() {
            return Err("A running Run has no result cursor.".into());
        }
        return Ok(RunResultPage {
            run: id.clone(),
            phase: ResultState::from_observed(observation.state, false),
            findings: Vec::new(),
            next_cursor: None,
        });
    }
    let state_path = context.state.clone();
    let run_id = id.clone();
    let (view, details) = crate::task::joined(
        tokio::task::spawn_blocking(move || {
            let mut reader = crate::store::reader::Reader::default();
            reader.snapshot(&state_path, |db, cache, state| {
                cache
                    .selected(db, state, &run_id, None, true)?
                    .ok_or_else(|| crate::store::receipts::Error::Invalid("Run not found.".into()))
            })
        })
        .await,
    )
    .map_err(|e| e.to_string())??;
    let state = observe::state(&view.run, &view.requests);
    let revision =
        Sha256Digest::of(&serde_json::to_vec(&(&view, &details)).map_err(|e| e.to_string())?);
    let offset = if let Some(cursor) = cursor {
        if cursor.run != *id || cursor.revision != revision {
            return Err("Expired or foreign result cursor.".into());
        }
        cursor.offset
    } else {
        0
    };
    if offset > view.requests.len() {
        return Err("Invalid result cursor offset.".into());
    }
    let findings = if state.is_terminal() {
        view.requests
            .iter()
            .skip(offset)
            .take(PAGE_SIZE)
            .map(finding)
            .scan(0, |bytes, finding| {
                let size = serde_json::to_vec(&finding)
                    .expect("typed finding serializes")
                    .len()
                    + 1;
                *bytes += size;
                (*bytes <= FINDING_BYTES).then_some(finding)
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let next = offset + findings.len();
    let next_cursor = (state.is_terminal() && next < view.requests.len()).then(|| ResultCursor {
        run: id.clone(),
        revision,
        offset: next,
    });
    Ok(RunResultPage {
        run: id.clone(),
        phase: ResultState::from_observed(
            state,
            view.run.validation.snapshot().is_some_and(|validation| {
                matches!(validation.satisfied, crate::config::Field::Value(true))
            }),
        ),
        findings,
        next_cursor,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn result_phase_is_the_only_source_of_wire_readiness_and_verdict() {
        let page = |phase| RunResultPage {
            run: "run-wire".parse().unwrap(),
            phase,
            findings: Vec::new(),
            next_cursor: None,
        };
        assert_eq!(
            serde_json::to_value(page(ResultState::Running)).unwrap(),
            json!({
                "run":"run-wire","state":"running","ready":false,"verdict":null,
                "satisfied":false,"findings":[],"next_cursor":null,
            })
        );
        assert_eq!(
            serde_json::to_value(page(ResultState::Green { satisfied: true })).unwrap(),
            json!({
                "run":"run-wire","state":"green","ready":true,"verdict":"GREEN",
                "satisfied":true,"findings":[],"next_cursor":null,
            })
        );
        let red = serde_json::to_value(page(ResultState::Red)).unwrap();
        assert_eq!(red["verdict"], "RED");
        assert_eq!(red["ready"], true);
        assert_eq!(red["satisfied"], false);
    }
}
