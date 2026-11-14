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
    required: &'a BTreeSet<&'a crate::config::ArtifactName>,
    selected_eval_ids: &'a [crate::config::EvalId],
    included_eval_ids: &'a [crate::config::EvalId],
    fingerprints: &'a BTreeMap<&'a crate::config::ArtifactName, cache::PreparedFingerprint>,
    known: &'a Known<'a>,
}

/// What the store and the remote review store say about the keys a scan computed so far.
#[derive(Default)]
struct Known<'c> {
    cached: BTreeMap<crate::config::EvalId, Claim>,
    previous: BTreeMap<(crate::config::EvalId, crate::types::DefinitionHash), store::Execution>,
    latest: BTreeMap<crate::config::EvalId, LastRequest>,
    waiting: Vec<store::RequestView>,
    unreadable: Vec<store::Unreadable>,
    /// Every key computed so far, by eval.
    keys: BTreeMap<&'c crate::config::EvalId, cache::Key>,
}

/// What a scan knows part way: the status as far as its fingerprints go, the results it found
/// to reuse, and the evals whose action may still change.
pub(crate) struct Progress<'a> {
    pub view: StatusView,
    pub cached: &'a BTreeMap<crate::config::EvalId, Claim>,
    pub checking: BTreeSet<&'a crate::config::EvalId>,
}

/// Where a scan hands what it knows part way.
pub(crate) type Publish<'p> = &'p mut (dyn for<'a> FnMut(Progress<'a>) + Send);

/// Prepare current fingerprints without executing evals or changing saved evidence.
pub async fn status(
    repo: &Path,
    state_dir: Option<&Path>,
    selection: &Selection,
    options: &VerifyOptions,
    cancellation: CancellationToken,
) -> Result<StatusView, String> {
    // Options are checked before anything is read.
    super::fingerprint_parallelism(options)?;
    let config = read_workspace_config(repo).map_err(|error| error.to_string())?;
    scan(config, state_dir, selection, options, cancellation, None)
        .await
        .map(|(view, _)| view)
}

/// [`status`] of a workspace configuration already read, with the claims it found by eval.
/// With `publish`, what it knows is handed there as its fingerprints come in.
pub(crate) async fn scan(
    config: crate::config::RepoConfig,
    state_dir: Option<&Path>,
    selection: &Selection,
    options: &VerifyOptions,
    cancellation: CancellationToken,
    publish: Option<Publish<'_>>,
) -> Result<(StatusView, BTreeMap<crate::config::EvalId, Claim>), String> {
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
    // Read-only: a store record that completed after the local latest counts as reuse without
    // being mirrored, as verify would take it before claiming. --force reads nothing from the
    // store, so its prediction has none.
    let remote = if options.force {
        None
    } else {
        Session::open(Some(&state), Some(&config.root))?
    };
    let scope = Scope {
        config: &config,
        graph: &graph,
        selection,
        options,
        required: &required,
        selected_eval_ids: &selected_eval_ids,
        included_eval_ids: &included_eval_ids,
        state: &state,
        remote: remote.as_deref(),
    };
    let targets = cache::fingerprint_targets(&config, &required);
    let fingerprints = match publish {
        None => {
            cache::prepare(&config, targets, &state, &parallelism, cancellation.clone()).await?
        }
        Some(publish) => {
            scope
                .progressively(targets, &parallelism, cancellation.clone(), publish)
                .await?
        }
    };
    // Read once every fingerprint is in, so a Run that ended meanwhile counts.
    let known = scope.read(&fingerprints).await?;
    if cancellation.is_cancelled() {
        return Err("Project preparation was cancelled.".into());
    }
    let view = scope.assemble(&fingerprints, &known)?;
    Ok((view, known.cached))
}

/// What one scan reads against.
struct Scope<'a> {
    config: &'a crate::config::RepoConfig,
    graph: &'a Graph<'a>,
    selection: &'a Selection,
    options: &'a VerifyOptions,
    required: &'a BTreeSet<&'a crate::config::ArtifactName>,
    selected_eval_ids: &'a [crate::config::EvalId],
    included_eval_ids: &'a [crate::config::EvalId],
    state: &'a Path,
    remote: Option<&'a Session>,
}

type Fingerprints<'a> = BTreeMap<&'a crate::config::ArtifactName, cache::PreparedFingerprint>;

impl<'a> Scope<'a> {
    /// Prepare the fingerprints, handing `publish` what is known each time some come in: first
    /// the graph with every eval being checked, then each action as soon as it cannot change.
    async fn progressively(
        &self,
        targets: BTreeSet<&'a crate::config::ArtifactName>,
        parallelism: &cache::Parallelism,
        cancellation: CancellationToken,
        publish: Publish<'_>,
    ) -> Result<Fingerprints<'a>, String> {
        let mut fingerprints = Fingerprints::new();
        let first = Known::default();
        publish(Progress {
            view: self.assemble(&fingerprints, &first)?,
            cached: &first.cached,
            checking: self.checking(&fingerprints, &first.cached),
        });
        let mut known = self.read(&fingerprints).await?;
        let (ready, mut arrived) = tokio::sync::mpsc::unbounded_channel();
        // A failed store read stops the fingerprints nobody would read any more.
        let preparation = cancellation.child_token();
        // Dependency first: an eval's action is known once its gates' are, so what is upstream
        // is fingerprinted first and the graph fills in from there.
        let targets = self
            .graph
            .components()
            .iter()
            .flat_map(|component| &component.artifacts)
            .filter_map(|id| targets.get(*id).copied());
        let preparing = cache::prepare_each(
            self.config,
            targets,
            self.state,
            parallelism,
            preparation.clone(),
            move |id, fingerprint| {
                let _ = ready.send((id, fingerprint));
            },
        );
        let publishing = async {
            while let Some((id, fingerprint)) = arrived.recv().await {
                let started = tokio::time::Instant::now();
                fingerprints.insert(id, fingerprint);
                while let Ok((id, fingerprint)) = arrived.try_recv() {
                    fingerprints.insert(id, fingerprint);
                }
                self.absorb(&mut known, &fingerprints).await?;
                publish(Progress {
                    view: self.assemble(&fingerprints, &known)?,
                    cached: &known.cached,
                    checking: self.checking(&fingerprints, &known.cached),
                });
                // Publishing takes from the fingerprints' time: leave them as long again.
                tokio::time::sleep(started.elapsed()).await;
            }
            Ok::<_, String>(())
        };
        let publishing = async {
            let published = publishing.await;
            if published.is_err() {
                preparation.cancel();
            }
            published
        };
        let (prepared, published) = tokio::join!(preparing, publishing);
        prepared?;
        published?;
        Ok(fingerprints)
    }

    /// What the store says about `fingerprints`, read afresh.
    async fn read(&self, fingerprints: &Fingerprints<'a>) -> Result<Known<'a>, String> {
        let mut known = Known {
            latest: store::read_latest_requests(self.state, &self.config.root).await?,
            waiting: store::read_waiting(self.state, Some(&self.config.root)).await?,
            unreadable: store::read_unreadable(self.state, Some(&self.config.root)).await?,
            ..Known::default()
        };
        self.absorb(&mut known, fingerprints).await?;
        Ok(known)
    }

    /// Look up the keys `fingerprints` complete that `known` has not looked up yet.
    async fn absorb(
        &self,
        known: &mut Known<'a>,
        fingerprints: &Fingerprints<'a>,
    ) -> Result<(), String> {
        let new: BTreeMap<&'a crate::config::EvalId, cache::Key> = self
            .config
            .evals
            .iter()
            .filter(|eval| !known.keys.contains_key(&eval.id))
            .filter_map(|eval| {
                Some((
                    &eval.id,
                    cache::eval_key(self.config, eval, fingerprints).ok()?,
                ))
            })
            .collect();
        let selected: BTreeSet<_> = self.selected_eval_ids.iter().collect();
        let keys: BTreeMap<_, _> = new
            .iter()
            .filter(|(id, _)| !(self.options.force && selected.contains(**id)))
            .map(|(id, key)| (*id, key))
            .collect();
        let mut cached = store::read_profiled_executions(
            self.state,
            self.config
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
        if let Some(remote) = self.remote {
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
        // Explain changed keys against the newest cached result for the same Eval definition.
        let stale: Vec<_> = keys
            .iter()
            .filter(|(id, _)| !matches!(cached.get(**id), Some(Claim::Reuse(_))))
            .map(|(id, key)| ((*id).clone(), key.eval_def_hash.clone()))
            .collect();
        known
            .previous
            .extend(store::read_latest_cached(self.state, &stale).await?);
        known.cached.extend(cached);
        known.keys.extend(new);
        Ok(())
    }

    fn assemble(
        &self,
        fingerprints: &Fingerprints<'a>,
        known: &Known<'a>,
    ) -> Result<StatusView, String> {
        view::assemble(
            self.config,
            self.graph,
            self.selection,
            self.options,
            Snapshot {
                required: self.required,
                selected_eval_ids: self.selected_eval_ids,
                included_eval_ids: self.included_eval_ids,
                fingerprints,
                known,
            },
        )
    }

    /// The evals whose action may still change: a fingerprint their key needs is not in yet,
    /// or, unless they reuse a result, the action of an eval they wait for may still change.
    fn checking(
        &self,
        fingerprints: &Fingerprints<'a>,
        cached: &BTreeMap<crate::config::EvalId, Claim>,
    ) -> BTreeSet<&'a crate::config::EvalId> {
        let evals: BTreeMap<_, _> = self
            .config
            .evals
            .iter()
            .map(|eval| (&eval.id, eval))
            .collect();
        let mut settled = BTreeSet::new();
        let mut checking = BTreeSet::new();
        // Dependency first, so an eval's gates are decided before it; gates in its own cycle
        // are not, and the cycle stays being checked until every fingerprint is in.
        for component in self.graph.components() {
            for artifact in component
                .artifacts
                .iter()
                .filter(|id| self.required.contains(*id))
            {
                for id in &self.graph.artifacts()[*artifact].evals {
                    let keyed =
                        cache::dependencies(self.config, evals[id])
                            .into_iter()
                            .all(|input| {
                                self.config.artifacts[input].fingerprint.is_none()
                                    || fingerprints.contains_key(input)
                            });
                    let decided = keyed
                        && (matches!(cached.get(*id), Some(Claim::Reuse(_)))
                            || self
                                .graph
                                .gates(id)
                                .iter()
                                .all(|gate| settled.contains(gate)));
                    if decided {
                        settled.insert(*id);
                    } else {
                        checking.insert(*id);
                    }
                }
            }
        }
        checking
    }
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
