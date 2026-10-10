mod states;
pub use states::{ArtifactCondition, EvalCondition, VerifyAction};
#[cfg(test)]
mod tests;

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::{
    cache,
    config::{DependencyGates, Profile, read_workspace_config},
    graph::{Evidence, Graph, Readiness},
    project::{VerifyOptions, selection::select_profiles},
    remote::Session,
    store::{self, Claim, LastRequest},
};

use super::selection::Selection;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusView {
    pub selection: Selection,
    pub recursive: bool,
    pub force: bool,
    pub ignore_gates: bool,
    pub selected_eval_ids: Vec<crate::config::EvalId>,
    pub included_eval_ids: Vec<crate::config::EvalId>,
    pub satisfied: bool,
    pub obligations: Vec<crate::config::ArtifactName>,
    pub artifacts: Vec<ArtifactState>,
    pub evals: Vec<EvalState>,
    pub counts: Counts,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactState {
    pub id: crate::config::ArtifactName,
    pub kind: crate::config::ArtifactKind,
    #[serde(with = "crate::platform::path_serde")]
    pub path: std::path::PathBuf,
    pub tags: Vec<String>,
    pub state: ArtifactCondition,
    pub reason: String,
    pub eval_ids: Vec<crate::config::EvalId>,
    pub passed: usize,
    pub total: usize,
    pub satisfied: bool,
    pub obligations: Vec<crate::config::ArtifactName>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvalState {
    pub id: crate::config::EvalId,
    pub target: crate::config::ArtifactName,
    pub title: String,
    pub profile: Profile,
    pub selected: bool,
    pub included: bool,
    pub force: bool,
    pub state: EvalCondition,
    pub action: VerifyAction,
    pub reason: String,
    pub blocked_by: Vec<String>,
    pub obligations: Vec<crate::config::ArtifactName>,
    /// The Eval definition hash: the eval strategy the key covers.
    pub eval_def_hash: crate::types::DefinitionHash,
    /// The target's current fingerprint; null without one.
    pub fingerprint: Option<crate::types::Fingerprint>,
    /// Each Artifact the eval depends on, with its current fingerprint or null without one.
    pub fingerprints: BTreeMap<crate::config::ArtifactName, Option<crate::types::Fingerprint>>,
    /// The reuse key composed of the hash and the fingerprints; null when one is missing.
    pub key: Option<crate::types::ReuseKey>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last: Option<LastRequest>,
    /// Why the key no longer matches the newest cached result for this Eval definition: the
    /// target's changed files and the dependency Artifacts whose fingerprints changed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changes: Option<cache::Changes>,
}

#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub artifacts: BTreeMap<ArtifactCondition, usize>,
    pub evals: BTreeMap<EvalCondition, usize>,
    pub execute: usize,
    pub derive: usize,
    pub reuse: usize,
    pub wait: usize,
    pub blocked: usize,
}

/// Prepare current fingerprints without executing evals or changing saved evidence.
pub async fn status(
    repo: &Path,
    state_dir: Option<&Path>,
    selection: &Selection,
    options: &VerifyOptions,
    cancellation: CancellationToken,
) -> Result<StatusView, String> {
    let config = read_workspace_config(repo).map_err(|error| error.to_string())?;
    let config = select_profiles(
        config,
        selection,
        options.profile.as_ref(),
        options.recursive,
    )?;
    let ignore_gates = options.ignore_gates.unwrap_or_else(|| {
        config
            .artifacts
            .values()
            .find(|artifact| artifact.path.as_os_str().is_empty())
            .and_then(|artifact| artifact.review_policy.as_ref())
            .is_some_and(|policy| matches!(policy.dependency_gates, Some(DependencyGates::Ignore)))
    });
    let graph = Graph::new(&config).map_err(|error| error.to_string())?;
    let selected = selection.resolve(&config)?;
    let required: BTreeSet<_> = graph
        .dependency_closure(&selected.roots)
        .map_err(|error| error.to_string())?
        .into_iter()
        .collect();
    let selected_eval_ids: Vec<_> = selected.evals.iter().map(|eval| eval.id.clone()).collect();
    let included_eval_ids: Vec<_> = selection
        .included_evals(&config, options.recursive)?
        .iter()
        .map(|eval| eval.id.clone())
        .collect();
    let state = store::state_dir(state_dir)?;
    let mut latest = store::read_latest_requests(&state, &config.root).await?;
    let mut evidence: BTreeMap<_, _> = latest
        .keys()
        .map(|id| (id.parse().expect("saved Eval id"), Evidence::Stale))
        .collect();
    if options.force {
        evidence.extend(
            selected_eval_ids
                .iter()
                .map(|id| (id.clone(), Evidence::Stale)),
        );
    }
    let selected_ids: BTreeSet<_> = selected_eval_ids.iter().collect();
    let included_ids: BTreeSet<_> = included_eval_ids.iter().collect();
    let fingerprints = cache::prepare(
        &config,
        cache::fingerprint_targets(&config, &required.iter().map(|id| id.as_str()).collect()),
        &state,
        &super::fingerprint_parallelism(options)?,
        cancellation.clone(),
    )
    .await?;
    let all_keys = cache::eval_keys(&config, &fingerprints);
    let keys: BTreeMap<_, _> = all_keys
        .iter()
        .filter(|(id, _)| {
            !(options.force
                && selected_ids
                    .iter()
                    .any(|selected| selected.as_str() == **id))
        })
        .map(|(id, key)| (*id, key))
        .collect();
    let mut cached = store::read_keyed_executions(
        &state,
        &keys
            .values()
            .map(|key| key.value.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>(),
    )
    .await?;
    // Read-only: a store record that completed after the local latest counts as reuse without
    // being mirrored, as verify would take it before claiming. --force reads nothing from the
    // store, so its prediction has none.
    if !options.force
        && let Some(remote) = Session::open(Some(&state), Some(&config.root))?
    {
        let all: Vec<_> = keys
            .values()
            .map(|key| key.value.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        for execution in remote.lookup(&all).await? {
            let key = execution.key.clone().expect("remote key");
            let newer = match cached.get(&key) {
                Some(Claim::Reuse(local)) => execution.completed_after(local),
                _ => true,
            };
            if newer {
                cached.insert(key, Claim::Reuse(Box::new(execution)));
            }
        }
    }
    let claim = |id: &str| keys.get(id).and_then(|key| cached.get(&key.value));
    // Explain changed keys against the newest cached result for the same Eval definition.
    let stale: Vec<_> = keys
        .iter()
        .filter(|(id, _)| !matches!(claim(id), Some(Claim::Reuse(_))))
        .map(|(id, key)| ((*id).to_owned(), key.eval_def_hash.clone()))
        .collect();
    let previous = store::read_latest_cached(&state, &stale).await?;
    for eval in &config.evals {
        if let Some(Claim::Reuse(execution)) = claim(&eval.id) {
            evidence.insert(
                eval.id.clone(),
                Evidence::Current(execution.verdict().expect("completed cache entry")),
            );
        }
    }
    if cancellation.is_cancelled() {
        return Err("Project preparation was cancelled.".into());
    }
    let evaluation = graph.evaluate_with_policy(&evidence, ignore_gates);
    let obligations: Vec<_> = evaluation
        .obligations
        .iter()
        .filter(|id| required.contains(*id))
        .map(|id| (*id).clone())
        .collect();
    let mut counts = Counts::default();
    let mut artifacts = Vec::new();
    for id in &required {
        let artifact = &evaluation.artifacts[id.as_str()];
        let status = ArtifactCondition::from(artifact.status);
        let own = &graph.artifacts()[id.as_str()];
        let unmet = graph
            .dependency_closure(&[id])
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter(|id| !evaluation.artifacts[id.as_str()].own_satisfied)
            .map(|id| (*id).clone())
            .collect::<Vec<_>>();
        let reason = if artifact.satisfied {
            "All required obligations are satisfied.".into()
        } else if own.evals.is_empty() && !own.basis {
            "No Evals are declared and this Artifact is not a basis.".into()
        } else {
            format!(
                "Unmet obligations: {}",
                unmet
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        *counts.artifacts.entry(status).or_default() += 1;
        artifacts.push(ArtifactState {
            kind: config.artifacts[*id].kind,
            path: config.artifacts[*id].path.clone(),
            id: (*id).clone(),
            tags: config.artifacts[*id].tags.clone(),
            state: status,
            reason,
            eval_ids: own.evals.iter().map(|id| (*id).clone()).collect(),
            passed: artifact.passed,
            total: artifact.total,
            satisfied: artifact.satisfied,
            obligations: unmet,
        });
    }
    let obligations_by_artifact: BTreeMap<_, _> = artifacts
        .iter()
        .map(|artifact| (artifact.id.as_str(), &artifact.obligations))
        .collect();
    let mut evals = Vec::new();
    for eval in config
        .evals
        .iter()
        .filter(|eval| required.contains(&eval.target))
    {
        let current = &evaluation.evals[eval.id.as_str()];
        let selected = selected_ids.contains(&eval.id);
        let included = included_ids.contains(&eval.id);
        let derived = matches!(eval.declaration.profile(), Profile::Dependency { .. });
        let force = options.force && selected && !derived;
        let (action, reason) = match current.readiness {
            _ if derived => (
                VerifyAction::Derive,
                if current.blocked_by.is_empty() {
                    "Derived from current GREEN dependency evidence; no execution or reuse.".into()
                } else {
                    format!(
                        "Derived dependency verdict: waiting for current GREEN evidence from {}.",
                        current
                            .blocked_by
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                },
            ),
            // verify takes cached results before gates resolve, even behind RED; the state keeps
            // the gate.
            _ if matches!(current.evidence, Some(Evidence::Current(_))) => (
                VerifyAction::Reuse,
                format!(
                    "The current eval and fingerprints have a completed {}{}{}",
                    match claim(&eval.id) {
                        Some(Claim::Reuse(execution)) if execution.origin.is_some() => format!(
                            "result in the remote review store, from {}",
                            execution
                                .producer
                                .as_ref()
                                .map_or("an unknown producer", |producer| producer.name.as_str())
                        ),
                        _ => "cached result".into(),
                    },
                    match claim(&eval.id) {
                        Some(Claim::Reuse(execution))
                            if execution.profile
                                != crate::config::StoredProfile::from(
                                    eval.declaration.profile()
                                ) =>
                            format!(
                                " produced by profile {}",
                                crate::query::profile_name(&execution.profile, &execution.options)
                            ),
                        _ => String::new(),
                    },
                    match current.readiness {
                        Readiness::Ready => ".".into(),
                        Readiness::Wait => format!(
                            "; its gates still wait for: {}",
                            current
                                .unmet_gates
                                .iter()
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        Readiness::Blocked => format!(
                            "; its gates are blocked by RED: {}",
                            current
                                .unmet_gates
                                .iter()
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    }
                ),
            ),
            Readiness::Blocked => (
                VerifyAction::Blocked,
                format!(
                    "Dependency verdict RED: {}",
                    current
                        .unmet_gates
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ),
            Readiness::Wait => (
                VerifyAction::Wait,
                format!(
                    "Waiting for current GREEN dependency evidence: {}",
                    current
                        .unmet_gates
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ),
            Readiness::Ready
                if !force
                    && matches!(claim(&eval.id), Some(Claim::Wait(_) | Claim::WaitHuman(_))) =>
            {
                (
                    VerifyAction::Wait,
                    "The current eval and fingerprints have a live execution.".into(),
                )
            }
            Readiness::Ready => match eval.declaration.profile() {
                Profile::Dependency { .. } => unreachable!("derived above"),
                Profile::Human { .. } => (
                    VerifyAction::Execute,
                    "Record a request awaiting a Human claim and submission.".into(),
                ),
                Profile::Runtime { .. } | Profile::Agent { .. } => (
                    VerifyAction::Execute,
                    if force {
                        "An explicitly forced Eval requires a new execution.".into()
                    } else {
                        match cache::eval_key(&config, eval, &fingerprints) {
                            Ok(_) => {
                                "The current eval and fingerprints have no completed cached result."
                                    .into()
                            }
                            Err(cache::Unkeyed::Derived) => unreachable!("derived above"),
                            Err(cache::Unkeyed::Target) => {
                                "The Artifact declares fingerprint: false; saved noncached results satisfy only their own Run."
                                    .into()
                            }
                            Err(cache::Unkeyed::Dependency(id)) => format!(
                                "Dependency {id} declares fingerprint: false, so this eval has no reuse key; saved noncached results satisfy only their own Run."
                            ),
                        }
                    },
                ),
            },
        };
        let status = if !force && matches!(claim(&eval.id), Some(Claim::WaitHuman(_))) {
            EvalCondition::WaitingHuman
        } else {
            EvalCondition::from(current.status)
        };
        *counts.evals.entry(status).or_default() += 1;
        if included {
            counts.action(action);
        }
        evals.push(EvalState {
            id: eval.id.clone(),
            target: eval.target.clone(),
            title: eval.declaration.title.clone(),
            profile: eval.declaration.profile().clone(),
            selected,
            included,
            force,
            state: status,
            action,
            reason,
            blocked_by: current.blocked_by.iter().map(ToString::to_string).collect(),
            obligations: obligations_by_artifact[eval.target.as_str()].clone(),
            eval_def_hash: cache::eval_definition_hash(&eval.declaration),
            fingerprint: fingerprints
                .get(eval.target.as_str())
                .map(|fingerprint| fingerprint.value.clone()),
            fingerprints: cache::dependencies(&config, eval)
                .into_iter()
                .map(|id| {
                    (
                        config.artifacts[id].name.clone(),
                        fingerprints
                            .get(id)
                            .map(|fingerprint| fingerprint.value.clone()),
                    )
                })
                .collect(),
            key: all_keys.get(eval.id.as_str()).map(|key| key.value.clone()),
            last: latest.remove(eval.id.as_str()),
            changes: keys.get(eval.id.as_str()).and_then(|key| {
                previous
                    .get(&(eval.id.to_string(), key.eval_def_hash.clone()))
                    .map(|execution| {
                        cache::changes(
                            execution,
                            &eval.target,
                            key,
                            fingerprints[eval.target.as_str()].manifest.as_ref(),
                        )
                    })
            }),
        });
    }
    Ok(StatusView {
        selection: selection.clone(),
        recursive: options.recursive,
        force: options.force,
        ignore_gates,
        selected_eval_ids,
        included_eval_ids,
        satisfied: obligations.is_empty(),
        obligations,
        artifacts,
        evals,
        counts,
    })
}

impl Counts {
    fn action(&mut self, action: VerifyAction) {
        match action {
            VerifyAction::Execute => self.execute += 1,
            VerifyAction::Derive => self.derive += 1,
            VerifyAction::Reuse => self.reuse += 1,
            VerifyAction::Wait => self.wait += 1,
            VerifyAction::Blocked => self.blocked += 1,
        }
    }
}
