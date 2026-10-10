//! Pure projections of saved state for the monitor screens.
mod states;
mod tree;
pub use states::{Activity, Busy, Completion, EvalView, NotRun, Queue, Source, Upstream, Waits};
pub use tree::{Kind, Node, Segment, Tone, Weight, completion, tree, upstream_index};

use serde_json::Value;
use time::OffsetDateTime;

use crate::{
    query,
    store::{RequestView, RunSummary, RunView},
};

/// What a tree node shows in the detail pane, parsed from its stable tree identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// The open Run itself, from the node above its Artifacts.
    Run,
    Artifact(String),
    Eval(String),
}

impl Target {
    pub fn parse(id: &str) -> Option<Self> {
        let (kind, name) = id.split_once(':')?;
        let name = name.to_owned();
        match kind {
            "run" => Some(Self::Run),
            "a" => Some(Self::Artifact(name)),
            "e" => Some(Self::Eval(name)),
            _ => None,
        }
    }
}

/// Where a Detail field goes, in display order. Technical is folded by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Section {
    Outcome,
    /// The Artifacts an eval waits for, each with its pending evals.
    Waits,
    What,
    Provenance,
    Technical,
}

impl Section {
    pub const ALL: [Self; 5] = [
        Self::Outcome,
        Self::Waits,
        Self::What,
        Self::Provenance,
        Self::Technical,
    ];

    /// The verdict first; identifiers and hashes last. Unknown fields stay visible in What.
    pub fn of(key: &str) -> Self {
        match key {
            "Status" | "Error" | "Reason" | "Blocked by" | "Result" | "Claim" | "Source"
            | "At Run end" | "Validation" | "Counts" | "Errors" | "Waiting" | "Running"
            | "Evals" | "Gates" => Self::Outcome,
            "Waits for" => Self::Waits,
            "Timing" | "Usage" | "Budget" | "Work" | "Saved usage" => Self::Provenance,
            "Request" | "Execution" | "Fingerprint" | "Key" | "Key covers" | "Path" | "Tags"
            | "Cycle" | "Run" | "Repository" => Self::Technical,
            _ => Self::What,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Outcome => "Outcome",
            Self::Waits => "Waits for",
            Self::What => "What",
            Self::Provenance => "Provenance",
            Self::Technical => "Technical",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRow {
    pub id: String,
    pub repo: String,
    pub status: String,
    /// Request status glyphs with their counts, most urgent first, such as `✗1 ✓2`.
    pub counts: String,
    pub age: String,
    /// Wall time of a finished Run.
    pub took: String,
}

#[derive(Debug, Clone, Default)]
pub struct Progress {
    pub status: String,
    pub repo: String,
    pub validation: String,
    pub timing: String,
    /// Elapsed time of a running Run, or how long a finished one took.
    pub elapsed: String,
    pub finished: bool,
    /// The saved validation snapshot: `None` until the Run ends.
    pub satisfied: Option<bool>,
    pub counts: Vec<(String, u64)>,
    /// Executions against their budget, and jobs.
    pub budget: String,
    /// Executed, reused and derived requests.
    pub work: String,
    /// Every spent token counter, and the counters reuse saved.
    pub usage: String,
    pub saved: String,
    /// The spent token total, the only usage the Run headline shows.
    pub tokens: Option<u64>,
    /// Eval ID and elapsed time.
    pub running: Vec<(String, String)>,
    /// Eval ID and claim state.
    pub waiting: Vec<(String, String)>,
    /// Eval ID (or "Run") and message.
    pub errors: Vec<(String, String)>,
}

/// The Run headline: one status line, then what needs attention.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Strip {
    pub status: String,
    pub headline: String,
    /// Status and text: errors, then waiting Human reviews, then running evals.
    pub attention: Vec<(String, String)>,
    /// Attention lines beyond the shown ones.
    pub more: usize,
}

#[derive(Debug, Clone, Default)]
pub struct Detail {
    pub title: String,
    /// One-line outcome: state, elapsed time, profile and dependencies.
    pub summary: String,
    pub fields: Vec<(&'static str, String)>,
}

impl Detail {
    pub fn field(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.as_str())
    }

    fn push(&mut self, key: &'static str, value: impl Into<String>) {
        let value = value.into();
        if !value.is_empty() {
            self.fields.push((key, value));
        }
    }

    fn insert_after(&mut self, after: &str, key: &'static str, value: String) {
        if value.is_empty() {
            return;
        }
        let index = self.fields.iter().position(|(name, _)| *name == after);
        let index = index.map_or(self.fields.len(), |index| index + 1);
        self.fields.insert(index, (key, value));
    }
}

/// Attention lines shown in the Run headline before `+N more`.
const ATTENTION_LINES: usize = 3;

/// Most to least urgent, for status counts in the header, Scope, Runs and the headline.
const URGENCY: &str = "ERROR RED BLOCKED RUNNING WAITING_HUMAN QUEUED BUDGET_EXHAUSTED \
    WAIT_DEPENDENCY WAIT STALE UNREVIEWED INCOMPLETE GREEN BASIS";
const ABSENT: &str = "not in Run";

pub fn glyph(status: Option<&str>) -> &'static str {
    match status {
        Some("GREEN") => "✓",
        Some("RED") => "✗",
        Some("ERROR") => "!",
        Some("RUNNING") => "◐",
        Some("WAITING_HUMAN") => "?",
        Some("QUEUED") => "·",
        Some("BLOCKED") => "⊘",
        Some("WAIT_DEPENDENCY" | "WAIT") => "…",
        Some("BUDGET_EXHAUSTED") => "$",
        Some("BASIS") => "◇",
        None => "-",
        _ => "○",
    }
}

fn meaning(status: &str) -> &'static str {
    match status {
        "GREEN" => "criteria met",
        "RED" => "criteria not met",
        "ERROR" => "operational failure, not a verdict",
        "RUNNING" => "executing",
        "WAITING_HUMAN" => "waiting for a Human submission",
        "BLOCKED" => "blocked by a RED dependency",
        "WAIT_DEPENDENCY" => "waiting for current GREEN dependency evidence",
        "BUDGET_EXHAUSTED" => "maxExecutions reached before it started",
        _ => "not executed",
    }
}

fn join<T: AsRef<str>>(items: impl IntoIterator<Item = T>, separator: &str) -> String {
    let items: Vec<_> = items
        .into_iter()
        .map(|item| item.as_ref().to_owned())
        .collect();
    items.join(separator)
}

fn pairs(value: &Value) -> String {
    let pairs = value.as_object().into_iter().flatten();
    join(pairs.map(|(key, value)| format!("{key} {value}")), ", ")
}

fn mark(name: &str, status: Option<&str>) -> String {
    format!("{} {name} {}", glyph(status), status.unwrap_or(ABSENT))
}

/// Seconds per minute, hour and day, the units a duration is shown in.
const MINUTE: i64 = 60;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;

pub fn duration(seconds: i64) -> String {
    let seconds = seconds.max(0);
    match seconds {
        0..MINUTE => format!("{seconds}s"),
        MINUTE..HOUR => format!("{}m {:02}s", seconds / MINUTE, seconds % MINUTE),
        HOUR..DAY => format!("{}h {:02}m", seconds / HOUR, seconds % HOUR / MINUTE),
        _ => format!("{}d {}h", seconds / DAY, seconds % DAY / HOUR),
    }
}

/// Saved RFC 3339 times; an open end is measured to `now`.
fn span(
    start: crate::types::Timestamp,
    end: Option<crate::types::Timestamp>,
    now: OffsetDateTime,
) -> Option<String> {
    let end = end.map_or(now, crate::types::Timestamp::time);
    Some(duration((end - start.time()).whole_seconds()))
}

/// Position in the urgency order; unknown statuses come last.
pub fn urgency(status: &str) -> usize {
    URGENCY
        .split_whitespace()
        .position(|known| known == status)
        .unwrap_or(usize::MAX)
}

/// Status counts as glyphs, most urgent first and zero counts left out: `!2 ✓1`.
pub fn counts<'a>(counts: impl IntoIterator<Item = (&'a str, u64)>) -> String {
    let mut counts: Vec<_> = counts.into_iter().filter(|(_, n)| *n > 0).collect();
    counts.sort_by_key(|(status, _)| urgency(status));
    join(
        counts
            .into_iter()
            .map(|(status, n)| format!("{}{n}", glyph(Some(status)))),
        " ",
    )
}

/// Token totals for a headline: `950`, `12.3k`, `3.9M`.
pub fn tokens(total: u64) -> String {
    match total {
        0..1_000 => total.to_string(),
        1_000..1_000_000 => format!("{:.1}k", total as f64 / 1e3),
        _ => format!("{:.1}M", total as f64 / 1e6),
    }
}

pub fn run_rows(runs: &[RunSummary], now: OffsetDateTime) -> Vec<RunRow> {
    runs.iter()
        .map(|run| RunRow {
            id: run.id.to_string(),
            repo: run.repo_path.display().to_string(),
            status: run.status.to_string(),
            counts: counts(run.counts.iter().map(|(status, n)| (status.as_str(), *n))),
            age: span(run.created_at, None, now).unwrap_or_default(),
            took: run
                .completed_at
                .and_then(|end| span(run.created_at, Some(end), now))
                .unwrap_or_default(),
        })
        .collect()
}

fn reused(view: &RequestView) -> bool {
    query::reused(&view.request)
}

fn elapsed(view: &RequestView, now: OffsetDateTime) -> Option<String> {
    let request = &view.request;
    if view.request.profile.kind() == crate::config::ProfileKind::Dependency {
        return Some("derived".into());
    }
    if reused(view) {
        return Some("reused".into());
    }
    let active = matches!(request.status.as_str(), "RUNNING" | "WAITING_HUMAN");
    let start = request
        .started_at
        .or(active.then_some(request.created_at))?;
    span(start, request.completed_at, now)
}

fn claim(view: &RequestView) -> String {
    view.claim.as_ref().map_or("unclaimed".into(), |claim| {
        format!("claimed by {} at {}", claim.reviewer, claim.claimed_at)
    })
}

fn error(view: &RequestView) -> Option<String> {
    let request = &view.request;
    let error = request.error.clone()?;
    Some(match &request.error_code {
        Some(code) => format!("[{code}] {error}"),
        None => error,
    })
}

pub fn progress(view: &RunView, requests: &[RequestView], now: OffsetDateTime) -> Progress {
    let run = &view.run;
    let output = query::run_output(view, now);
    let summary = &output["summary"];
    let with = |status: &'static str| {
        requests
            .iter()
            .filter(move |view| view.request.status.as_str() == status)
    };
    let usage = pairs(&summary["usage"]);
    let saved = pairs(&output["usage"]["saved"]);
    let spent = &summary["usage"];
    let tokens = spent["totalTokens"].as_u64().or_else(|| {
        let input = spent["inputTokens"].as_u64();
        let output = spent["outputTokens"].as_u64();
        (input.is_some() || output.is_some())
            .then(|| input.unwrap_or(0).saturating_add(output.unwrap_or(0)))
    });
    let validation = run.validation.snapshot();
    let unmet = validation
        .and_then(|v| v.obligations.value())
        .map(|ids| {
            ids.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let wall = span(run.created_at, run.completed_at, now).unwrap_or_default();
    Progress {
        status: run.status.to_string(),
        repo: run.repo_path.display().to_string(),
        // The saved snapshot; the tree shows the current state.
        validation: match validation.and_then(|v| v.satisfied.value()) {
            None => "pending".into(),
            Some(true) => "SATISFIED at Run end".into(),
            Some(_) if unmet.is_empty() => "NOT SATISFIED at Run end".into(),
            Some(_) => format!("NOT SATISFIED at Run end (unmet: {unmet})"),
        },
        timing: format!(
            "started {} · {} {wall}",
            run.created_at,
            if run.completed_at.is_some() {
                "took"
            } else {
                "elapsed"
            },
        ),
        elapsed: wall,
        finished: run.completed_at.is_some(),
        satisfied: validation.and_then(|v| v.satisfied.value()).copied(),
        counts: summary["counts"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(status, count)| (status.clone(), count.as_u64().unwrap_or(0)))
            .collect(),
        budget: format!(
            "executions {}/{} · jobs {}",
            run.executions_started,
            run.max_executions
                .map_or("unlimited".into(), |max| max.to_string()),
            run.jobs,
        ),
        work: format!(
            "executed {} · reused {} · derived {}",
            summary["executed"]["total"],
            summary["reused"]["total"],
            summary["derived"].as_u64().unwrap_or(0),
        ),
        usage,
        saved,
        tokens,
        running: with("RUNNING")
            .map(|view| {
                (
                    view.request.eval_id.to_string(),
                    elapsed(view, now).unwrap_or_default(),
                )
            })
            .collect(),
        waiting: with("WAITING_HUMAN")
            .map(|view| (view.request.eval_id.to_string(), claim(view)))
            .collect(),
        errors: with("ERROR")
            .map(|view| {
                (
                    view.request.eval_id.to_string(),
                    error(view).unwrap_or_default(),
                )
            })
            .chain(run.error.clone().map(|error| ("Run".into(), error)))
            .collect(),
    }
}

/// The Run headline from its progress: snapshot verdict, counts, time and token total.
pub fn strip(progress: &Progress) -> Strip {
    let validation = match progress.satisfied {
        None => "validation pending".to_owned(),
        Some(true) => "SATISFIED at Run end".to_owned(),
        Some(false) => "NOT SATISFIED at Run end".to_owned(),
    };
    let counts = counts(
        progress
            .counts
            .iter()
            .map(|(status, n)| (status.as_str(), *n)),
    );
    let time = format!(
        "{} {}",
        if progress.finished { "took" } else { "elapsed" },
        progress.elapsed
    );
    let tokens = progress
        .tokens
        .map(|total| format!("tokens {}", tokens(total)));
    let headline = join(
        [Some(validation), Some(counts), Some(time), tokens]
            .into_iter()
            .flatten()
            .filter(|part| !part.is_empty()),
        " · ",
    );
    let mut attention: Vec<(String, String)> = progress
        .errors
        .iter()
        .map(|(eval, error)| ("ERROR".into(), format!("{eval}  {error}")))
        .chain(progress.waiting.iter().map(|(eval, claim)| {
            (
                "WAITING_HUMAN".into(),
                format!("{eval}  waiting Human · {claim}"),
            )
        }))
        .chain(progress.running.iter().map(|(eval, time)| {
            (
                "RUNNING".into(),
                format!("{eval}  running {time}").trim_end().to_owned(),
            )
        }))
        .collect();
    let more = attention.len().saturating_sub(ATTENTION_LINES);
    attention.truncate(ATTENTION_LINES);
    Strip {
        status: progress.status.clone(),
        headline,
        attention,
        more,
    }
}

/// The row above the Artifacts; Enter on it opens the Run detail.
pub fn run_node(run: &RunView) -> Node {
    let status = run.run.status.to_string();
    Node {
        id: format!("run:{}", run.run.id),
        clock: None,
        kind: Kind::Artifact {
            completion: Completion::Complete,
            basis: false,
        },
        glyph: glyph(Some(&status)),
        tone: match status.as_str() {
            "GREEN" => Tone::Green,
            "RED" => Tone::Red,
            "ERROR" => Tone::Error,
            "RUNNING" => Tone::Running,
            "BLOCKED" | "BUDGET_EXHAUSTED" | "INCOMPLETE" => Tone::Blocked,
            _ => Tone::Muted,
        },
        weight: Weight::Normal,
        name: "Run".into(),
        marks: String::new(),
        text: vec![Segment {
            text: "Enter: usage, budgets, counts".into(),
            tone: Some(Tone::Muted),
        }],
        right: String::new(),
        compact: String::new(),
        changed: false,
        upstream: Vec::new(),
        children: Vec::new(),
    }
}

/// Top-level entries of a result, one line each, such as `verdict GREEN`. Saved runtime
/// logs are left to the evidence view.
fn result_summary(result: &Value) -> String {
    let Some(object) = result.as_object() else {
        return result
            .to_string()
            .lines()
            .next()
            .unwrap_or_default()
            .to_owned();
    };
    // The verdict first, then the other entries in their saved order.
    let mut entries: Vec<_> = object
        .iter()
        .filter(|(key, _)| !matches!(key.as_str(), "stdout" | "stderr"))
        .collect();
    entries.sort_by_key(|(key, _)| *key != "verdict");
    let parts = entries.into_iter().map(|(key, value)| match value {
        Value::String(text) => {
            let line = text.lines().next().unwrap_or_default();
            if line.len() < text.trim_end().len() {
                format!("{key} {line} …")
            } else {
                format!("{key} {line}")
            }
        }
        Value::Array(items) => format!("{key} [{} items]", items.len()),
        Value::Object(fields) => format!("{key} {{{} fields}}", fields.len()),
        other => format!("{key} {other}"),
    });
    join(parts, "\n")
}

/// Saved Run definitions joined with the Run's requests.
struct Saved<'a> {
    run: &'a RunView,
    requests: &'a [RequestView],
}

impl<'a> Saved<'a> {
    fn definitions(&self) -> Option<&'a crate::store::definitions::Graph> {
        self.run.run.definitions.graph()
    }
    fn artifacts(
        &self,
    ) -> impl Iterator<
        Item = (
            &'a crate::types::ArtifactName,
            &'a crate::store::definitions::Artifact,
        ),
    > {
        self.definitions()
            .into_iter()
            .flat_map(|graph| graph.artifacts())
    }
    fn artifact(&self, id: &str) -> Option<&'a crate::store::definitions::Artifact> {
        self.definitions().and_then(|graph| graph.artifact(id))
    }
    fn eval_definitions(&self) -> &'a [crate::store::definitions::Eval] {
        self.definitions().map_or(&[], |graph| graph.evals())
    }
    fn components(&self) -> &'a [crate::store::definitions::Component] {
        self.definitions().map_or(&[], |graph| graph.components())
    }

    fn request(&self, eval: &str) -> Option<&'a RequestView> {
        self.requests
            .iter()
            .find(|view| view.request.eval_id == eval)
    }

    fn status(&self, eval: &str) -> Option<&'a str> {
        self.request(eval).map(|view| view.request.status.as_str())
    }

    /// Dependency-first component order with cycle peers by name, then any other saved or
    /// requested Artifact.
    fn artifact_ids(&self) -> Vec<&'a str> {
        let components = self.components().iter();
        let saved = components
            .flat_map(|component| {
                let mut members: Vec<_> = component
                    .artifacts
                    .value()
                    .into_iter()
                    .flatten()
                    .map(crate::types::ArtifactName::as_str)
                    .collect();
                members.sort_unstable();
                members
            })
            .chain(self.artifacts().map(|(id, _)| id.as_str()))
            .chain(
                self.requests
                    .iter()
                    .map(|view| view.request.target.as_str()),
            );
        let mut seen = std::collections::HashSet::new();
        saved.filter(|id| seen.insert(*id)).collect()
    }

    /// Saved Eval definitions targeting the Artifact, then requests without one.
    fn evals(&self, artifact: &str) -> Vec<(&'a str, Option<&'a crate::store::definitions::Eval>)> {
        let mut evals: Vec<_> = self
            .eval_definitions()
            .iter()
            .filter(|eval| eval.target == artifact)
            .map(|eval| (eval.id.as_str(), Some(eval)))
            .collect();
        for request in self.requests.iter().map(|view| &view.request) {
            if request.target == artifact && !evals.iter().any(|(id, _)| *id == request.eval_id) {
                evals.push((&request.eval_id, None));
            }
        }
        evals
    }

    fn validation(&self, artifact: &str) -> Option<&'a crate::store::ArtifactValidation> {
        self.run
            .run
            .validation
            .snapshot()?
            .artifacts
            .value()?
            .iter()
            .find(|a| a.id == artifact)
    }

    fn component(&self, id: &str) -> Option<&'a crate::store::definitions::Component> {
        self.components().iter().find(|component| {
            component
                .artifacts
                .value()
                .is_some_and(|artifacts| artifacts.iter().any(|artifact| artifact == id))
        })
    }

    fn relations(
        &self,
        artifact: &str,
        input: bool,
    ) -> Vec<&'a crate::store::definitions::Relation> {
        self.definitions()
            .into_iter()
            .flat_map(|graph| graph.relations())
            .filter(|relation| {
                if input {
                    relation.target == artifact
                } else {
                    relation.source == artifact
                }
            })
            .collect()
    }
}

fn relation_kind(relation: &crate::store::definitions::Relation) -> String {
    use crate::store::definitions::RelationKind;
    let kind = match &relation.kind {
        RelationKind::Unknown { .. } => serde_json::to_string(relation).unwrap_or_default(),
        RelationKind::Child { path } => {
            format!("child {}", path.value().map_or("", String::as_str))
        }
        RelationKind::Mount { alias } => {
            format!("mount {}", alias.value().map_or("", String::as_str))
        }
        RelationKind::Dependency { name, eval_id } => format!(
            "dependency {} in {}",
            name.value().map_or("", String::as_str),
            eval_id.value().map_or("", crate::types::EvalId::as_str)
        ),
        RelationKind::Instruction { name, eval_id } => format!("{{{name}}} in {eval_id}"),
        RelationKind::Argument {
            eval_id,
            index,
            name,
            ..
        } => format!("argv {eval_id}[{index}] {{{name}}}"),
    };
    if relation.cyclic.value() == Some(&true) {
        format!("{kind} ↻")
    } else {
        kind
    }
}

fn profile(profile: &crate::config::StoredProfile) -> String {
    use crate::config::{Field, StoredProfile};
    match profile {
        StoredProfile::Agent {
            backend,
            model,
            reasoning,
            ..
        } => {
            let reasoning = match reasoning {
                Field::Value(reasoning) => format!(" reasoning {reasoning}"),
                _ => String::new(),
            };
            format!(
                "agent {} {model}{reasoning}",
                serde_json::to_value(backend)
                    .expect("backend is JSON")
                    .as_str()
                    .expect("backend is a string")
            )
        }
        StoredProfile::Runtime { command, args, .. } => {
            format!("runtime {command} {}", args.join(" "))
        }
        StoredProfile::Human {} => "human".into(),
        StoredProfile::Dependency { depends_on } => format!("dependency {}", depends_on.join(", ")),
    }
}

/// Execution options that are set, such as `variant fast · timeoutMs 60000`.
fn options(options: &crate::store::ExecutionOptions) -> String {
    let value = serde_json::to_value(options).unwrap_or_default();
    let set: Vec<_> = value
        .as_object()
        .into_iter()
        .flatten()
        .map(|(key, value)| match value.as_str() {
            Some(text) => format!("{key} {text}"),
            None => format!("{key} {value}"),
        })
        .collect();
    if set.is_empty() {
        "declared defaults".into()
    } else {
        set.join(" · ")
    }
}

fn request_detail(view: &RequestView, now: OffsetDateTime) -> Detail {
    let request = &view.request;
    let summary = &query::request_output(view, now)["summary"];
    let deps = join(&request.deps, ", ");
    let mut detail = Detail {
        title: format!("{} · {}", request.eval_id, request.title),
        summary: join(
            [
                format!(
                    "{}{}",
                    request.status,
                    elapsed(view, now).map_or(String::new(), |time| format!(" {time}"))
                ),
                profile(&request.profile),
                if deps.is_empty() {
                    deps
                } else {
                    format!("← {deps}")
                },
            ]
            .into_iter()
            .filter(|part| !part.is_empty()),
            " · ",
        ),
        fields: Vec::new(),
    };
    detail.push(
        "Status",
        format!("{} — {}", request.status, meaning(request.status.as_str())),
    );
    detail.push("Request", request.id.as_str());
    detail.push("Reason", request.blocked_reason.clone().unwrap_or_default());
    if request.profile.kind() == crate::config::ProfileKind::Dependency {
        detail.push("Source", "derived (no execution)");
        detail.push(
            "Blocked by",
            request
                .blocked_by
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    detail.push("Error", error(view).unwrap_or_default());
    detail.push("Instruction", request.payload.instruction());
    detail.push("Profile", profile(&request.profile));
    if request.requested_profile != request.profile {
        detail.push("Requested", profile(&request.requested_profile));
    }
    detail.push("Options", options(&request.options));
    detail.push(
        "Fingerprint",
        request.fingerprint.as_deref().unwrap_or("none"),
    );
    detail.push("Key", request.key.as_deref().unwrap_or("none (no reuse)"));
    detail.push(
        "Key covers",
        join(
            request
                .fingerprints
                .iter()
                .map(|(name, fingerprint)| format!("{name} {fingerprint}")),
            "\n",
        ),
    );
    if let Some(source) = &request.provenance {
        detail.push(
            "Source",
            if reused(view) {
                format!(
                    "reused from Run {} request {} ({}, {}) completed {}{}",
                    source.run_id,
                    source.request_id,
                    source.eval_id,
                    source.repo_path.display(),
                    source
                        .completed_at
                        .map_or_else(|| "?".to_owned(), |time| time.to_string()),
                    request
                        .producer
                        .as_ref()
                        .map_or(String::new(), |producer| format!(" by {}", producer.name))
                )
            } else {
                "executed in this Run".into()
            },
        );
    }
    if let Some(execution) = &view.execution {
        let owner = execution.owner_pid;
        detail.push(
            "Execution",
            format!("{} {} · owner pid {owner}", execution.id, execution.status),
        );
    }
    if request.profile.kind() == crate::config::ProfileKind::Human || view.claim.is_some() {
        detail.push("Claim", claim(view));
    }
    detail.push(
        "Timing",
        format!(
            "created {} · started {} · completed {}{}",
            request.created_at,
            request
                .started_at
                .map_or_else(|| "-".to_owned(), |time| time.to_string()),
            request
                .completed_at
                .map_or_else(|| "-".to_owned(), |time| time.to_string()),
            elapsed(view, now).map_or(String::new(), |time| format!(" · {time}"))
        ),
    );
    detail.push("Deps", request.deps.join(", "));
    detail.push(
        "Argv",
        request
            .argv
            .as_ref()
            .map_or(String::new(), |argv| argv.join(" ")),
    );
    if let Some(result) = &request.result {
        detail.push("Result", result_summary(result));
        detail.push(
            "Raw result",
            serde_json::to_string_pretty(result).unwrap_or_default(),
        );
    }
    let total = pairs(&summary["usage"]);
    let attempts = request.usage.as_ref().or(request.reused_usage.as_ref());
    let attempts = attempts.map_or(&[][..], Vec::as_slice);
    let original = attempts.len();
    let attempts = attempts.iter().map(|attempt| {
        let error = attempt.error.as_deref();
        let error = error.map_or(String::new(), |error| format!(" error: {error}"));
        format!(
            "turn {} attempt {}: {}{error}",
            attempt.turn,
            attempt.attempt,
            pairs(&serde_json::to_value(&attempt.usage).expect("usage is JSON"))
        )
    });
    let state = if reused(view) {
        format!("reused: spent none · original attempts {original}")
    } else {
        format!(
            "{} · attempts {}{}",
            summary["usageState"].as_str().unwrap_or("unreported"),
            summary["attempts"],
            if total.is_empty() {
                total
            } else {
                format!(" · total {total}")
            }
        )
    };
    detail.push("Usage", join(std::iter::once(state).chain(attempts), "\n"));
    detail
}

pub fn detail(
    run: &RunView,
    requests: &[RequestView],
    target: &Target,
    now: OffsetDateTime,
) -> Detail {
    let saved = Saved { run, requests };
    let mut detail = Detail::default();
    match target {
        Target::Run => {
            let progress = progress(run, requests, now);
            let strip = strip(&progress);
            detail.title = format!("Run {}", run.run.id);
            detail.summary = format!("{} · {}", progress.status, strip.headline);
            detail.push("Status", progress.status.as_str());
            detail.push("Validation", progress.validation.as_str());
            detail.push(
                "Counts",
                join(
                    progress
                        .counts
                        .iter()
                        .filter(|(_, n)| *n > 0)
                        .map(|(status, n)| format!("{} {status} {n}", glyph(Some(status)))),
                    " · ",
                ),
            );
            let lines = |items: &[(String, String)]| {
                join(
                    items.iter().map(|(eval, text)| format!("{eval}  {text}")),
                    "\n",
                )
            };
            detail.push("Errors", lines(&progress.errors));
            detail.push("Waiting", lines(&progress.waiting));
            detail.push("Running", lines(&progress.running));
            detail.push("Timing", progress.timing.as_str());
            detail.push("Budget", progress.budget.as_str());
            detail.push("Work", progress.work.as_str());
            detail.push("Usage", progress.usage.as_str());
            detail.push("Saved usage", progress.saved.as_str());
            detail.push("Run", run.run.id.as_str());
            detail.push("Repository", progress.repo.as_str());
        }
        Target::Eval(id) => {
            let waits = tree::waits_for(&saved, id, now);
            if let Some(view) = saved.request(id) {
                let mut detail = request_detail(view, now);
                detail.insert_after("Status", "Waits for", waits);
                return detail;
            }
            let declaration = saved
                .eval_definitions()
                .iter()
                .find(|eval| &eval.id == id)
                .and_then(|eval| eval.declaration.value());
            detail.title = format!(
                "{id} · {}",
                declaration
                    .and_then(|declaration| declaration.title.value())
                    .map_or("", String::as_str)
            );
            detail.push("Status", "not in this Run (outside the selection)");
            detail.push("Waits for", waits);
            detail.push(
                "Instruction",
                declaration
                    .and_then(|declaration| declaration.payload.value())
                    .map_or("", |payload| payload.instruction()),
            );
            detail.push(
                "Profile",
                declaration
                    .and_then(|declaration| declaration.profile.value())
                    .map_or_else(|| "unknown".into(), |saved| profile(&saved.known)),
            );
        }
        Target::Artifact(id) => {
            let artifact = saved.artifact(id);
            let basis = if artifact.and_then(|artifact| artifact.basis.value()) == Some(&true) {
                " [basis]"
            } else {
                ""
            };
            let kind = if artifact.and_then(|artifact| artifact.kind.value())
                == Some(&crate::config::ArtifactKind::File)
            {
                " [file]"
            } else {
                ""
            };
            detail.title = format!("Artifact {id}{basis}{kind}");
            detail.push(
                "Tags",
                artifact
                    .and_then(|artifact| artifact.tags.value())
                    .map_or_else(String::new, |tags| tags.join(", ")),
            );
            detail.summary = tree::artifact_status(&saved, id, now);
            detail.push("Status", detail.summary.clone());
            let snapshot = saved.validation(id);
            if let Some(status) = snapshot.and_then(|s| s.status.value()) {
                detail.push(
                    "At Run end",
                    format!(
                        "{status} · {}/{} Evals GREEN",
                        snapshot
                            .and_then(|s| s.passed.value())
                            .copied()
                            .unwrap_or(0),
                        snapshot.and_then(|s| s.total.value()).copied().unwrap_or(0)
                    ),
                );
            }
            detail.push(
                "Path",
                artifact
                    .and_then(|artifact| artifact.path.value())
                    .map_or_else(
                        || "-".into(),
                        |path| {
                            if path.as_os_str().is_empty() {
                                ".".into()
                            } else {
                                path.display().to_string()
                            }
                        },
                    ),
            );
            detail.push(
                "Fingerprint",
                saved
                    .validation(id)
                    .and_then(|s| s.fingerprint.value())
                    .map(|f| f.as_str())
                    .unwrap_or_default(),
            );
            let component = saved.component(id);
            if component.and_then(|component| component.cyclic.value()) == Some(&true) {
                detail.push(
                    "Cycle",
                    format!(
                        "↻ {}",
                        join(
                            component
                                .and_then(|component| component.artifacts.value())
                                .into_iter()
                                .flatten(),
                            ", "
                        )
                    ),
                );
            }
            let gates = component
                .and_then(|component| component.gates.value())
                .into_iter()
                .flatten()
                .map(|gate| mark(gate, saved.status(gate)));
            detail.push("Gates", join(gates, "\n"));
            let evals = saved.evals(id).into_iter();
            detail.push(
                "Evals",
                join(evals.map(|(eval, _)| mark(eval, saved.status(eval))), "\n"),
            );
            for (key, input) in [("Inputs", true), ("Used by", false)] {
                let relations = saved.relations(id, input).into_iter();
                let relations = relations.map(|relation| {
                    format!(
                        "{} — {}",
                        if input {
                            &relation.source
                        } else {
                            &relation.target
                        },
                        relation_kind(relation)
                    )
                });
                detail.push(key, join(relations, "\n"));
            }
        }
    }
    detail
}
