//! Pure projections of saved state for the monitor screens.

use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
    query,
    store::{RequestView, RunSummary, RunView},
};

/// What a tree node shows in the detail pane, parsed from its stable tree identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Family(String),
    Artifact(String),
    Eval(String),
}

impl Target {
    /// Relation nodes (`r:`) open the input Artifact they point to.
    pub fn parse(id: &str) -> Option<Self> {
        let (kind, name) = id.split_once(':')?;
        let name = name.to_owned();
        match kind {
            "f" => Some(Self::Family(name)),
            "a" | "r" => Some(Self::Artifact(name)),
            "e" => Some(Self::Eval(name)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRow {
    pub id: String,
    pub repo: String,
    pub status: String,
    pub counts: String,
    pub age: String,
}

#[derive(Debug, Clone, Default)]
pub struct Progress {
    pub status: String,
    pub repo: String,
    pub validation: String,
    pub timing: String,
    pub counts: Vec<(String, u64)>,
    pub work: String,
    /// Eval ID and elapsed time.
    pub running: Vec<(String, String)>,
    /// Eval ID and claim state.
    pub waiting: Vec<(String, String)>,
    /// Eval ID (or "Run") and message.
    pub errors: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub id: String,
    pub status: Option<String>,
    pub text: String,
    pub children: Vec<Node>,
}

#[derive(Debug, Clone, Default)]
pub struct Detail {
    pub title: String,
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
}

/// Most to least urgent, for family and live Artifact roll-ups.
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

fn rollup<'a>(statuses: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let rank = |status: &&str| URGENCY.split_whitespace().position(|s| s == *status);
    statuses.into_iter().min_by_key(rank).map(str::to_owned)
}

fn strs(value: &Value) -> impl Iterator<Item = &str> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
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

pub fn duration(seconds: i64) -> String {
    let seconds = seconds.max(0);
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m {:02}s", seconds / 60, seconds % 60),
        3600..86400 => format!("{}h {:02}m", seconds / 3600, seconds % 3600 / 60),
        _ => format!("{}d {}h", seconds / 86400, seconds % 86400 / 3600),
    }
}

/// Saved RFC 3339 times; an open end is measured to `now`.
fn span(start: &str, end: Option<&str>, now: OffsetDateTime) -> Option<String> {
    let parse = |time| OffsetDateTime::parse(time, &Rfc3339).ok();
    let end = end.map_or(Some(now), parse)?;
    Some(duration((end - parse(start)?).whole_seconds()))
}

pub fn run_rows(runs: &[RunSummary], now: OffsetDateTime) -> Vec<RunRow> {
    runs.iter()
        .map(|run| RunRow {
            id: run.id.to_string(),
            repo: run.repo_path.display().to_string(),
            status: run.status.to_string(),
            counts: join(
                run.counts.iter().map(|(status, n)| format!("{status} {n}")),
                "  ",
            ),
            age: span(&run.created_at, None, now).unwrap_or_default(),
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
    let active = matches!(request.status.as_str(), "RUNNING" | "WAITING_HUMAN");
    let start = request.started_at.as_deref();
    let start = start.or(active.then_some(request.created_at.as_str()))?;
    span(start, request.completed_at.as_deref(), now)
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
    let unmet = join(strs(&run.validation["obligations"]), ", ");
    Progress {
        status: run.status.to_string(),
        repo: run.repo_path.display().to_string(),
        validation: match run.validation.get("satisfied") {
            None => "pending".into(),
            Some(Value::Bool(true)) => "SATISFIED".into(),
            Some(_) if unmet.is_empty() => "NOT SATISFIED".into(),
            Some(_) => format!("NOT SATISFIED (unmet: {unmet})"),
        },
        timing: format!(
            "started {} · {} {}",
            run.created_at,
            if run.completed_at.is_some() {
                "took"
            } else {
                "elapsed"
            },
            span(&run.created_at, run.completed_at.as_deref(), now).unwrap_or_default()
        ),
        counts: summary["counts"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(status, count)| (status.clone(), count.as_u64().unwrap_or(0)))
            .collect(),
        work: format!(
            "executions {}/{} · jobs {} · executed {} · reused {}{}{}",
            run.executions_started,
            run.max_executions
                .map_or("unlimited".into(), |max| max.to_string()),
            run.jobs,
            summary["executed"]["total"],
            summary["reused"]["total"],
            if usage.is_empty() {
                usage
            } else {
                format!(" · usage {usage}")
            },
            if saved.is_empty() {
                saved
            } else {
                format!(" · saved {saved}")
            }
        ),
        running: with("RUNNING")
            .map(|view| {
                (
                    view.request.eval_id.clone(),
                    elapsed(view, now).unwrap_or_default(),
                )
            })
            .collect(),
        waiting: with("WAITING_HUMAN")
            .map(|view| (view.request.eval_id.clone(), claim(view)))
            .collect(),
        errors: with("ERROR")
            .map(|view| {
                (
                    view.request.eval_id.clone(),
                    error(view).unwrap_or_default(),
                )
            })
            .chain(run.error.clone().map(|error| ("Run".into(), error)))
            .collect(),
    }
}

/// Saved Run definitions joined with the Run's requests.
struct Saved<'a> {
    run: &'a RunView,
    requests: &'a [RequestView],
}

impl<'a> Saved<'a> {
    fn definitions(&self, key: &str) -> &'a Value {
        &self.run.run.definitions[key]
    }

    fn list(&self, key: &str) -> &'a [Value] {
        self.definitions(key).as_array().map_or(&[], Vec::as_slice)
    }

    fn request(&self, eval: &str) -> Option<&'a RequestView> {
        self.requests
            .iter()
            .find(|view| view.request.eval_id == eval)
    }

    fn status(&self, eval: &str) -> Option<&'a str> {
        self.request(eval).map(|view| view.request.status.as_str())
    }

    /// Dependency-first component order, then any other saved or requested Artifact.
    fn artifact_ids(&self) -> Vec<&'a str> {
        let components = self.list("components").iter();
        let saved = components
            .flat_map(|component| strs(&component["artifacts"]))
            .chain(
                self.definitions("artifacts")
                    .as_object()
                    .into_iter()
                    .flat_map(|a| a.keys().map(String::as_str)),
            )
            .chain(
                self.requests
                    .iter()
                    .map(|view| view.request.target.as_str()),
            );
        let mut ids = Vec::new();
        for id in saved {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        ids
    }

    /// Saved Eval definitions targeting the Artifact, then requests without one.
    fn evals(&self, artifact: &str) -> Vec<(&'a str, Option<&'a Value>)> {
        let mut evals: Vec<_> = self
            .list("evals")
            .iter()
            .filter(|eval| eval["target"] == artifact)
            .filter_map(|eval| Some((eval["id"].as_str()?, Some(eval))))
            .collect();
        for request in self.requests.iter().map(|view| &view.request) {
            if request.target == artifact && !evals.iter().any(|(id, _)| *id == request.eval_id) {
                evals.push((&request.eval_id, None));
            }
        }
        evals
    }

    fn validation(&self, artifact: &str) -> &'a Value {
        let saved = self.run.run.validation["artifacts"].as_array();
        let saved = saved.and_then(|artifacts| artifacts.iter().find(|a| a["id"] == artifact));
        saved.unwrap_or(&Value::Null)
    }

    /// Saved validation when the Run finished, otherwise a live roll-up of its requests.
    fn artifact_state(&self, id: &str) -> (Option<String>, u64, u64) {
        let saved = self.validation(id);
        if !saved.is_null() {
            let count = |key| saved[key].as_u64().unwrap_or(0);
            return (
                saved["status"].as_str().map(str::to_owned),
                count("passed"),
                count("total"),
            );
        }
        let evals = self.evals(id);
        let statuses: Vec<_> = evals.iter().filter_map(|(id, _)| self.status(id)).collect();
        let status = if self.definitions("artifacts")[id]["basis"] == true {
            Some("BASIS".into())
        } else if evals.is_empty() {
            Some("UNREVIEWED".into())
        } else {
            rollup(statuses.iter().copied())
        };
        let passed = statuses.iter().filter(|status| **status == "GREEN").count();
        (status, passed as u64, evals.len() as u64)
    }

    fn component(&self, id: &str) -> &'a Value {
        let mut components = self.list("components").iter();
        let found = components.find(|component| strs(&component["artifacts"]).any(|a| a == id));
        found.unwrap_or(&Value::Null)
    }

    fn family(&self, id: &str) -> Option<&'a str> {
        self.definitions("artifacts")[id]["family"]["name"].as_str()
    }

    fn family_members(&self, name: &str) -> Vec<&'a str> {
        let mut members: Vec<_> =
            strs(&self.definitions("families")[name]["artifactIds"]).collect();
        for id in self.artifact_ids() {
            if self.family(id) == Some(name) && !members.contains(&id) {
                members.push(id);
            }
        }
        members
    }

    fn relations(&self, artifact: &str, side: &str) -> Vec<&'a Value> {
        let relations = self.list("relations").iter();
        relations
            .filter(|relation| relation[side] == artifact)
            .collect()
    }
}

fn relation_kind(relation: &Value) -> String {
    let field = |key: &str| relation[key].as_str().unwrap_or_default();
    let kind = match field("kind") {
        "child" => format!("child {}", field("path")),
        "mount" => format!("mount {}", field("alias")),
        "instruction" => format!("{{{}}} in {}", field("name"), field("evalId")),
        "argv" => format!(
            "argv {}[{}] {{{}}}",
            field("evalId"),
            relation["index"],
            field("name")
        ),
        _ => relation.to_string(),
    };
    if relation["cyclic"] == true {
        format!("{kind} ↻")
    } else {
        kind
    }
}

fn artifact_node(saved: &Saved, id: &str, now: OffsetDateTime) -> Node {
    let (status, passed, total) = saved.artifact_state(id);
    let mut children: Vec<_> = saved
        .evals(id)
        .into_iter()
        .map(|(eval, definition)| {
            let request = saved.request(eval);
            let status = request.map(|view| view.request.status.to_string());
            let local = eval.rsplit_once('/').map_or(eval, |(_, local)| local);
            let mut text = format!("{local} {}", status.as_deref().unwrap_or(ABSENT));
            if let Some(time) = request.and_then(|view| elapsed(view, now)) {
                text = format!("{text} {time}");
            }
            let deps = match request {
                Some(view) => join(&view.request.deps, ", "),
                None => join(strs(&definition.unwrap_or(&Value::Null)["deps"]), ", "),
            };
            if !deps.is_empty() {
                text = format!("{text}  ← {deps}");
            }
            Node {
                id: format!("e:{eval}"),
                status,
                text,
                children: Vec::new(),
            }
        })
        .collect();
    let mut inputs: Vec<(&str, Vec<String>)> = Vec::new();
    for relation in saved.relations(id, "target") {
        let source = relation["source"].as_str().unwrap_or_default();
        match inputs.iter_mut().find(|(id, _)| *id == source) {
            Some((_, kinds)) => kinds.push(relation_kind(relation)),
            None => inputs.push((source, vec![relation_kind(relation)])),
        }
    }
    children.extend(inputs.into_iter().map(|(source, kinds)| Node {
        id: format!("r:{source}"),
        status: saved.artifact_state(source).0,
        text: format!("⇐ {source} ({})", kinds.join(", ")),
        children: Vec::new(),
    }));
    let cycle = if saved.component(id)["cyclic"] == true {
        " ↻"
    } else {
        ""
    };
    let text = format!(
        "{id}{cycle}  {} {passed}/{total}",
        status.as_deref().unwrap_or(ABSENT)
    );
    Node {
        id: format!("a:{id}"),
        status,
        text,
        children,
    }
}

/// Every saved or requested Artifact appears exactly once; family instances group under their family.
pub fn tree(run: &RunView, requests: &[RequestView], now: OffsetDateTime) -> Vec<Node> {
    let saved = Saved { run, requests };
    let mut nodes = Vec::new();
    let mut families = Vec::new();
    for id in saved.artifact_ids() {
        match saved.family(id) {
            Some(name) if families.contains(&name) => {}
            Some(name) => {
                families.push(name);
                let members = saved.family_members(name).into_iter();
                let children: Vec<_> = members.map(|id| artifact_node(&saved, id, now)).collect();
                let status = rollup(children.iter().filter_map(|node| node.status.as_deref()));
                let text = format!(
                    "family {name}  {} · {} instances",
                    status.as_deref().unwrap_or(ABSENT),
                    children.len()
                );
                nodes.push(Node {
                    id: format!("f:{name}"),
                    status,
                    text,
                    children,
                });
            }
            None => nodes.push(artifact_node(&saved, id, now)),
        }
    }
    nodes
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
    let mut detail = Detail {
        title: format!("{} · {}", request.eval_id, request.title),
        fields: Vec::new(),
    };
    detail.push(
        "Status",
        format!("{} — {}", request.status, meaning(request.status.as_str())),
    );
    detail.push("Request", request.id.as_str());
    detail.push("Reason", request.blocked_reason.clone().unwrap_or_default());
    detail.push("Error", error(view).unwrap_or_default());
    detail.push(
        "Instruction",
        request.payload["instruction"].as_str().unwrap_or_default(),
    );
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
                    source.completed_at.as_deref().unwrap_or("?"),
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
            request.started_at.as_deref().unwrap_or("-"),
            request.completed_at.as_deref().unwrap_or("-"),
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
        detail.push(
            "Result",
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
        Target::Eval(id) => {
            if let Some(view) = saved.request(id) {
                return request_detail(view, now);
            }
            let definition = saved
                .list("evals")
                .iter()
                .find(|eval| eval["id"] == id.as_str());
            let declaration = definition.map_or(&Value::Null, |eval| &eval["declaration"]);
            detail.title = format!(
                "{id} · {}",
                declaration["title"].as_str().unwrap_or_default()
            );
            detail.push("Status", "not in this Run (outside the selection)");
            detail.push(
                "Instruction",
                declaration["payload"]["instruction"]
                    .as_str()
                    .unwrap_or_default(),
            );
            detail.push(
                "Profile",
                serde_json::from_value::<crate::config::StoredProfile>(
                    declaration["profile"].clone(),
                )
                .map(|value| profile(&value))
                .unwrap_or_else(|_| "unknown".into()),
            );
        }
        Target::Artifact(id) => {
            let artifact = &saved.definitions("artifacts")[id.as_str()];
            let basis = if artifact["basis"] == true {
                " [basis]"
            } else {
                ""
            };
            detail.title = format!("Artifact {id}{basis}");
            let (status, passed, total) = saved.artifact_state(id);
            let status = status.as_deref().unwrap_or(ABSENT);
            detail.push("Status", format!("{status} · {passed}/{total} Evals GREEN"));
            detail.push(
                "Path",
                match artifact["path"].as_str() {
                    Some("") => ".",
                    path => path.unwrap_or("-"),
                },
            );
            detail.push("Family", saved.family(id).unwrap_or_default());
            detail.push(
                "Fingerprint",
                saved.validation(id)["fingerprint"]
                    .as_str()
                    .unwrap_or_default(),
            );
            let component = saved.component(id);
            if component["cyclic"] == true {
                detail.push(
                    "Cycle",
                    format!("↻ {}", join(strs(&component["artifacts"]), ", ")),
                );
            }
            let gates = strs(&component["gates"]).map(|gate| mark(gate, saved.status(gate)));
            detail.push("Gates", join(gates, "\n"));
            let evals = saved.evals(id).into_iter();
            detail.push(
                "Evals",
                join(evals.map(|(eval, _)| mark(eval, saved.status(eval))), "\n"),
            );
            for (key, side, other) in [
                ("Inputs", "target", "source"),
                ("Used by", "source", "target"),
            ] {
                let relations = saved.relations(id, side).into_iter();
                let relations = relations.map(|relation| {
                    format!(
                        "{} — {}",
                        relation[other].as_str().unwrap_or_default(),
                        relation_kind(relation)
                    )
                });
                detail.push(key, join(relations, "\n"));
            }
        }
        Target::Family(name) => {
            detail.title = format!("Family {name}");
            detail.push(
                "Path",
                saved.definitions("families")[name.as_str()]["path"]
                    .as_str()
                    .unwrap_or("-"),
            );
            let members = saved.family_members(name).into_iter().map(|member| {
                let (status, passed, total) = saved.artifact_state(member);
                format!("{} {passed}/{total}", mark(member, status.as_deref()))
            });
            detail.push("Instances", join(members, "\n"));
        }
    }
    detail
}
