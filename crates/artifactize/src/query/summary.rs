use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::{Value, json};
use time::OffsetDateTime;

use crate::store::{ExecutionOptions, Request, RequestView, RunView};

/// How a request got a result that another request's execution produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    /// A completed local record of the reuse key.
    Cache,
    /// The live execution of the reuse key that the request waited for.
    Joined,
    /// A record from the remote review store.
    Remote,
    /// Current graph evidence, without an execution or reusable record.
    Derived,
}

/// The request whose execution produced a reused result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Source<'a> {
    pub run_id: &'a crate::types::RunId,
    pub request_id: &'a crate::types::RequestId,
    pub kind: SourceKind,
}

/// Where a request's result came from: `None` when the request executed itself or has no
/// result from an execution yet. The one rule behind `source`, `summary.reused`, the text
/// marker and the monitor.
pub fn source(request: &Request) -> Option<Source<'_>> {
    if request.profile.kind() == crate::config::ProfileKind::Dependency {
        return Some(Source {
            run_id: &request.run_id,
            request_id: &request.id,
            kind: SourceKind::Derived,
        });
    }
    let provenance = request
        .provenance
        .as_ref()
        .filter(|source| source.request_id != request.id)?;
    let kind = if request.origin.is_some() {
        SourceKind::Remote
    } else if request.joined {
        SourceKind::Joined
    } else {
        SourceKind::Cache
    };
    Some(Source {
        run_id: &provenance.run_id,
        request_id: &provenance.request_id,
        kind,
    })
}

/// The result came from another request's execution, possibly in this Run.
pub fn reused(request: &Request) -> bool {
    source(request).is_some_and(|source| source.kind != SourceKind::Derived)
}

/// A saved request as output: `source` stands for the saved `joined` flag.
fn with_source(value: &mut Value, request: &Request) {
    let object = value.as_object_mut().expect("a request is an object");
    object.remove("joined");
    object.insert("source".into(), json!(source(request)));
    // The reference in the text form `session show` and `session send` take.
    if let (Some(reference), Some(Value::Object(session))) =
        (&request.session, object.get_mut("session"))
    {
        session.insert("ref".into(), json!(reference.to_string()));
    }
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageTotals {
    pub attempts: u64,
    pub usage_state: UsageState,
    pub reported_attempts: u64,
    pub unreported_attempts: u64,
    pub usage: BTreeMap<String, u64>,
}
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UsageState {
    #[default]
    None,
    Unreported,
    Reported,
    Partial,
}
#[derive(Debug, Default, Serialize)]
pub struct Kinds {
    pub total: u64,
    pub runtime: u64,
    pub agent: u64,
    pub human: u64,
}
impl Kinds {
    fn add(&mut self, kind: crate::config::ProfileKind) {
        if kind == crate::config::ProfileKind::Dependency {
            return;
        }
        self.total += 1;
        match kind {
            crate::config::ProfileKind::Runtime => self.runtime += 1,
            crate::config::ProfileKind::Agent => self.agent += 1,
            crate::config::ProfileKind::Human => self.human += 1,
            crate::config::ProfileKind::Dependency => unreachable!("dependency Evals are derived"),
        }
    }
}
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reuses {
    #[serde(flatten)]
    pub kinds: Kinds,
    pub other_profile: u64,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestSummary<'a> {
    pub status: crate::types::RequestStatus,
    pub wall_ms: Option<u64>,
    pub executor_starts: u64,
    #[serde(flatten)]
    pub usage: UsageTotals,
    pub execution_source: Option<&'a crate::store::Provenance>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub derived: u64,
    pub counts: BTreeMap<crate::types::RequestStatus, u64>,
    pub executed: Kinds,
    pub reused: Reuses,
    pub wall_ms: Option<u64>,
    pub executor_starts: u64,
    #[serde(flatten)]
    pub usage: UsageTotals,
}

pub fn request_output(view: &RequestView, now: OffsetDateTime) -> Value {
    let mut value = json!(view);
    let request = &view.request;
    with_source(&mut value, request);
    let summary = request_summary(view, now);
    value["summary"] = json!(summary);
    value
}

pub fn run_output(view: &RunView, now: OffsetDateTime) -> Value {
    let mut value = json!(view);
    let requests = value["requests"]
        .as_array_mut()
        .expect("serialized requests are an array");
    for (output, request) in requests.iter_mut().zip(&view.requests) {
        with_source(output, request);
    }
    let (summary, saved) = run_summary(view, now);
    value["usage"] = json!({"spent":summary.usage.usage,"saved":saved});
    value["summary"] = json!(summary);
    value
}

/// A short name for the profile that produced a request's result: its variant, or its kind
/// with the Agent backend, model and reasoning.
pub fn profile_name(profile: &crate::config::StoredProfile, options: &ExecutionOptions) -> String {
    if let Some(variant) = &options.variant {
        return variant.to_string();
    }
    match profile.kind() {
        crate::config::ProfileKind::Agent => [
            options.backend.as_ref().map(|backend| backend.as_str()),
            options.model.as_deref(),
            options.reasoning.map(crate::config::Reasoning::as_str),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" "),
        kind => kind.name().to_owned(),
    }
}

fn local_execution(request: &Request) -> bool {
    request
        .provenance
        .as_ref()
        .is_some_and(|source| source.request_id == request.id)
}

fn wall_ms(
    start: crate::types::Timestamp,
    end: Option<crate::types::Timestamp>,
    now: OffsetDateTime,
) -> Option<u64> {
    let end = end.map_or(now, crate::types::Timestamp::time);
    Some((end - start.time()).whole_milliseconds().max(0) as u64)
}

fn usage_totals<'a>(requests: impl IntoIterator<Item = &'a Request>) -> UsageTotals {
    let (mut attempts, mut reported, mut unreported) = (0, 0, 0);
    let mut totals = BTreeMap::<String, u64>::new();
    for request in requests {
        if let Some(entries) = request.usage.as_ref() {
            if entries.is_empty() && request.started_at.is_some() {
                unreported += 1;
            }
            for attempt in entries {
                attempts += 1;
                if add_usage(&mut totals, attempt) {
                    reported += 1;
                } else {
                    unreported += 1;
                }
            }
        } else if request.execution_id.is_some()
            && request.profile.kind() != crate::config::ProfileKind::Agent
        {
            attempts += 1;
            unreported += 1;
        }
    }
    UsageTotals {
        attempts,
        usage_state: usage_state(reported, unreported),
        reported_attempts: reported,
        unreported_attempts: unreported,
        usage: totals,
    }
}

/// Adds one attempt's reported counters; false when it reported none.
fn add_usage(totals: &mut BTreeMap<String, u64>, attempt: &crate::llm::Attempt) -> bool {
    for (key, value) in attempt.usage.iter() {
        let total = totals.entry(key.clone()).or_default();
        *total = total.saturating_add(*value);
    }
    !attempt.usage.is_empty()
}

fn usage_state(reported: u64, unreported: u64) -> UsageState {
    match (reported, unreported) {
        (0, 0) => UsageState::None,
        (0, _) => UsageState::Unreported,
        (_, 0) => UsageState::Reported,
        _ => UsageState::Partial,
    }
}

pub fn request_summary(view: &RequestView, now: OffsetDateTime) -> RequestSummary<'_> {
    let request = &view.request;
    RequestSummary {
        status: request.status(),
        wall_ms: wall_ms(request.created_at, request.completed_at(), now),
        executor_starts: u64::from(
            local_execution(request)
                && request.started_at.is_some()
                && request.profile.kind() != crate::config::ProfileKind::Human,
        ),
        usage: usage_totals((!reused(request)).then_some(request)),
        execution_source: request.provenance.as_ref(),
    }
}

pub fn run_summary(view: &RunView, now: OffsetDateTime) -> (RunSummary, BTreeMap<String, u64>) {
    let mut counts = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let local = view.requests.iter().filter(|request| {
        local_execution(request)
            && request
                .execution_id
                .as_ref()
                .is_some_and(|id| seen.insert(id))
    });
    let usage = usage_totals(local);
    let (mut executed, mut reuses, mut saved) =
        (Kinds::default(), Reuses::default(), BTreeMap::new());
    for request in &view.requests {
        *counts.entry(request.status()).or_default() += 1;
        if reused(request) {
            for attempt in request.reused_usage.iter().flatten() {
                add_usage(&mut saved, attempt);
            }
            reuses.other_profile += u64::from(request.profile != request.requested_profile);
            reuses.kinds.add(request.profile.kind());
        } else if local_execution(request) {
            executed.add(request.profile.kind());
        }
    }
    let summary = RunSummary {
        derived: view
            .requests
            .iter()
            .filter(|request| request.profile.kind() == crate::config::ProfileKind::Dependency)
            .count() as u64,
        counts,
        executed,
        reused: reuses,
        wall_ms: wall_ms(view.run.created_at, view.run.completed_at(), now),
        executor_starts: view.run.executions_started,
        usage,
    };
    (summary, saved)
}

impl std::fmt::Display for UsageState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::None => "none",
            Self::Unreported => "unreported",
            Self::Reported => "reported",
            Self::Partial => "partial",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn view() -> RunView {
        serde_json::from_value(json!({
            "id":"run-legacy",
            "repoPath":"/repo",
            "stateDir":"/state",
            "status":"RUNNING",
            "createdAt":"2026-01-01T00:00:00Z",
            "completedAt":null,
            "selection":{"kind":"all"},
            "validation":null,
            "error":null,
            "requests":[
                {
                    "id":"run-legacy-1",
                    "runId":"run-legacy",
                    "evalId":"app/check",
                    "target":"app",
                    "title":"Check",
                    "profile":{"kind":"runtime","command":"fixture","args":[]},
                    "requestedProfile":{
                        "kind":"runtime",
                        "command":"fixture",
                        "args":[],
                        "timeoutMs":null,
                    },
                    "evalDefHash":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
                    "executionId":"execution-source-1",
                    "provenance":{
                        "repoPath":"/source",
                        "runId":"run-source",
                        "requestId":"run-source-1",
                        "evalId":"app/check",
                        "evalDefHash":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
                        "completedAt":"2026-01-01T00:00:01Z",
                    },
                    "usage":null,
                    "reusedUsage":[
                        {
                            "turn":1,
                            "attempt":1,
                            "usage":{"inputTokens":17,"vendorDetail":{"cache":true}},
                        },
                    ],
                    "payload":{},
                    "references":{},
                    "deps":[],
                    "status":"GREEN",
                    "createdAt":"2026-01-01T00:00:00Z",
                    "startedAt":null,
                    "completedAt":"2026-01-01T00:00:01Z",
                    "cwd":"/repo",
                    "runDir":null,
                    "argv":null,
                    "child":null,
                    "result":{"verdict":"GREEN"},
                    "error":null,
                    "errorCode":null,
                    "blockedReason":null,
                },
            ],
        }))
        .unwrap()
    }
    #[test]
    fn supplied_time_is_deterministic_and_profile_omission_is_not_null() {
        let now = "2026-01-01T00:00:05Z"
            .parse::<crate::types::Timestamp>()
            .unwrap()
            .time();
        let view = view();
        let output = run_output(&view, now);
        assert_eq!(output["summary"]["wallMs"], 5000);
        assert_eq!(output["summary"]["reused"]["otherProfile"], 1);
        assert_eq!(output["summary"]["reused"]["runtime"], 1);
        assert_eq!(output["summary"]["attempts"], 0);
        assert_eq!(output["usage"]["saved"], json!({"inputTokens":17}));
        assert_eq!(run_output(&view, now), output);
        assert_eq!(
            wall_ms("2026-01-01T00:00:06Z".parse().unwrap(), None, now),
            Some(0)
        );
        let request = RequestView {
            request: view.requests[0].clone(),
            claim: None,
            execution: None,
            definition: None,
        };
        assert_eq!(request_output(&request, now)["summary"]["wallMs"], 1000);
        assert_eq!(
            serde_json::to_value(&request.request.profile).unwrap(),
            json!({"kind":"runtime","command":"fixture","args":[]})
        );
        assert_eq!(
            serde_json::to_value(&request.request.requested_profile).unwrap(),
            json!({"kind":"runtime","command":"fixture","args":[],"timeoutMs":null})
        );
    }
}
