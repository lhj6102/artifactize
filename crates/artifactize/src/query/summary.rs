use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::store::{ExecutionOptions, Request, RequestView, RunView};

pub fn request_output(view: &RequestView) -> Value {
    let mut value = json!(view);
    let request = &view.request;
    // A reused request spent nothing; the source's attempts and tools stay in its audit.
    let spent = (!reused(request)).then_some(request);
    let (attempts, reported, unreported, usage) = usage_totals(spent);
    value["summary"] = json!({
        "status":request.status,
        "wallMs":wall_ms(&request.created_at, request.completed_at.as_deref()),
        "executorStarts":u64::from(local_execution(request) && request.started_at.is_some() && request.profile["kind"] != "human"),
        "attempts":attempts,
        "toolCalls":tool_totals(spent),
        "usageState":usage_state(reported, unreported),
        "reportedAttempts":reported,"unreportedAttempts":unreported,"usage":usage,
        "executionSource":request.provenance,
    });
    value
}

pub fn run_output(view: &RunView) -> Value {
    let mut value = json!(view);
    let mut counts = BTreeMap::<&str, u64>::new();
    let mut seen = BTreeSet::new();
    let local: Vec<_> = view
        .requests
        .iter()
        .filter(|request| {
            *counts.entry(&request.status).or_default() += 1;
            local_execution(request)
                && request
                    .execution_id
                    .as_ref()
                    .is_some_and(|id| seen.insert(id))
        })
        .collect();
    let (attempts, reported, unreported, usage) = usage_totals(local.iter().copied());
    let kinds = || BTreeMap::from([("total", 0u64), ("runtime", 0), ("agent", 0), ("human", 0)]);
    let (mut executed, mut reuses, mut saved) = (kinds(), kinds(), BTreeMap::new());
    // Reused results that another profile produced: the key leaves execution options out.
    reuses.insert("otherProfile", 0);
    for request in &view.requests {
        let tally = if reused(request) {
            // Requests saved before reusedUsage existed kept the original attempts in usage.
            let original = request.reused_usage.as_ref().or(request.usage.as_ref());
            for attempt in original.and_then(Value::as_array).into_iter().flatten() {
                add_usage(&mut saved, attempt);
            }
            if request.profile != request.requested_profile {
                *reuses.get_mut("otherProfile").expect("tally") += 1;
            }
            &mut reuses
        } else if local_execution(request) {
            &mut executed
        } else {
            continue;
        };
        for kind in [
            "total",
            request.profile["kind"].as_str().unwrap_or_default(),
        ] {
            if let Some(count) = tally.get_mut(kind) {
                *count += 1;
            }
        }
    }
    value["summary"] = json!({
        "counts":counts,
        "executed":executed,"reused":reuses,
        "wallMs":wall_ms(&view.run.created_at, view.run.completed_at.as_deref()),
        "executorStarts":view.run.executions_started,
        "attempts":attempts,"toolCalls":tool_totals(local.iter().copied()),
        "usageState":usage_state(reported, unreported),
        "reportedAttempts":reported,"unreportedAttempts":unreported,"usage":usage,
    });
    value["usage"] = json!({"spent":value["summary"]["usage"],"saved":saved});
    value
}

/// A short name for the profile that produced a request's result: its variant, or its kind
/// with the Agent backend, model and reasoning.
pub fn profile_name(profile: &Value, options: &ExecutionOptions) -> String {
    if let Some(variant) = &options.variant {
        return variant.clone();
    }
    match profile["kind"].as_str() {
        Some("agent") => [
            options.backend.as_deref(),
            options.model.as_deref(),
            options.reasoning.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" "),
        Some(kind) => kind.to_owned(),
        None => "unknown".into(),
    }
}

fn local_execution(request: &Request) -> bool {
    request
        .provenance
        .as_ref()
        .is_some_and(|source| source.request_id == request.id)
}

/// The result came from another request's execution, possibly in this Run.
pub fn reused(request: &Request) -> bool {
    request
        .provenance
        .as_ref()
        .is_some_and(|source| source.request_id != request.id)
}

fn wall_ms(start: &str, end: Option<&str>) -> Option<u64> {
    let start = OffsetDateTime::parse(start, &Rfc3339).ok()?;
    let end = end
        .map(|end| OffsetDateTime::parse(end, &Rfc3339))
        .transpose()
        .ok()?
        .unwrap_or_else(OffsetDateTime::now_utc);
    Some((end - start).whole_milliseconds().max(0) as u64)
}

fn tool_totals<'a>(requests: impl IntoIterator<Item = &'a Request>) -> BTreeMap<&'a str, u64> {
    let mut counts = BTreeMap::new();
    for request in requests {
        for call in &request.tool_calls {
            if let Some(name) = call["name"].as_str() {
                *counts.entry(name).or_default() += 1;
            }
        }
    }
    counts
}

fn usage_totals<'a>(
    requests: impl IntoIterator<Item = &'a Request>,
) -> (u64, u64, u64, BTreeMap<String, u64>) {
    let (mut attempts, mut reported, mut unreported) = (0, 0, 0);
    let mut totals = BTreeMap::<String, u64>::new();
    for request in requests {
        if let Some(entries) = request.usage.as_ref().and_then(Value::as_array) {
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
        } else if request.execution_id.is_some() && request.profile["kind"] != "agent" {
            attempts += 1;
            unreported += 1;
        }
    }
    (attempts, reported, unreported, totals)
}

/// Adds one attempt's reported counters; false when it reported none.
fn add_usage(totals: &mut BTreeMap<String, u64>, attempt: &Value) -> bool {
    let mut has_usage = false;
    for (key, value) in attempt["usage"].as_object().into_iter().flatten() {
        if let Some(value) = value.as_u64() {
            has_usage = true;
            let total = totals.entry(key.clone()).or_default();
            *total = total.saturating_add(value);
        }
    }
    has_usage
}

fn usage_state(reported: u64, unreported: u64) -> &'static str {
    match (reported, unreported) {
        (0, 0) => "none",
        (0, _) => "unreported",
        (_, 0) => "reported",
        _ => "partial",
    }
}
