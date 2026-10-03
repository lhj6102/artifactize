use std::collections::BTreeMap;

use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::store::{Request, RunSummary, RunView};

#[derive(Debug, PartialEq)]
pub(super) struct RunRow {
    pub id: String,
    pub repo: String,
    pub status: String,
    pub counts: BTreeMap<String, u64>,
    pub age: String,
}

impl RunRow {
    pub fn new(run: &RunSummary, now: OffsetDateTime) -> Self {
        Self {
            id: run.id.clone(),
            repo: text(&run.repo_path.to_string_lossy()),
            status: text(&run.status),
            counts: run.counts.clone(),
            age: elapsed(&run.created_at, None, now),
        }
    }
}

#[derive(Debug, PartialEq)]
pub(super) struct EvalRow {
    pub id: String,
    pub request: String,
    pub status: String,
    pub duration: String,
    pub error: Option<String>,
    pub reason: Option<String>,
}

impl EvalRow {
    fn new(request: &Request, now: OffsetDateTime) -> Self {
        Self {
            id: text(&request.eval_id),
            request: text(&request.id),
            status: text(&request.status),
            duration: request.started_at.as_deref().map_or_else(
                || "-".into(),
                |start| elapsed(start, request.completed_at.as_deref(), now),
            ),
            error: match (&request.error_code, &request.error) {
                (Some(code), Some(error)) => Some(text(&format!("{code}: {error}"))),
                (Some(error), None) | (None, Some(error)) => Some(text(error)),
                (None, None) => None,
            },
            reason: request.blocked_reason.as_deref().map(text),
        }
    }
}

#[derive(Debug, PartialEq)]
pub(super) struct Progress {
    pub id: String,
    pub repo: String,
    pub status: String,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub duration: String,
    pub satisfied: Option<bool>,
    pub counts: BTreeMap<String, u64>,
    pub running: Vec<EvalRow>,
    pub waiting: Vec<EvalRow>,
    pub other: Vec<EvalRow>,
    pub error: Option<String>,
}

impl Progress {
    pub fn new(view: &RunView, now: OffsetDateTime) -> Self {
        let run = &view.run;
        let mut counts: BTreeMap<String, u64> = [
            "GREEN",
            "RED",
            "ERROR",
            "RUNNING",
            "WAITING_HUMAN",
            "WAIT_DEPENDENCY",
            "BLOCKED",
            "QUEUED",
            "UNREVIEWED",
            "STALE",
            "BUDGET_EXHAUSTED",
        ]
        .into_iter()
        .map(|status| (status.into(), 0))
        .collect();
        let mut running = Vec::new();
        let mut waiting = Vec::new();
        let mut other = Vec::new();
        for request in &view.requests {
            *counts.entry(text(&request.status)).or_default() += 1;
            let row = EvalRow::new(request, now);
            match request.status.as_str() {
                "RUNNING" => running.push(row),
                "WAITING_HUMAN" => waiting.push(row),
                _ => other.push(row),
            }
        }
        Self {
            id: text(&run.id),
            repo: text(&run.repo_path.to_string_lossy()),
            status: text(&run.status),
            created_at: text(&run.created_at),
            completed_at: run.completed_at.as_deref().map(text),
            duration: elapsed(&run.created_at, run.completed_at.as_deref(), now),
            satisfied: run.validation["satisfied"].as_bool(),
            counts,
            running,
            waiting,
            other,
            error: run.error.as_deref().map(text),
        }
    }
}

pub(super) fn text(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

fn elapsed(start: &str, end: Option<&str>, now: OffsetDateTime) -> String {
    let Ok(start) = OffsetDateTime::parse(start, &Rfc3339) else {
        return "-".into();
    };
    let end = match end {
        Some(end) => match OffsetDateTime::parse(end, &Rfc3339) {
            Ok(end) => end,
            Err(_) => return "-".into(),
        },
        None => now,
    };
    let seconds = (end - start).whole_seconds().max(0);
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m {}s", seconds / 60, seconds % 60),
        3600..86400 => format!("{}h {}m", seconds / 3600, seconds / 60 % 60),
        _ => format!("{}d {}h", seconds / 86400, seconds / 3600 % 24),
    }
}
