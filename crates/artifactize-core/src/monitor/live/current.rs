//! A branch's worktree as it is now, scanned as `artifactize status` does: the graph of its
//! current declarations, each Artifact marked by what verify would do with it now.
use crate::{
    config::{ArtifactKind, Profile, ProfileKind},
    graph_structure::{self, Structure},
    monitor::model::{self, Card, Status, Tone},
    project::{
        ArtifactCondition, EvalCondition, EvalState, StatusView, VerifyAction, VerifyOptions,
        selection::Selection,
    },
    store::{Claim, Execution, ExecutionResult},
    types::{ArtifactName, EvalId, Fingerprint},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};
use time::OffsetDateTime;
use tokio_util::sync::CancellationToken;

/// What verify would do now. Declared in precedence order: an Artifact takes the first plan
/// any of its evals has (one to run before one still being checked before blocked before
/// waiting before reused).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Plan {
    /// Evaluated again: at least one eval has no result to reuse.
    Run,
    /// Not known yet: a fingerprint it needs, or the plan of an eval it waits for, is still
    /// being computed.
    Check,
    /// Cannot be evaluated: an upstream eval is RED.
    Blocked,
    /// Waits: for its dependencies' evidence, or a live execution.
    Wait,
    /// Every eval's result would be reused without evaluation (an Artifact without evals has
    /// nothing to evaluate).
    Reuse,
}

impl Plan {
    /// The plan as a Current names it: `reused`, `would run`, `checking`, `waiting` or
    /// `blocked`.
    pub fn word(self) -> &'static str {
        match self {
            Self::Run => "would run",
            Self::Check => "checking",
            Self::Blocked => "blocked",
            Self::Wait => "waiting",
            Self::Reuse => "reused",
        }
    }

    /// Its one-character mark: reused as a peer's reuse is marked, the others as the statuses
    /// they lead to.
    pub fn glyph(self) -> &'static str {
        match self {
            Self::Run => "○",
            Self::Check => "…",
            Self::Blocked => Status::Blocked.glyph(),
            Self::Wait => Status::Wait.glyph(),
            Self::Reuse => "↻",
        }
    }

    pub fn tone(self) -> Tone {
        match self {
            Self::Run => Tone::Running,
            Self::Check => Tone::Muted,
            Self::Blocked => Tone::Blocked,
            Self::Wait => Tone::Queued,
            Self::Reuse => Tone::Green,
        }
    }
}

impl CurrentArtifact {
    /// Its plan as its node and row read: `would run (2 evals)`, `reused`, ….
    pub fn text(&self) -> String {
        match (self.plan, self.runs) {
            (Plan::Run, 1) => "would run (1 eval)".into(),
            (Plan::Run, runs) => format!("would run ({runs} evals)"),
            (plan, _) => plan.word().into(),
        }
    }
}

impl From<VerifyAction> for Plan {
    fn from(action: VerifyAction) -> Self {
        match action {
            VerifyAction::Execute => Self::Run,
            VerifyAction::Blocked => Self::Blocked,
            VerifyAction::Wait => Self::Wait,
            VerifyAction::Reuse => Self::Reuse,
        }
    }
}

/// Why an eval is still being checked.
const CHECKING: &str = "Still being checked: a fingerprint its key needs, or the plan of an eval it waits for, is not known yet.";

/// One eval of the scan.
#[derive(Debug, Clone)]
pub struct CurrentEval {
    pub id: EvalId,
    /// Its id within its Artifact, as a Run's tree names it.
    pub name: String,
    pub title: String,
    pub kind: ProfileKind,
    pub plan: Plan,
    pub state: EvalCondition,
    /// Why it would run, wait, be blocked or be reused.
    pub reason: String,
    /// Its instruction and the result it would reuse, as an eval's Summary shows them.
    pub card: Card,
    /// Every other fact by name: its profile, fingerprint, reuse key and what changed.
    pub facts: Vec<(&'static str, String)>,
}

/// One Artifact of the scan.
#[derive(Debug, Clone)]
pub struct CurrentArtifact {
    pub name: ArtifactName,
    pub kind: ArtifactKind,
    pub plan: Plan,
    /// How many of its evals would run.
    pub runs: usize,
    pub path: PathBuf,
    pub state: ArtifactCondition,
    pub reason: String,
    pub evals: Vec<CurrentEval>,
}

/// `to`'s evals use `from`, lit by `from`'s plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentEdge {
    pub from: ArtifactName,
    pub to: ArtifactName,
    pub plan: Plan,
}

/// The scan of one worktree.
#[derive(Debug, Clone)]
pub struct Current {
    pub scanned_at: OffsetDateTime,
    pub fingerprint: Fingerprint,
    /// Every Artifact, eval criterion and labeled relation as text, shared by a scan's steps.
    pub structure: Arc<str>,
    /// In name order.
    pub artifacts: Vec<CurrentArtifact>,
    pub edges: Vec<CurrentEdge>,
}

/// Scan the workspace at `repo` as it is now against the state at `state`, as `artifactize
/// status` does: it hashes files and may run fingerprint scripts or ask the review store, and
/// records nothing. `publish` gets the graph as soon as the declarations are read, every
/// Artifact still being checked, then again each time fingerprints come in.
pub async fn scan(
    repo: &Path,
    state: &Path,
    cancellation: CancellationToken,
    publish: impl FnMut(Current) + Send,
) -> Result<Current, String> {
    let current = read(repo, state, cancellation, publish).await;
    // What the scan read is several times what it keeps: give it back, or a long-lived
    // monitor keeps the largest scan's memory after the scan is gone.
    crate::platform::release_free_memory();
    current
}

async fn read(
    repo: &Path,
    state: &Path,
    cancellation: CancellationToken,
    mut publish: impl FnMut(Current) + Send,
) -> Result<Current, String> {
    // One read of the declarations serves both: the structure depends on them alone.
    let config = crate::config::read_workspace_config(repo).map_err(|error| error.to_string())?;
    let structure = graph_structure::extract(&config);
    // Every step of the scan shows the same declarations: describe them once.
    let declared = Declared {
        tree: structure.tree().into(),
        fingerprint: structure.fingerprint(),
        structure: &structure,
    };
    let (status, cached) = crate::project::scan(
        config,
        Some(state),
        &Selection::All,
        &VerifyOptions::default(),
        cancellation,
        Some(&mut |progress: crate::project::Progress<'_>| {
            publish(project(
                &declared,
                &progress.view,
                progress.cached,
                &progress.checking,
                OffsetDateTime::now_utc(),
            ));
        }),
    )
    .await?;
    Ok(project(
        &declared,
        &status,
        &cached,
        &BTreeSet::new(),
        OffsetDateTime::now_utc(),
    ))
}

/// What running `profile` costs as Execution options name it.
fn profile(profile: &Profile) -> String {
    match profile {
        Profile::Agent { backend, model, .. } => format!("agent · {} · {model}", backend.as_str()),
        other => other.kind().name().to_owned(),
    }
}

fn card(
    instruction: &str,
    profile: &Profile,
    plan: Plan,
    reused: Option<&Execution>,
    now: OffsetDateTime,
) -> Card {
    let result = reused.and_then(Execution::result);
    Card {
        instruction: instruction.to_owned(),
        result: result.map(model::entries),
        exit_code: match result {
            Some(ExecutionResult::Runtime(result)) => result.exit_code.value().copied(),
            _ => None,
        },
        error: None,
        reused: plan == Plan::Reuse,
        took: reused
            .and_then(|execution| model::span(execution.started_at, execution.completed_at(), now)),
        tokens: reused
            .and_then(|execution| execution.usage.as_ref())
            .map(|attempts| {
                attempts
                    .iter()
                    .filter_map(|attempt| attempt.usage.get("totalTokens"))
                    .sum::<u64>()
            })
            .filter(|tokens| *tokens > 0),
        model: match profile {
            Profile::Agent { model, .. } => Some(model.to_string()),
            _ => None,
        },
    }
}

/// The declarations a scan read, as every one of its steps shows them.
struct Declared<'s> {
    structure: &'s Structure,
    tree: Arc<str>,
    fingerprint: Fingerprint,
}

/// The scan of `declared` as `status` found it, with the claims it found, the evals still
/// being `checking` marked so.
fn project(
    declared: &Declared<'_>,
    status: &StatusView,
    cached: &BTreeMap<EvalId, Claim>,
    checking: &BTreeSet<&EvalId>,
    now: OffsetDateTime,
) -> Current {
    let mut by_target: BTreeMap<&ArtifactName, Vec<&EvalState>> = BTreeMap::new();
    for eval in &status.evals {
        by_target.entry(&eval.target).or_default().push(eval);
    }
    let artifacts: Vec<CurrentArtifact> = status
        .artifacts
        .iter()
        .map(|artifact| {
            let evals: Vec<CurrentEval> =
                by_target
                    .get(&artifact.id)
                    .into_iter()
                    .flatten()
                    .map(|eval| {
                        let plan = if checking.contains(&eval.id) {
                            Plan::Check
                        } else {
                            Plan::from(eval.action)
                        };
                        let reused = match cached.get(&eval.id) {
                            Some(Claim::Reuse(execution)) => Some(&**execution),
                            _ => None,
                        };
                        let instruction = declared
                            .structure
                            .evals
                            .get(&eval.id)
                            .and_then(|declared| declared.payload.as_ref())
                            .map_or("", |payload| payload.instruction.as_str());
                        let mut facts = vec![("Profile", profile(&eval.profile))];
                        facts.extend(
                            eval.fingerprint
                                .as_ref()
                                .map(|fingerprint| ("Fingerprint", fingerprint.to_string())),
                        );
                        facts.push(("Eval", eval.id.to_string()));
                        facts.push(("Verify would", eval.action.to_string()));
                        facts.push(("State", eval.state.to_string()));
                        facts.extend(eval.key.as_ref().map(|key| ("Reuse key", key.to_string())));
                        facts.push(("Definition hash", eval.eval_def_hash.to_string()));
                        facts.extend(eval.last.as_ref().map(|last| {
                            ("Last Run", format!("{} · {}", last.run_id, last.verdict))
                        }));
                        if let Some(changes) = &eval.changes {
                            facts.push(("Changes", changes.summary.clone()));
                            facts.extend(
                                changes
                                    .files
                                    .as_ref()
                                    .map(|files| ("Changed files", files.join("\n"))),
                            );
                            facts.extend(
                                changes
                                    .dependencies
                                    .as_ref()
                                    .map(|names| ("Changed dependencies", names.join("\n"))),
                            );
                        }
                        CurrentEval {
                            id: eval.id.clone(),
                            name: crate::monitor::model::tree::local(&eval.id).to_owned(),
                            title: eval.title.clone(),
                            kind: eval.profile.kind(),
                            plan,
                            state: eval.state,
                            reason: if plan == Plan::Check {
                                CHECKING.into()
                            } else {
                                eval.reason.clone()
                            },
                            card: card(instruction, &eval.profile, plan, reused, now),
                            facts,
                        }
                    })
                    .collect();
            CurrentArtifact {
                name: artifact.id.clone(),
                kind: artifact.kind,
                plan: evals
                    .iter()
                    .map(|eval| eval.plan)
                    .min()
                    .unwrap_or(Plan::Reuse),
                runs: evals.iter().filter(|eval| eval.plan == Plan::Run).count(),
                path: artifact.path.clone(),
                state: artifact.state,
                reason: artifact.reason.clone(),
                evals,
            }
        })
        .collect();
    let plans: BTreeMap<&ArtifactName, Plan> = artifacts
        .iter()
        .map(|artifact| (&artifact.name, artifact.plan))
        .collect();
    let pairs: BTreeSet<(&ArtifactName, &ArtifactName)> = declared
        .structure
        .relations
        .iter()
        .filter(|relation| relation.source != relation.target)
        .map(|relation| (&relation.source, &relation.target))
        .collect();
    let edges = pairs
        .into_iter()
        .filter_map(|(from, to)| {
            Some(CurrentEdge {
                plan: *plans.get(from)?,
                from: from.clone(),
                to: plans.contains_key(to).then(|| to.clone())?,
            })
        })
        .collect();
    Current {
        scanned_at: now,
        fingerprint: declared.fingerprint.clone(),
        structure: declared.tree.clone(),
        artifacts,
        edges,
    }
}

#[cfg(test)]
mod tests;
