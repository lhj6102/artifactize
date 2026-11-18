//! Cards and detail-pane presentation for Runs, Artifacts and Evals.
use super::*;
use serde_json::Value;

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

    /// The verdict first; technical identifiers and hashes last. The request stays visible.
    pub fn of(key: &str) -> Self {
        match key {
            "Status" | "Error" | "Reason" | "Blocked by" | "Result" | "Claim" | "Source"
            | "At Run end" | "Validation" | "Counts" | "Errors" | "Waiting" | "Running"
            | "Evals" | "Gates" => Self::Outcome,
            "Waits for" => Self::Waits,
            "Timing" | "Usage" | "Budget" | "Work" | "Saved usage" => Self::Provenance,
            "Execution" | "Fingerprint" | "Key" | "Key covers" | "Path" | "Tags" | "Cycle"
            | "Run" | "Repository" => Self::Technical,
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

/// Top-level entries of a result, one line each, such as `verdict GREEN`. Saved runtime
/// logs are left to the evidence view.
fn result_summary(result: &crate::store::ExecutionResult) -> String {
    let result = result.to_json();
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
            format!("agent {backend} {model}{reasoning}")
        }
        StoredProfile::Runtime { command, args, .. } => {
            format!(
                "runtime {} {}",
                crate::platform::program_text(command),
                args.join(" ")
            )
        }
        StoredProfile::Human {} => "human".into(),
    }
}

/// Execution options that are set, such as `variant fast · timeoutMs 60000`.
fn options(options: &crate::store::ExecutionOptions) -> String {
    let set: Vec<_> = [
        options.backend.as_ref().map(|x| format!("backend {x}")),
        options.max_tokens.map(|x| format!("maxTokens {x}")),
        options.max_tool_calls.map(|x| format!("maxToolCalls {x}")),
        options.model.as_ref().map(|x| format!("model {x}")),
        (!options.mutex.is_empty()).then(|| {
            // Mutex tags use validated ASCII identifiers with no JSON escape characters.
            format!(
                "mutex [\"{}\"]",
                join(options.mutex.iter().map(|x| x.as_str()), "\",\"")
            )
        }),
        options.reasoning.map(|x| format!("reasoning {x}")),
        options
            .timeout_ms
            .map(|x| format!("timeoutMs {}", x.as_millis())),
        options.variant.as_ref().map(|x| format!("variant {x}")),
    ]
    .into_iter()
    .flatten()
    .collect();
    if set.is_empty() {
        "declared defaults".into()
    } else {
        set.join(" · ")
    }
}

fn request_detail(view: &RequestView, now: OffsetDateTime) -> Detail {
    let request = &view.request;
    let summary = query::request_summary(view, now);
    let deps = join(&request.deps, ", ");
    let mut detail = Detail {
        title: format!("{} · {}", request.eval_id, request.title),
        summary: join(
            [
                format!(
                    "{}{}",
                    request.status(),
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
        format!(
            "{} — {}",
            request.status(),
            Status::from(request.status()).meaning()
        ),
    );
    detail.push("Request", request.id.as_str());
    detail.push("Reason", request.blocked_reason.clone().unwrap_or_default());
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
                    crate::platform::path_text(&source.repo_path),
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
        let owner = execution
            .owner
            .map_or(String::new(), |owner| format!(" · owner pid {}", owner.pid));
        detail.push(
            "Execution",
            format!("{} {}{owner}", execution.id, execution.status()),
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
                .completed_at()
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
    if let Some(result) = &request.result() {
        detail.push("Result", result_summary(result));
        detail.push(
            "Raw result",
            serde_json::to_string_pretty(result).unwrap_or_default(),
        );
    }
    let total = counter_pairs(&summary.usage.usage);
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
            counter_pairs(&attempt.usage)
        )
    });
    let state = if reused(view) {
        format!("reused: spent none · original attempts {original}")
    } else {
        format!(
            "{} · attempts {}{}",
            summary.usage.usage_state,
            summary.usage.attempts,
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

/// An eval at a glance: what it checks, its result once it has one, and what running it cost.
/// For a reused result, `took` and `tokens` are what reuse saved. Everything else about the
/// request stays in its [`Detail`]'s fields.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Card {
    pub instruction: String,
    /// The result's own entries, without its verdict and saved output.
    pub result: Option<serde_json::Map<String, Value>>,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
    pub reused: bool,
    /// The execution's own span: from its start to its end, or to `now` while it runs.
    pub took: Option<String>,
    /// Total tokens over every attempt.
    pub tokens: Option<u64>,
    pub model: Option<String>,
}

/// A result's own entries, without its verdict and saved output, as a Card shows them.
pub(crate) fn entries(result: &crate::store::ExecutionResult) -> serde_json::Map<String, Value> {
    let mut entries = result.to_json().as_object().cloned().unwrap_or_default();
    for key in [
        "verdict",
        "stdout",
        "stderr",
        "exitCode",
        "durationMs",
        "truncated",
    ] {
        entries.remove(key);
    }
    entries
}

pub fn card(view: &RequestView, now: OffsetDateTime) -> Card {
    use crate::config::StoredProfile;
    use crate::store::ExecutionResult;
    let request = &view.request;
    let reused = reused(view);
    let result = request.result();
    let attempts = if reused {
        request.reused_usage.as_ref()
    } else {
        request.usage.as_ref()
    };
    let tokens = attempts.and_then(|attempts| attempt_tokens(attempts));
    Card {
        instruction: request.payload.instruction().to_owned(),
        result: result.map(entries),
        exit_code: match result {
            Some(ExecutionResult::Runtime(result)) => result.exit_code.value().copied(),
            _ => None,
        },
        error: error(view),
        reused,
        took: view
            .execution
            .as_ref()
            .and_then(|execution| span(execution.started_at, execution.completed_at(), now)),
        tokens,
        model: match &request.profile {
            StoredProfile::Agent { model, .. } => Some(model.to_string()),
            _ => None,
        },
    }
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
            detail.push("Comment", progress.comment.clone().unwrap_or_default());
            detail.push("Validation", progress.validation.as_str());
            detail.push(
                "Counts",
                join(
                    progress
                        .counts
                        .iter()
                        .filter(|(_, n)| *n > 0)
                        .map(|(status, n)| format!("{} {status} {n}", status.glyph())),
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
            detail.push("Waits for", tree::artifact_waits(&saved, id, now));
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
                            if path.local().is_some_and(|path| path.as_os_str().is_empty()) {
                                ".".into()
                            } else {
                                path.to_string()
                            }
                        },
                    ),
            );
            if artifact
                .and_then(|artifact| artifact.path.value())
                .and_then(crate::config::Target::uri)
                .is_some()
            {
                detail.push("Target kind", "Live");
            }
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
