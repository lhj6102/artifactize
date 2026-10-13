//! Bounded pages derived from a saved Run/request/execution read snapshot, never a second store.
use super::{Context, State, observe};
use crate::types::{EvalId, RequestId, RequestStatus, RunId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
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
    Error,
    Cancelled,
}
impl RunVerdict {
    fn from_state(state: State) -> Option<Self> {
        match state {
            State::Green => Some(Self::Green),
            State::Red => Some(Self::Red),
            State::Incomplete => Some(Self::Incomplete),
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
    revision: String,
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
            .filter(|part| part.len() == 64 && part.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .ok_or("Invalid result cursor.")?
            .to_owned();
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
#[derive(Debug, Clone, Serialize)]
pub struct RunResultPage {
    pub run: RunId,
    pub state: State,
    pub ready: bool,
    pub verdict: Option<RunVerdict>,
    pub satisfied: bool,
    pub findings: Vec<Finding>,
    pub next_cursor: Option<ResultCursor>,
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
            state: observation.state,
            ready: false,
            verdict: None,
            satisfied: false,
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
        Sha256::digest(serde_json::to_vec(&(&view, &details)).map_err(|e| e.to_string())?)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
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
            .map(|request| {
                let summary = if let Some(error) = request.error() {
                    error.to_owned()
                } else if let Some(crate::store::ExecutionResult::Runtime(result)) =
                    request.result()
                {
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
            })
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
        state,
        ready: state.is_terminal(),
        verdict: RunVerdict::from_state(state),
        satisfied: view.run.validation.snapshot().is_some_and(|validation| {
            matches!(validation.satisfied, crate::config::Field::Value(true))
        }),
        findings,
        next_cursor,
    })
}
