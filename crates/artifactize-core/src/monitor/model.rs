//! Pure projections of saved state for the monitor screens.
mod detail;
pub(crate) use detail::raw;
pub use detail::{Card, Detail, Section, card, detail};
mod graph;
pub use graph::{Graph, GraphEdge, GraphNode, Light, graph};
mod node_id;
pub use node_id::NodeId;
mod states;
mod status;
#[cfg(test)]
mod tests;
pub(crate) mod tree;
pub use states::{Activity, Busy, Completion, EvalView, NotRun, Queue, Source, Upstream, Waits};
pub use status::Status;
pub use tree::{Clock, Kind, Node, Segment, Tally, Tone, Weight, completion, tree, upstream_index};

use time::OffsetDateTime;

use crate::{
    query,
    store::{RequestView, RunSummary, RunView},
    types::{ArtifactName, EvalId},
};

/// What a tree node shows in the detail pane, parsed from its stable tree identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Target {
    /// The open Run itself, from the node above its Artifacts.
    Run,
    Artifact(crate::types::ArtifactName),
    Eval(crate::types::EvalId),
}

impl Target {
    pub fn parse(id: &str) -> Option<Self> {
        let (kind, name) = id.split_once(':')?;
        match kind {
            "run" => name.parse::<crate::types::RunId>().ok().map(|_| Self::Run),
            "a" => name.parse().ok().map(Self::Artifact),
            "e" => name.parse().ok().map(Self::Eval),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRow {
    pub id: crate::types::RunId,
    pub repo: std::path::PathBuf,
    pub status: crate::types::RunStatus,
    /// Request status glyphs with their counts, most urgent first, such as `✗1 ✓2`.
    pub counts: String,
    pub age: String,
    /// Wall time of a finished Run.
    pub took: String,
    /// What the requester said it is working on, or empty.
    pub comment: String,
}

#[derive(Debug, Clone)]
pub struct Progress {
    pub status: Status,
    pub repo: String,
    /// What the requester said it is working on (`verify --comment`).
    pub comment: Option<String>,
    pub validation: String,
    pub timing: String,
    /// Elapsed time of a running Run, or how long a finished one took.
    pub elapsed: String,
    /// The saved validation snapshot: `None` until the Run ends.
    pub satisfied: Option<bool>,
    pub counts: Vec<(Status, u64)>,
    /// Executions against their budget, and jobs.
    pub budget: String,
    /// Executed and reused requests.
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
    /// The evals that need a person's attention, each by its status and ID: RED, then ERROR,
    /// then the sign-offs and Human reviews that wait.
    pub needs: Vec<(Status, String)>,
}

/// The Run headline: one status line, then what needs attention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Strip {
    pub status: Status,
    pub headline: String,
    /// Status and text: errors, then waiting Human reviews, then running evals.
    pub attention: Vec<(Status, String)>,
    /// Attention lines beyond the shown ones.
    pub more: usize,
}

/// Attention lines shown in the Run headline before `+N more`.
const ATTENTION_LINES: usize = 3;

const ABSENT: &str = "not in Run";

/// The mark for `status`, or `-` for an eval without one.
pub fn glyph(status: Option<Status>) -> &'static str {
    status.map_or("-", Status::glyph)
}

fn join<T: AsRef<str>>(items: impl IntoIterator<Item = T>, separator: &str) -> String {
    let items: Vec<_> = items
        .into_iter()
        .map(|item| item.as_ref().to_owned())
        .collect();
    items.join(separator)
}

fn mark(name: &EvalId, status: Option<Status>) -> String {
    format!(
        "{} {name} {}",
        glyph(status),
        status.map_or(ABSENT, Status::as_str)
    )
}

/// Seconds per minute, hour and day, the units a duration is shown in.
const MINUTE: i64 = 60;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;

pub fn duration(elapsed: time::Duration) -> String {
    let seconds = elapsed.whole_seconds().max(0);
    match seconds {
        0..MINUTE => format!("{seconds}s"),
        MINUTE..HOUR => format!("{}m {:02}s", seconds / MINUTE, seconds % MINUTE),
        HOUR..DAY => format!("{}h {:02}m", seconds / HOUR, seconds % HOUR / MINUTE),
        _ => format!("{}d {}h", seconds / DAY, seconds % DAY / HOUR),
    }
}

#[cfg(test)]
#[test]
fn typed_durations_keep_the_display_units_and_clamp_clock_skew() {
    for (elapsed, expected) in [
        (time::Duration::seconds(-1), "0s"),
        (time::Duration::ZERO, "0s"),
        (time::Duration::milliseconds(59_999), "59s"),
        (time::Duration::minutes(1), "1m 00s"),
        (time::Duration::seconds(125), "2m 05s"),
        (time::Duration::hours(1), "1h 00m"),
        (time::Duration::minutes(61), "1h 01m"),
        (time::Duration::days(1), "1d 0h"),
        (time::Duration::hours(25), "1d 1h"),
    ] {
        assert_eq!(duration(elapsed), expected);
    }
}

/// The tokens a Request's saved attempts spent, `None` for none. The counts are
/// read from the state, so a sum past `u64::MAX` saturates instead of panicking.
pub(crate) fn attempt_tokens(attempts: &[crate::llm::Attempt]) -> Option<u64> {
    let tokens = attempts
        .iter()
        .filter_map(|attempt| attempt.usage.get("totalTokens"))
        .fold(0u64, |total, tokens| total.saturating_add(*tokens));
    (tokens > 0).then_some(tokens)
}

#[test]
fn attempt_tokens_saturate_on_saved_counts() {
    let attempt = |tokens: u64| -> crate::llm::Attempt {
        serde_json::from_value(serde_json::json!({ "usage": { "totalTokens": tokens } }))
            .expect("a saved attempt")
    };
    assert_eq!(attempt_tokens(&[]), None);
    assert_eq!(attempt_tokens(&[attempt(0)]), None);
    assert_eq!(attempt_tokens(&[attempt(14), attempt(8)]), Some(22));
    assert_eq!(
        attempt_tokens(&[attempt(u64::MAX), attempt(1)]),
        Some(u64::MAX)
    );
}

/// Saved RFC 3339 times; an open end is measured to `now`.
pub(crate) fn span(
    start: crate::types::Timestamp,
    end: Option<crate::types::Timestamp>,
    now: OffsetDateTime,
) -> Option<String> {
    let end = end.map_or(now, crate::types::Timestamp::time);
    Some(duration(end - start.time()))
}

/// Status counts as glyphs, most urgent first and zero counts left out: `!2 ✓1`.
pub fn counts(counts: impl IntoIterator<Item = (Status, u64)>) -> String {
    let mut counts: Vec<_> = counts.into_iter().filter(|(_, n)| *n > 0).collect();
    counts.sort_by_key(|(status, _)| *status);
    join(
        counts
            .into_iter()
            .map(|(status, n)| format!("{}{n}", status.glyph())),
        " ",
    )
}

/// Token totals for a headline: `950`, `12.3k`, `3.9M`.
pub fn tokens(total: u64) -> String {
    // Headlines abbreviate decimal thousands and millions as k and M, respectively.
    const THOUSAND: u64 = 1_000;
    const MILLION: u64 = 1_000_000;
    match total {
        0..THOUSAND => total.to_string(),
        THOUSAND..MILLION => format!("{:.1}k", total as f64 / THOUSAND as f64),
        _ => format!("{:.1}M", total as f64 / MILLION as f64),
    }
}

pub fn run_rows(runs: &[RunSummary], now: OffsetDateTime) -> Vec<RunRow> {
    runs.iter()
        .map(|run| RunRow {
            id: run.id.clone(),
            repo: run.repo_path.clone(),
            status: run.status,
            counts: counts(run.counts.iter().map(|(status, n)| ((*status).into(), *n))),
            age: span(run.created_at, None, now).unwrap_or_default(),
            took: run
                .completed_at
                .and_then(|end| span(run.created_at, Some(end), now))
                .unwrap_or_default(),
            comment: run.comment.as_deref().unwrap_or_default().to_owned(),
        })
        .collect()
}

fn reused(view: &RequestView) -> bool {
    query::reused(&view.request)
}

fn elapsed(view: &RequestView, now: OffsetDateTime) -> Option<String> {
    let request = &view.request;
    if reused(view) {
        return Some("reused".into());
    }
    let active = matches!(
        request.status(),
        crate::types::RequestStatus::Running
            | crate::types::RequestStatus::WaitingHuman
            | crate::types::RequestStatus::Deferred
    );
    let start = request
        .started_at
        .or(active.then_some(request.created_at))?;
    span(start, request.completed_at(), now)
}

fn claim(view: &RequestView) -> String {
    view.claim.as_ref().map_or("unclaimed".into(), |claim| {
        format!("claimed by {} at {}", claim.reviewer, claim.claimed_at)
    })
}

fn error(view: &RequestView) -> Option<String> {
    let request = &view.request;
    let error = request.error().map(str::to_owned)?;
    Some(match &request.error_code() {
        Some(code) => format!("[{code}] {error}"),
        None => error,
    })
}

pub fn progress(view: &RunView, requests: &[RequestView], now: OffsetDateTime) -> Progress {
    let run = &view.run;
    let (summary, saved_usage) = query::run_summary(view, now);
    let with = |status: crate::types::RequestStatus| {
        requests
            .iter()
            .filter(move |view| view.request.status() == status)
    };
    let usage = counter_pairs(&summary.usage.usage);
    let saved = counter_pairs(&saved_usage);
    let spent = &summary.usage.usage;
    let tokens = spent.get("totalTokens").copied().or_else(|| {
        let input = spent.get("inputTokens").copied();
        let output = spent.get("outputTokens").copied();
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
    let wall = span(run.created_at, run.completed_at(), now).unwrap_or_default();
    Progress {
        status: run.status().into(),
        repo: crate::platform::path_text(&run.repo_path),
        comment: run.comment.as_ref().map(ToString::to_string),
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
            if run.completed_at().is_some() {
                "took"
            } else {
                "elapsed"
            },
        ),
        elapsed: wall,
        satisfied: validation.and_then(|v| v.satisfied.value()).copied(),
        counts: summary
            .counts
            .iter()
            .map(|(status, count)| ((*status).into(), *count))
            .collect(),
        budget: format!(
            "executions {}/{} · jobs {}",
            run.executions_started,
            run.max_executions
                .map_or("unlimited".into(), |max| max.to_string()),
            run.jobs
                .map_or("unlimited".into(), |limit| limit.to_string()),
        ),
        work: format!(
            "executed {} · reused {}",
            summary.executed.total, summary.reused.kinds.total,
        ),
        usage,
        saved,
        tokens,
        running: with(crate::types::RequestStatus::Running)
            .map(|view| {
                (
                    view.request.eval_id.to_string(),
                    elapsed(view, now).unwrap_or_default(),
                )
            })
            .collect(),
        waiting: requests
            .iter()
            .filter(|view| view.request.status().is_human_wait())
            .map(|view| (view.request.eval_id.to_string(), claim(view)))
            .collect(),
        errors: with(crate::types::RequestStatus::Error)
            .map(|view| {
                (
                    view.request.eval_id.to_string(),
                    error(view).unwrap_or_default(),
                )
            })
            .chain(
                run.error()
                    .map(str::to_owned)
                    .map(|error| ("Run".into(), error)),
            )
            .chain(
                view.unreadable
                    .iter()
                    .map(|record| (record.id.clone(), record.to_string())),
            )
            .collect(),
        needs: with(crate::types::RequestStatus::Red)
            .chain(with(crate::types::RequestStatus::Error))
            .chain(
                requests
                    .iter()
                    .filter(|view| view.request.status().is_human_wait()),
            )
            .map(|view| {
                (
                    Status::from(view.request.status()),
                    view.request.eval_id.to_string(),
                )
            })
            .collect(),
    }
}

impl Progress {
    pub fn finished(&self) -> bool {
        !matches!(self.status, Status::Running | Status::WaitingHuman)
    }
}

/// The Run headline from its progress: snapshot verdict, counts, time and token total.
pub fn strip(progress: &Progress) -> Strip {
    let validation = match progress.satisfied {
        None => "validation pending".to_owned(),
        Some(true) => "SATISFIED at Run end".to_owned(),
        Some(false) => "NOT SATISFIED at Run end".to_owned(),
    };
    let counts = counts(progress.counts.iter().copied());
    let time = format!(
        "{} {}",
        if progress.finished() {
            "took"
        } else {
            "elapsed"
        },
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
    let mut attention: Vec<(Status, String)> = progress
        .errors
        .iter()
        .map(|(eval, error)| (Status::Error, format!("{eval}  {error}")))
        .chain(progress.waiting.iter().map(|(eval, claim)| {
            (
                Status::WaitingHuman,
                format!("{eval}  waiting Human · {claim}"),
            )
        }))
        .chain(progress.running.iter().map(|(eval, time)| {
            (
                Status::Running,
                format!("{eval}  running {time}").trim_end().to_owned(),
            )
        }))
        .collect();
    let more = attention.len().saturating_sub(ATTENTION_LINES);
    attention.truncate(ATTENTION_LINES);
    Strip {
        status: progress.status,
        headline,
        attention,
        more,
    }
}

/// The row above the Artifacts; Enter on it opens the Run detail.
pub fn run_node(run: &RunView) -> Node {
    let status = Status::from(run.run.status());
    Node {
        id: NodeId::run(&run.run.id),
        clock: None,
        kind: Kind::Artifact {
            completion: Completion::Complete,
            basis: false,
        },
        glyph: status.glyph(),
        tone: match status {
            Status::Green => Tone::Green,
            Status::Red => Tone::Red,
            Status::Error => Tone::Error,
            Status::Running => Tone::Running,
            Status::Blocked | Status::BudgetExhausted | Status::Incomplete => Tone::Blocked,
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
        tally: None,
        changed: false,
        upstream: Vec::new(),
        held: false,
        parts: Vec::new(),
        children: Vec::new(),
    }
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
    fn artifact(&self, id: &ArtifactName) -> Option<&'a crate::store::definitions::Artifact> {
        self.definitions().and_then(|graph| graph.artifact(id))
    }
    fn eval_definitions(&self) -> &'a [crate::store::definitions::Eval] {
        self.definitions().map_or(&[], |graph| graph.evals())
    }
    fn components(&self) -> &'a [crate::store::definitions::Component] {
        self.definitions().map_or(&[], |graph| graph.components())
    }

    fn request(&self, eval: &EvalId) -> Option<&'a RequestView> {
        self.requests
            .iter()
            .find(|view| &view.request.eval_id == eval)
    }

    fn status(&self, eval: &EvalId) -> Option<Status> {
        self.request(eval).map(|view| view.request.status().into())
    }

    /// Dependency-first component order with cycle peers by name, then any other saved or
    /// requested Artifact.
    fn artifact_ids(&self) -> Vec<&'a ArtifactName> {
        let components = self.components().iter();
        let saved = components
            .flat_map(|component| {
                let mut members: Vec<_> =
                    component.artifacts.value().into_iter().flatten().collect();
                members.sort_unstable();
                members
            })
            .chain(self.artifacts().map(|(id, _)| id))
            .chain(self.requests.iter().map(|view| &view.request.target));
        let mut seen = std::collections::HashSet::new();
        saved.filter(|id| seen.insert(*id)).collect()
    }

    /// Saved Eval definitions targeting the Artifact, then requests without one.
    fn evals(
        &self,
        artifact: &ArtifactName,
    ) -> Vec<(&'a EvalId, Option<&'a crate::store::definitions::Eval>)> {
        let mut evals: Vec<_> = self
            .eval_definitions()
            .iter()
            .filter(|eval| &eval.target == artifact)
            .map(|eval| (&eval.id, Some(eval)))
            .collect();
        for request in self.requests.iter().map(|view| &view.request) {
            if &request.target == artifact && !evals.iter().any(|(id, _)| *id == &request.eval_id) {
                evals.push((&request.eval_id, None));
            }
        }
        evals
    }

    fn validation(&self, artifact: &ArtifactName) -> Option<&'a crate::store::ArtifactValidation> {
        self.run
            .run
            .validation
            .snapshot()?
            .artifacts
            .value()?
            .iter()
            .find(|a| &a.id == artifact)
    }

    fn component(&self, id: &ArtifactName) -> Option<&'a crate::store::definitions::Component> {
        self.components().iter().find(|component| {
            component
                .artifacts
                .value()
                .is_some_and(|artifacts| artifacts.iter().any(|artifact| artifact == id))
        })
    }

    /// Each Artifact with its direct parts.
    fn parts(&self) -> std::collections::BTreeMap<&'a ArtifactName, Vec<&'a ArtifactName>> {
        let mut parts: std::collections::BTreeMap<_, Vec<_>> = Default::default();
        for relation in self
            .definitions()
            .into_iter()
            .flat_map(|graph| graph.relations())
        {
            if matches!(relation.kind, crate::store::definitions::RelationKind::Part) {
                parts
                    .entry(&relation.target)
                    .or_default()
                    .push(&relation.source);
            }
        }
        parts
    }

    fn relations(
        &self,
        artifact: &ArtifactName,
        input: bool,
    ) -> Vec<&'a crate::store::definitions::Relation> {
        self.definitions()
            .into_iter()
            .flat_map(|graph| graph.relations())
            .filter(|relation| {
                if input {
                    &relation.target == artifact
                } else {
                    &relation.source == artifact
                }
            })
            .collect()
    }
}

fn relation_kind(relation: &crate::store::definitions::Relation) -> String {
    use crate::store::definitions::RelationKind;
    let kind = match &relation.kind {
        RelationKind::Unknown { .. } => serde_json::to_string(relation).unwrap_or_default(),
        RelationKind::Part => "part".into(),
        RelationKind::Child { path } => format!(
            "child {}",
            path.value().map_or("", crate::config::ChildPrefix::as_str)
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

fn counter_pairs(counters: &std::collections::BTreeMap<String, u64>) -> String {
    join(
        counters
            .iter()
            .map(|(name, value)| format!("{name} {value}")),
        ", ",
    )
}

#[cfg(any(test, feature = "test-support"))]
pub fn derivation_count(run: &RunView, requests: &[RequestView]) -> usize {
    states::States::new(&Saved { run, requests })
        .derivations
        .get()
}
