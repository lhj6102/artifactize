//! The Artifacts and evals tree: one row per eval, Artifact rows rolled up from them.
use serde_json::Value;
use time::OffsetDateTime;

use super::{
    Saved,
    states::{
        Activity, Busy, Completion, EvalView, NotRun, Queue, Source, States, Upstream, Waits,
    },
};
use crate::store::{RequestView, RunView};

/// Colour classes; the renderer maps them to terminal colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Green,
    Red,
    Error,
    Running,
    Human,
    Queued,
    Blocked,
    Muted,
}

/// In-progress rows are bold; rows that wait or did not run are dimmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weight {
    Bold,
    Normal,
    Dim,
}

/// Status text; a toned segment (an X token) keeps its colour inside a dimmed row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub text: String,
    pub tone: Option<Tone>,
}

impl Segment {
    fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            tone: None,
        }
    }
    fn toned(text: impl Into<String>, tone: Tone) -> Self {
        Self {
            text: text.into(),
            tone: Some(tone),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// An Artifact rolled up from its eval rows; basis Artifacts are complete.
    Artifact {
        completion: Completion,
        basis: bool,
    },
    Eval(EvalView),
}

#[derive(Debug, Clone)]
pub struct Node {
    /// Stable tree identifier: `a:<artifact>` or `e:<eval>`.
    pub id: super::NodeId,
    pub kind: Kind,
    pub glyph: &'static str,
    pub tone: Tone,
    pub weight: Weight,
    pub name: String,
    /// Dim markers after the name: `↻ peer`, `[file]`, `[dep]`.
    pub marks: String,
    pub text: Vec<Segment>,
    /// Right column: elapsed time for evals, `passed/total` for Artifacts.
    pub right: String,
    /// Right column of the compact tree: elapsed time, `←X`/`←N`, or `passed/total`.
    pub compact: String,
    /// The state changed after the Run ended (rendered as a dim `*`).
    pub changed: bool,
    /// Eval rows: every dependency Artifact, unmet ones first in X order.
    pub upstream: Vec<Upstream>,
    /// Timed eval rows, so a cached tree can redraw its elapsed times.
    pub clock: Option<Clock>,
    pub children: Vec<Node>,
}

/// When a timed eval started and, once settled, ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clock {
    pub start: OffsetDateTime,
    pub end: Option<OffsetDateTime>,
}

impl Clock {
    pub fn elapsed(self, now: OffsetDateTime) -> String {
        super::duration((self.end.unwrap_or(now) - self.start).whole_seconds())
    }
}

impl Node {
    /// The right column (or the compact one) as of `now`.
    pub fn right_at(&self, now: OffsetDateTime, compact: bool) -> String {
        match self.clock {
            Some(clock) => clock.elapsed(now),
            None if compact => self.compact.clone(),
            None => self.right.clone(),
        }
    }

    /// The status text without styling.
    pub fn status(&self) -> String {
        self.text
            .iter()
            .map(|segment| segment.text.as_str())
            .collect()
    }

    /// The whole row as plain text, for tests and logs.
    pub fn line(&self) -> String {
        let mut line = format!("{} {}{}", self.glyph, self.name, self.marks);
        for part in [self.status(), self.right.clone()] {
            if !part.is_empty() {
                line = format!("{line}  {part}");
            }
        }
        if self.changed {
            line.push_str(" *");
        }
        line
    }

    /// An Artifact whose evals are all done; folded by default.
    pub fn done(&self) -> bool {
        matches!(
            self.kind,
            Kind::Artifact {
                completion: Completion::Complete,
                ..
            }
        ) && !self.children.is_empty()
    }

    /// An eval row that is in progress or failed: where the cursor starts.
    pub fn attention(&self) -> bool {
        matches!(
            self.kind,
            Kind::Eval(EvalView::InProgress(_) | EvalView::Failed { .. })
        )
    }
}

/// Glyph, colour and word of an Artifact's completion where another row names it.
pub fn completion(completion: Completion) -> (&'static str, Tone, &'static str) {
    match completion {
        Completion::Complete => ("✓", Tone::Green, "complete"),
        Completion::InProgress(Busy::Running) => ("◐", Tone::Running, "in progress"),
        Completion::InProgress(Busy::Human) => ("?", Tone::Human, "in progress"),
        Completion::InProgress(Busy::Queued) => ("·", Tone::Queued, "in progress"),
        Completion::Waiting { blocked: false } => ("…", Tone::Muted, "waiting"),
        Completion::Waiting { blocked: true } => ("⊘", Tone::Blocked, "blocked"),
        Completion::Failed { verdict: true } => ("✗", Tone::Red, "failed"),
        Completion::Failed { verdict: false } => ("!", Tone::Error, "failed"),
    }
}

/// Glyph, colour and weight of one eval row.
fn style(view: &EvalView) -> (&'static str, Tone, Weight) {
    match view {
        EvalView::Done(_) => ("✓", Tone::Green, Weight::Normal),
        EvalView::Failed { verdict: true } => ("✗", Tone::Red, Weight::Normal),
        EvalView::Failed { verdict: false } => ("!", Tone::Error, Weight::Normal),
        EvalView::InProgress(Activity::Running) => ("◐", Tone::Running, Weight::Bold),
        EvalView::InProgress(Activity::Human(_)) => ("?", Tone::Human, Weight::Bold),
        EvalView::InProgress(Activity::Queued(_)) => ("·", Tone::Queued, Weight::Normal),
        EvalView::WaitingOn(_) => ("…", Tone::Muted, Weight::Dim),
        EvalView::BlockedBy(_) => ("⊘", Tone::Blocked, Weight::Normal),
        EvalView::NotRun(NotRun::Budget) => ("$", Tone::Blocked, Weight::Normal),
        EvalView::NotRun(NotRun::Absent) => ("-", Tone::Muted, Weight::Dim),
        EvalView::NotRun(_) => ("○", Tone::Muted, Weight::Dim),
    }
}

/// Shown X per row; the rest become `+N`.
const SHOWN: usize = 2;
/// A shown text field keeps this many characters of its first line, enough for a short value
/// or the start of a sentence while two fields still fit on one row.
const SHOWN_TEXT: usize = 24;

/// `cli ◐ in progress`, or `a ◐, b ? +1`; an ERROR-failed X asks for a retry.
fn tokens(states: &States, x: &[crate::types::ArtifactName], glyphs: bool) -> Vec<Segment> {
    let mut segments = Vec::new();
    for (index, id) in x.iter().take(SHOWN).enumerate() {
        if index > 0 {
            segments.push(Segment::plain(", "));
        }
        let (glyph, tone, word) = completion(states.completion(id));
        let text = match (glyphs, x.len()) {
            (false, _) => id.to_string(),
            (true, 1) => format!("{id} {glyph} {word}"),
            (true, _) => format!("{id} {glyph}"),
        };
        segments.push(Segment::toned(text, tone));
    }
    if x.len() > SHOWN {
        segments.push(Segment::plain(format!(" +{}", x.len() - SHOWN)));
    }
    if glyphs
        && let Some(failed) = x
            .iter()
            .find(|id| states.completion(id) == Completion::Failed { verdict: false })
    {
        segments.push(Segment::plain(format!(" (retry {failed} first)")));
    }
    segments
}

fn waits(states: &States, prefix: &str, waits: &Waits) -> Vec<Segment> {
    let mut segments = vec![Segment::plain(prefix)];
    segments.extend(tokens(states, &waits.x, true));
    if let Some((via, root)) = &waits.root {
        segments.push(Segment::plain(format!(" ({via} waits for ")));
        let (_, tone, _) = completion(states.completion(root));
        segments.push(Segment::toned(root.clone(), tone));
        segments.push(Segment::plain(")"));
    }
    segments
}

/// The top-level Result keys behind a RED verdict, such as `violations 3`.
fn findings(result: Option<&Value>) -> String {
    let fields = result.and_then(Value::as_object).into_iter().flatten();
    let fields = fields.filter(|(key, _)| {
        !matches!(
            key.as_str(),
            "verdict" | "stdout" | "stderr" | "truncated" | "derived" | "durationMs"
        )
    });
    let parts: Vec<_> = fields
        .take(SHOWN)
        .map(|(key, value)| match value {
            Value::Array(items) => format!("{key} {}", items.len()),
            Value::Object(fields) => format!("{key} {{{}}}", fields.len()),
            Value::String(text) => {
                let text = text.lines().next().unwrap_or_default();
                let mut short: String = text.chars().take(SHOWN_TEXT).collect();
                if short.len() < text.len() {
                    short.push('…');
                }
                format!("{key} {short}")
            }
            value => format!("{key} {value}"),
        })
        .collect();
    parts.join(", ")
}

fn short_profile(profile: &crate::config::StoredProfile) -> String {
    use crate::config::StoredProfile;
    match profile {
        StoredProfile::Agent { backend, model, .. } => format!(
            "agent {} {model}",
            serde_json::to_value(backend)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_default()
        ),
        StoredProfile::Runtime { command, .. } => format!("runtime {command}"),
        StoredProfile::Human {} => "human".into(),
        StoredProfile::Dependency { .. } => "dependency".into(),
    }
}

fn text(states: &States, view: &EvalView, request: Option<&RequestView>) -> Vec<Segment> {
    let plain = |text: &str| vec![Segment::plain(text)];
    match view {
        EvalView::Done(Source::Executed) => plain("GREEN"),
        EvalView::Done(Source::Reused) => plain("GREEN · reused"),
        EvalView::Done(Source::Derived) => plain("GREEN · derived"),
        EvalView::Done(Source::Saved) => plain("GREEN · saved result, not in this Run"),
        EvalView::Failed { verdict: true } => {
            let findings = findings(request.and_then(|view| view.request.result.as_ref()));
            if request.is_none() {
                plain("RED · saved result, not in this Run")
            } else if findings.is_empty() {
                plain("RED")
            } else {
                vec![Segment::plain(format!("RED · {findings}"))]
            }
        }
        EvalView::Failed { verdict: false } => match request.and_then(super::error) {
            Some(error) => vec![Segment::plain(format!(
                "ERROR · {}",
                error.lines().next().unwrap_or_default()
            ))],
            None if request.is_none() => plain("ERROR · saved result, not in this Run"),
            None => plain("ERROR"),
        },
        EvalView::InProgress(Activity::Running) => vec![Segment::plain(format!(
            "running · {}",
            request.map_or_else(String::new, |view| short_profile(&view.request.profile))
        ))],
        EvalView::InProgress(Activity::Human(None)) => plain("Human sign-off · unclaimed"),
        EvalView::InProgress(Activity::Human(Some(reviewer))) => vec![Segment::plain(format!(
            "Human sign-off · claimed by {reviewer}"
        ))],
        EvalView::InProgress(Activity::Queued(Queue::Jobs)) => plain("queued"),
        EvalView::InProgress(Activity::Queued(Queue::Slot(backend))) => {
            vec![Segment::plain(format!("queued · {backend} slots full"))]
        }
        EvalView::InProgress(Activity::Queued(Queue::Joined)) => {
            plain("queued · joins the running review")
        }
        EvalView::WaitingOn(x) => waits(states, "waits for ", x),
        EvalView::BlockedBy(x) => {
            let mut segments = vec![Segment::plain(if states.running {
                "blocked by "
            } else {
                "not run: blocked by "
            })];
            segments.extend(tokens(states, x, true));
            segments
        }
        EvalView::NotRun(NotRun::Dependency(x)) => waits(states, "not run: waited for ", x),
        EvalView::NotRun(NotRun::Budget) => plain("not started · maxExecutions reached"),
        EvalView::NotRun(NotRun::Stale) => plain("not run · no reusable result"),
        EvalView::NotRun(NotRun::Unreviewed) => plain("not reviewed"),
        EvalView::NotRun(NotRun::Absent) => plain("not in this Run"),
    }
}

/// Start and end of executed, failed, running and Human evals; an open end runs to now.
fn clock(view: &EvalView, request: Option<&RequestView>) -> Option<Clock> {
    let timed = matches!(
        view,
        EvalView::Done(Source::Executed)
            | EvalView::Failed { .. }
            | EvalView::InProgress(Activity::Running | Activity::Human(_))
    );
    let request = &request.filter(|_| timed)?.request;
    let start = request.started_at.unwrap_or(request.created_at).time();
    let end = request.completed_at.map(crate::types::Timestamp::time);
    Some(Clock { start, end })
}

fn compact(view: &EvalView, right: &str) -> String {
    let x = match view {
        EvalView::WaitingOn(waits) | EvalView::NotRun(NotRun::Dependency(waits)) => &waits.x,
        EvalView::BlockedBy(x) => x,
        _ => return right.to_owned(),
    };
    match x.as_slice() {
        [one] => format!("←{one}"),
        many => format!("←{}", many.len()),
    }
}

fn local(eval: &str) -> &str {
    eval.rsplit_once('/').map_or(eval, |(_, local)| local)
}

fn eval_node<'a>(states: &States<'a>, eval: &'a str, now: OffsetDateTime) -> Node {
    let view = states.view(eval);
    let request = states.request(eval);
    let (glyph, tone, weight) = style(&view);
    let clock = clock(&view, request);
    let right = clock.map_or_else(String::new, |clock| clock.elapsed(now));
    Node {
        clock,
        id: super::NodeId::eval(eval.parse().expect("saved Eval ID")),
        glyph,
        tone,
        weight,
        name: local(eval).to_owned(),
        marks: if states.index.dependency.contains(eval) {
            "  [dep]".into()
        } else {
            String::new()
        },
        text: text(states, &view, request),
        compact: compact(&view, &right),
        right,
        changed: false,
        upstream: states.upstream_states(eval),
        children: Vec::new(),
        kind: Kind::Eval(view),
    }
}

/// Up to two eval names, then `+N`.
fn names(nodes: &[&Node]) -> String {
    let mut names = nodes
        .iter()
        .take(SHOWN)
        .map(|node| node.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    if nodes.len() > SHOWN {
        names = format!("{names} +{}", nodes.len() - SHOWN);
    }
    names
}

/// One clause for the most urgent class of eval rows, plus in-progress ones after a failure.
fn summary(states: &States, id: &str, children: &[Node]) -> Vec<Segment> {
    let view = |node: &Node| match &node.kind {
        Kind::Eval(view) => Some(view.clone()),
        Kind::Artifact { .. } => None,
    };
    let Some(top) = children
        .iter()
        .filter_map(view)
        .map(|view| view.rank())
        .min()
    else {
        return Vec::new();
    };
    let class = |rank: u8| -> Vec<&Node> {
        children
            .iter()
            .filter(|node| view(node).is_some_and(|view| view.rank() == rank))
            .collect()
    };
    let x = |ids: Vec<crate::types::ArtifactName>| tokens(states, &ids, false);
    let mut segments = match top {
        0 => {
            let failed = class(0);
            let errors: Vec<_> = failed
                .iter()
                .copied()
                .filter(|node| node.kind == Kind::Eval(EvalView::Failed { verdict: false }))
                .collect();
            let (label, nodes) = if errors.is_empty() {
                ("RED", failed)
            } else {
                ("ERROR", errors)
            };
            vec![Segment::plain(format!("{label}: {}", names(&nodes)))]
        }
        1 => vec![Segment::plain(format!("in progress: {}", names(&class(1))))],
        2 => {
            let mut segments = vec![Segment::plain(if states.running {
                "blocked by "
            } else {
                "not run: blocked by "
            })];
            segments.extend(x(states.blockers_of(id)));
            segments
        }
        3 => {
            let mut segments = vec![Segment::plain("waits for ")];
            segments.extend(x(states.waits_of(id)));
            segments
        }
        4 => {
            let first = class(4)[0];
            match &first.kind {
                Kind::Eval(EvalView::NotRun(NotRun::Dependency(_))) => {
                    let mut segments = vec![Segment::plain("not run: waited for ")];
                    segments.extend(x(states.waits_of(id)));
                    segments
                }
                _ => first.text.clone(),
            }
        }
        // Every eval row is done, but graph.rs still holds them on their own gates.
        _ => match states.completion(id) {
            Completion::Waiting { blocked: true } => {
                let mut segments = vec![Segment::plain("done, but blocked by ")];
                segments.extend(x(states.blockers_of(id)));
                segments
            }
            Completion::Waiting { blocked: false } => {
                let mut segments = vec![Segment::plain("done, but waits for ")];
                segments.extend(x(states.waits_of(id)));
                segments
            }
            _ => Vec::new(),
        },
    };
    if top == 0 {
        let busy = class(1);
        if !busy.is_empty() {
            segments.push(Segment::plain(format!(" · in progress: {}", names(&busy))));
        }
    }
    segments
}

fn artifact_node<'a>(
    states: &States<'a>,
    saved: &Saved<'a>,
    id: &str,
    now: OffsetDateTime,
) -> Node {
    let artifact = saved.artifact(id);
    let basis = artifact.and_then(|artifact| artifact.basis.value()) == Some(&true);
    let children: Vec<_> = states
        .evals(id)
        .iter()
        .map(|&eval| eval_node(states, eval, now))
        .collect();
    let mut marks = String::new();
    if states.index.cyclic.contains(id) {
        let peers = states.index.peers.get(id).map_or(&[][..], Vec::as_slice);
        marks = match peers {
            [] => "  ↻".into(),
            [peer] => format!("  ↻ {peer}"),
            [peer, rest @ ..] => format!("  ↻ {peer} +{}", rest.len()),
        };
    }
    if artifact.and_then(|artifact| artifact.kind.value())
        == Some(&crate::config::ArtifactKind::File)
    {
        marks.push_str("  [file]");
    }
    let completion = states.completion(id);
    // GREEN as the graph counts it: a done eval held on its own gates does not pass.
    let passed = states
        .evals(id)
        .iter()
        .filter(|eval| states.effective(eval) == crate::graph::EvalStatus::Green)
        .count();
    let right = if children.is_empty() {
        String::new()
    } else {
        format!("{passed}/{}", children.len())
    };
    let (glyph, tone, weight, text) = if basis {
        ("◇", Tone::Muted, Weight::Dim, vec![Segment::plain("basis")])
    } else if children.is_empty() {
        (
            "○",
            Tone::Muted,
            Weight::Dim,
            vec![Segment::plain("no evals")],
        )
    } else {
        // The row takes the glyph of its most urgent eval row; ERROR before RED.
        let top = children
            .iter()
            .min_by_key(|node| match &node.kind {
                Kind::Eval(EvalView::Failed { verdict: false }) => (0, 0),
                Kind::Eval(view) => (view.rank(), 1),
                Kind::Artifact { .. } => (u8::MAX, 0),
            })
            .expect("children");
        let (glyph, tone, weight) = match (&top.kind, completion) {
            (_, Completion::InProgress(busy)) => {
                let (glyph, tone, _) = self::completion(Completion::InProgress(busy));
                (glyph, tone, Weight::Bold)
            }
            // Every eval row is done, yet the graph holds them on their own gates.
            (Kind::Eval(EvalView::Done(_)), Completion::Waiting { blocked }) => {
                let (glyph, tone, _) = self::completion(Completion::Waiting { blocked });
                let weight = if blocked { Weight::Normal } else { Weight::Dim };
                (glyph, tone, weight)
            }
            _ => (top.glyph, top.tone, top.weight),
        };
        (glyph, tone, weight, summary(states, id, &children))
    };
    Node {
        id: super::NodeId::artifact(id.parse().expect("saved Artifact ID")),
        clock: None,
        kind: Kind::Artifact { completion, basis },
        glyph,
        tone,
        weight,
        name: id.to_owned(),
        marks,
        text,
        compact: right.clone(),
        right,
        changed: false,
        upstream: Vec::new(),
        children,
    }
}

/// Every saved or requested Artifact appears exactly once, upstream first where the graph
/// allows: dependency-first component order, cycle peers by name.
pub fn tree(run: &RunView, requests: &[RequestView], now: OffsetDateTime) -> Vec<Node> {
    let mut nodes = build(run, requests, now);
    // A finished Run: derive the same snapshot once more as of its end and mark every row
    // whose derived state differs.
    if let Some(ended) = at_end(run, requests) {
        mark(&mut nodes, &build(run, &ended, now));
    }
    nodes
}

fn build(run: &RunView, requests: &[RequestView], now: OffsetDateTime) -> Vec<Node> {
    let saved = Saved { run, requests };
    let states = States::new(&saved);
    states
        .index
        .order
        .iter()
        .map(|id| artifact_node(&states, &saved, id, now))
        .collect()
}

/// The requests as they stood when the Run ended, or `None` when nothing changed since
/// (or the Run has not ended). A request that completed later goes back to waiting for its
/// Human (or running), and a later claim is dropped; the Run's recorded evidence is fixed.
fn at_end(run: &RunView, requests: &[RequestView]) -> Option<Vec<RequestView>> {
    let end = run.run.completed_at?;
    let late = |time: Option<crate::types::Timestamp>| time.is_some_and(|time| time > end);
    let changed = requests.iter().any(|view| {
        late(view.request.completed_at) || late(view.claim.as_ref().map(|claim| claim.claimed_at))
    });
    if !changed {
        return None;
    }
    let mut ended = requests.to_vec();
    for view in &mut ended {
        if late(view.claim.as_ref().map(|claim| claim.claimed_at)) {
            view.claim = None;
        }
        let request = &mut view.request;
        if late(request.completed_at) {
            request.status = if request.profile.kind() == crate::config::ProfileKind::Human {
                crate::types::RequestStatus::WaitingHuman
            } else {
                crate::types::RequestStatus::Running
            };
            request.completed_at = None;
            request.result = None;
            request.error = None;
            request.error_code = None;
        }
    }
    Some(ended)
}

/// The state a row derives, without the root-cause note (an explanation, not a state).
fn state(node: &Node) -> (Kind, &'static str, String) {
    let kind = match &node.kind {
        Kind::Eval(EvalView::WaitingOn(waits)) => Kind::Eval(EvalView::WaitingOn(Waits {
            x: waits.x.clone(),
            root: None,
        })),
        Kind::Eval(EvalView::NotRun(NotRun::Dependency(waits))) => {
            Kind::Eval(EvalView::NotRun(NotRun::Dependency(Waits {
                x: waits.x.clone(),
                root: None,
            })))
        }
        kind => kind.clone(),
    };
    // Artifact rows compare their whole summary; eval rows their state and glyph.
    let summary = match node.kind {
        Kind::Artifact { .. } => format!("{} {}", node.status(), node.right),
        Kind::Eval(_) => String::new(),
    };
    (kind, node.glyph, summary)
}

/// Mark the rows whose derived state differs from the Run-end derivation.
fn mark(nodes: &mut [Node], ended: &[Node]) {
    let mut before = std::collections::HashMap::new();
    for node in ended {
        before.insert(node.id.as_str(), state(node));
        for child in &node.children {
            before.insert(child.id.as_str(), state(child));
        }
    }
    for node in nodes {
        node.changed = before.get(node.id.as_str()) != Some(&state(node));
        for child in &mut node.children {
            child.changed = before.get(child.id.as_str()) != Some(&state(child));
        }
    }
}

/// Every eval's direct dependency Artifacts, from the Run's saved definitions.
pub fn upstream_index(
    run: &RunView,
    requests: &[RequestView],
) -> Vec<(String, Vec<crate::types::ArtifactName>)> {
    let saved = Saved { run, requests };
    let index = super::states::Index::new(&saved);
    index
        .upstream
        .iter()
        .map(|(eval, artifacts)| {
            (
                (*eval).to_owned(),
                artifacts
                    .iter()
                    .map(|id| id.parse().expect("saved Artifact ID"))
                    .collect(),
            )
        })
        .collect()
}

/// The Artifact detail's current status, rolled up like its tree row.
pub(super) fn artifact_status(saved: &Saved, id: &str, now: OffsetDateTime) -> String {
    let states = States::new(saved);
    let node = artifact_node(&states, saved, id, now);
    let (_, _, word) = completion(states.completion(id));
    let status = node.status();
    let status = if status.is_empty() {
        word.to_owned()
    } else {
        status
    };
    if node.right.is_empty() {
        format!("{} {status}", node.glyph)
    } else {
        format!("{} {status} · {} Evals GREEN", node.glyph, node.right)
    }
}

/// The eval detail's `Waits for`: every dependency Artifact in the tree's order, with its
/// completion and why the eval depends on it, then its evals that are not done yet.
pub(super) fn waits_for(saved: &Saved, eval: &str, now: OffsetDateTime) -> String {
    let states = States::new(saved);
    let Some((&target, _)) = states
        .index
        .evals
        .iter()
        .find(|(_, evals)| evals.contains(&eval))
    else {
        return String::new();
    };
    let peers = |id: &str| states.index.peers.get(id).cloned().unwrap_or_default();
    let mut own = peers(target);
    own.push(target);
    let mut lines = Vec::new();
    for upstream in states.upstream_states(eval) {
        let (glyph, _, word) = completion(upstream.completion);
        let id = upstream.artifact.as_str();
        let mut origins: Vec<String> = saved
            .relations(id, false)
            .into_iter()
            .filter(|relation| own.contains(&relation.target.as_str()))
            .map(|relation| {
                let kind = super::relation_kind(relation);
                if relation.target == target {
                    kind
                } else {
                    format!("{kind} → {}", relation.target)
                }
            })
            .collect();
        if origins.is_empty() && !peers(id).is_empty() {
            origins.push(format!("↻ peer of {}", peers(id).join(", ")));
        }
        let origins = if origins.is_empty() {
            String::new()
        } else {
            format!(" · {}", origins.join(", "))
        };
        lines.push(format!("↑ {id} {glyph} {word}{origins}"));
        if upstream.completion == Completion::Complete {
            continue;
        }
        for &pending in states.evals(id) {
            let node = eval_node(&states, pending, now);
            if !matches!(node.kind, Kind::Eval(EvalView::Done(_))) {
                lines.push(format!("  {} {}  {}", node.glyph, node.name, node.status()));
            }
        }
    }
    lines.join("\n")
}
