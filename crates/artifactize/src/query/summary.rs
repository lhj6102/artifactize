use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::store::{Request, RequestView, RunView};

pub fn request_output(view: &RequestView) -> Value {
    let mut value = json!(view);
    let request = &view.request;
    let (attempts, reported, unreported, usage) = usage_totals([request]);
    value["summary"] = json!({
        "status":request.status,
        "wallMs":wall_ms(&request.created_at, request.completed_at.as_deref()),
        "executorStarts":u64::from(local_execution(request) && request.started_at.is_some() && request.profile["kind"] != "human"),
        "attempts":attempts,
        "toolCalls":tool_totals([request]),
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
    value["summary"] = json!({
        "counts":counts,
        "wallMs":wall_ms(&view.run.created_at, view.run.completed_at.as_deref()),
        "executorStarts":view.run.executions_started,
        "attempts":attempts,"toolCalls":tool_totals(local.iter().copied()),
        "usageState":usage_state(reported, unreported),
        "reportedAttempts":reported,"unreportedAttempts":unreported,"usage":usage,
    });
    value
}

fn local_execution(request: &Request) -> bool {
    request
        .provenance
        .as_ref()
        .is_some_and(|source| source.request_id == request.id)
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
                let mut has_usage = false;
                if let Some(usage) = attempt["usage"].as_object() {
                    for (key, value) in usage {
                        if let Some(value) = value.as_u64() {
                            has_usage = true;
                            let total = totals.entry(key.clone()).or_default();
                            *total = total.saturating_add(value);
                        }
                    }
                }
                if has_usage {
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

fn usage_state(reported: u64, unreported: u64) -> &'static str {
    match (reported, unreported) {
        (0, _) => "unreported",
        (_, 0) => "reported",
        _ => "partial",
    }
}
