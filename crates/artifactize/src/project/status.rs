use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::{
    cache,
    config::{DependencyGates, Profile, read_workspace_config},
    graph::{ArtifactStatus, EvalStatus, Evidence, Graph, Readiness},
    project::{VerifyOptions, selection::select_profiles},
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
    pub selected_eval_ids: Vec<String>,
    pub included_eval_ids: Vec<String>,
    pub satisfied: bool,
    pub obligations: Vec<String>,
    pub artifacts: Vec<ArtifactState>,
    pub evals: Vec<EvalState>,
    pub counts: Counts,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactState {
    pub id: String,
    pub family: Option<String>,
    pub state: &'static str,
    pub reason: String,
    pub eval_ids: Vec<String>,
    pub passed: usize,
    pub total: usize,
    pub satisfied: bool,
    pub obligations: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvalState {
    pub id: String,
    pub target: String,
    pub title: String,
    pub profile: Profile,
    pub selected: bool,
    pub included: bool,
    pub force: bool,
    pub state: &'static str,
    pub action: &'static str,
    pub reason: String,
    pub blocked_by: Vec<String>,
    pub obligations: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last: Option<LastRequest>,
}

#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub artifacts: BTreeMap<&'static str, usize>,
    pub evals: BTreeMap<&'static str, usize>,
    pub execute: usize,
    pub reuse: usize,
    pub wait: usize,
    pub blocked: usize,
}

/// Prepare current identities without executing evals or changing saved evidence.
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
        .map(|id| (id.clone(), Evidence::Stale))
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
    let identities = cache::prepare(
        &config,
        required.iter().copied(),
        &state,
        cancellation.clone(),
    )
    .await?;
    let keys: BTreeMap<_, _> = config
        .evals
        .iter()
        .filter(|eval| !(options.force && selected_ids.contains(&eval.id)))
        .filter_map(|eval| {
            identities.get(eval.target.as_str()).map(|identity| {
                (
                    eval.id.as_str(),
                    (
                        identity.clone(),
                        cache::eval_definition_hash(&eval.declaration),
                    ),
                )
            })
        })
        .collect();
    let cached =
        store::read_identity_executions(&state, &keys.values().cloned().collect::<Vec<_>>())
            .await?;
    for eval in &config.evals {
        if let Some(Claim::Reuse(execution)) =
            keys.get(eval.id.as_str()).and_then(|key| cached.get(key))
        {
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
        .map(|id| (*id).to_owned())
        .collect();
    let mut counts = Counts::default();
    let mut artifacts = Vec::new();
    for id in &required {
        let artifact = &evaluation.artifacts[id];
        let status = artifact_status(artifact.status);
        let own = &graph.artifacts()[id];
        let unmet = graph
            .dependency_closure(&[id])
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter(|id| !evaluation.artifacts[id].own_satisfied)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let reason = if artifact.satisfied {
            "All required obligations are satisfied.".into()
        } else if own.evals.is_empty() && !own.basis {
            "No Evals are declared and this Artifact is not a basis.".into()
        } else {
            format!("Unmet obligations: {}", unmet.join(", "))
        };
        *counts.artifacts.entry(status).or_default() += 1;
        artifacts.push(ArtifactState {
            id: (*id).to_owned(),
            family: config.artifacts[*id]
                .family
                .as_ref()
                .map(|family| family.name.clone()),
            state: status,
            reason,
            eval_ids: own.evals.iter().map(|id| (*id).to_owned()).collect(),
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
        .filter(|eval| required.contains(&eval.target.as_str()))
    {
        let current = &evaluation.evals[eval.id.as_str()];
        let selected = selected_ids.contains(&eval.id);
        let included = included_ids.contains(&eval.id);
        let force = options.force && selected;
        let (action, reason) = match current.readiness {
            // verify takes cached results before gates resolve, even behind RED; the state keeps the gate.
            _ if matches!(current.evidence, Some(Evidence::Current(_))) => (
                "reuse",
                format!(
                    "The current identity and Eval definition have a completed cached result{}",
                    match current.readiness {
                        Readiness::Ready => ".".into(),
                        Readiness::Wait => format!(
                            "; its gates still wait for: {}",
                            current.unmet_gates.join(", ")
                        ),
                        Readiness::Blocked => format!(
                            "; its gates are blocked by RED: {}",
                            current.unmet_gates.join(", ")
                        ),
                    }
                ),
            ),
            Readiness::Blocked => (
                "blocked",
                format!("Dependency verdict RED: {}", current.unmet_gates.join(", ")),
            ),
            Readiness::Wait => (
                "wait",
                format!(
                    "Waiting for current GREEN dependency evidence: {}",
                    current.unmet_gates.join(", ")
                ),
            ),
            Readiness::Ready
                if !force
                    && matches!(
                        keys.get(eval.id.as_str()).and_then(|key| cached.get(key)),
                        Some(Claim::Wait(_) | Claim::WaitHuman(_))
                    ) =>
            {
                (
                    "wait",
                    "The current identity and Eval definition have a live execution.".into(),
                )
            }
            Readiness::Ready => match eval.declaration.profile {
                Profile::Human { .. } => (
                    "execute",
                    "Record a request awaiting a Human claim and submission.".into(),
                ),
                Profile::Runtime { .. } | Profile::Agent { .. } => (
                    "execute",
                    if force {
                        "An explicitly forced Eval requires a new execution.".into()
                    } else if config.artifacts[&eval.target].stale.is_some() {
                        "The current identity and Eval definition have no completed cached result."
                            .into()
                    } else {
                        "No identity is declared; saved noncached results satisfy only their own Run.".into()
                    },
                ),
            },
        };
        let status = if !force
            && matches!(
                keys.get(eval.id.as_str()).and_then(|key| cached.get(key)),
                Some(Claim::WaitHuman(_))
            ) {
            "WAITING_HUMAN"
        } else {
            eval_status(current.status)
        };
        *counts.evals.entry(status).or_default() += 1;
        if included {
            match action {
                "execute" => counts.execute += 1,
                "reuse" => counts.reuse += 1,
                "blocked" => counts.blocked += 1,
                _ => counts.wait += 1,
            }
        }
        evals.push(EvalState {
            id: eval.id.clone(),
            target: eval.target.clone(),
            title: eval.declaration.title.clone(),
            profile: eval.declaration.profile.clone(),
            selected,
            included,
            force,
            state: status,
            action,
            reason,
            blocked_by: current
                .unmet_gates
                .iter()
                .map(|id| (*id).to_owned())
                .collect(),
            obligations: obligations_by_artifact[eval.target.as_str()].clone(),
            last: latest.remove(&eval.id),
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

fn eval_status(status: EvalStatus) -> &'static str {
    match status {
        EvalStatus::Green => "PASS",
        EvalStatus::Red => "RED",
        EvalStatus::Error => "ERROR",
        EvalStatus::Stale => "STALE",
        EvalStatus::Unreviewed => "UNREVIEWED",
        EvalStatus::Wait => "WAIT_DEPENDENCY",
        EvalStatus::Blocked => "BLOCKED",
    }
}

fn artifact_status(status: ArtifactStatus) -> &'static str {
    match status {
        ArtifactStatus::Basis => "BASIS",
        ArtifactStatus::Green => "PASS",
        ArtifactStatus::Incomplete => "INCOMPLETE",
        ArtifactStatus::Error => "ERROR",
        ArtifactStatus::Red => "RED",
        ArtifactStatus::Blocked => "BLOCKED",
        ArtifactStatus::Wait => "WAIT_DEPENDENCY",
        ArtifactStatus::Stale => "STALE",
        ArtifactStatus::Unreviewed => "UNREVIEWED",
    }
}
