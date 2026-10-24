mod states;
mod view;
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
    config::{Profile, read_workspace_config},
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
    pub selected_eval_ids: Vec<crate::config::EvalId>,
    pub included_eval_ids: Vec<crate::config::EvalId>,
    pub satisfied: bool,
    pub obligations: Vec<crate::config::ArtifactName>,
    pub artifacts: Vec<ArtifactState>,
    pub evals: Vec<EvalState>,
    pub counts: Counts,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unreadable: Vec<store::Unreadable>,
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
    pub blocked_by: Vec<crate::store::Blocker>,
    pub obligations: Vec<crate::config::ArtifactName>,
    /// The Eval definition hash: the evaluation criteria the key covers.
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
    pub reuse: usize,
    pub wait: usize,
    pub blocked: usize,
}

/// All effects needed by the status projection have finished before this is assembled.
struct Snapshot<'a> {
    required: BTreeSet<&'a crate::config::ArtifactName>,
    selected_eval_ids: Vec<crate::config::EvalId>,
    included_eval_ids: Vec<crate::config::EvalId>,
    fingerprints: BTreeMap<&'a crate::config::ArtifactName, cache::PreparedFingerprint>,
    all_keys: BTreeMap<&'a crate::config::EvalId, cache::Key>,
    cached: BTreeMap<crate::config::EvalId, Claim>,
    previous: BTreeMap<(crate::config::EvalId, crate::types::DefinitionHash), store::Execution>,
    latest: BTreeMap<crate::config::EvalId, LastRequest>,
    waiting: Vec<store::RequestView>,
    unreadable: Vec<store::Unreadable>,
}

/// Prepare current fingerprints without executing evals or changing saved evidence.
pub async fn status(
    repo: &Path,
    state_dir: Option<&Path>,
    selection: &Selection,
    options: &VerifyOptions,
    cancellation: CancellationToken,
) -> Result<StatusView, String> {
    scan(repo, state_dir, selection, options, cancellation)
        .await
        .map(|(view, _)| view)
}

/// [`status`], with the completed results it found to reuse, by eval.
pub(crate) async fn scan(
    repo: &Path,
    state_dir: Option<&Path>,
    selection: &Selection,
    options: &VerifyOptions,
    cancellation: CancellationToken,
) -> Result<
    (
        StatusView,
        BTreeMap<crate::config::EvalId, store::Execution>,
    ),
    String,
> {
    let config = read_workspace_config(repo).map_err(|error| error.to_string())?;
    scan_config(config, state_dir, selection, options, cancellation).await
}

/// [`scan`] of a workspace configuration already read.
pub(crate) async fn scan_config(
    config: crate::config::RepoConfig,
    state_dir: Option<&Path>,
    selection: &Selection,
    options: &VerifyOptions,
    cancellation: CancellationToken,
) -> Result<
    (
        StatusView,
        BTreeMap<crate::config::EvalId, store::Execution>,
    ),
    String,
> {
    let parallelism = super::fingerprint_parallelism(options)?;
    let config = select_profiles(
        config,
        selection,
        options.profile.as_ref(),
        options.recursive,
    )?;
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
    let latest = store::read_latest_requests(&state, &config.root).await?;
    let selected_ids: BTreeSet<_> = selected_eval_ids.iter().collect();
    let fingerprints = cache::prepare(
        &config,
        cache::fingerprint_targets(&config, &required),
        &state,
        &parallelism,
        cancellation.clone(),
    )
    .await?;
    let all_keys = cache::eval_keys(&config, &fingerprints);
    let keys: BTreeMap<_, _> = all_keys
        .iter()
        .filter(|(id, _)| !(options.force && selected_ids.contains(*id)))
        .map(|(id, key)| (*id, key))
        .collect();
    let mut cached = store::read_profiled_executions(
        &state,
        config
            .evals
            .iter()
            .filter_map(|eval| {
                keys.get(&eval.id).map(|key| {
                    (
                        eval.id.clone(),
                        key.value.clone(),
                        eval.declaration.profile().into(),
                    )
                })
            })
            .collect(),
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
            for (id, _) in keys.iter().filter(|(_, candidate)| candidate.value == key) {
                let newer = match cached.get(*id) {
                    Some(Claim::Reuse(local)) => execution.completed_after(local),
                    _ => true,
                };
                if newer {
                    cached.insert((*id).clone(), Claim::Reuse(Box::new(execution.clone())));
                }
            }
        }
    }
    let claim = |id: &crate::types::EvalId| cached.get(id);
    // Explain changed keys against the newest cached result for the same Eval definition.
    let stale: Vec<_> = keys
        .iter()
        .filter(|(id, _)| !matches!(claim(id), Some(Claim::Reuse(_))))
        .map(|(id, key)| ((*id).clone(), key.eval_def_hash.clone()))
        .collect();
    let previous = store::read_latest_cached(&state, &stale).await?;
    let waiting = store::read_waiting(&state, Some(&config.root)).await?;
    let unreadable = store::read_unreadable(&state, Some(&config.root)).await?;
    if cancellation.is_cancelled() {
        return Err("Project preparation was cancelled.".into());
    }
    let reused = cached
        .iter()
        .filter_map(|(id, claim)| match claim {
            Claim::Reuse(execution) => Some((id.clone(), (**execution).clone())),
            _ => None,
        })
        .collect();
    view::assemble(
        &config,
        &graph,
        selection,
        options,
        Snapshot {
            required,
            selected_eval_ids,
            included_eval_ids,
            fingerprints,
            all_keys,
            cached,
            previous,
            latest,
            waiting,
            unreadable,
        },
    )
    .map(|view| (view, reused))
}

impl Counts {
    fn action(&mut self, action: VerifyAction) {
        match action {
            VerifyAction::Execute => self.execute += 1,
            VerifyAction::Reuse => self.reuse += 1,
            VerifyAction::Wait => self.wait += 1,
            VerifyAction::Blocked => self.blocked += 1,
        }
    }
}
