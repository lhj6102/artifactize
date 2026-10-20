//! Pure status projection over one loaded snapshot; no store, filesystem or remote reads.
use super::*;

pub(super) fn assemble(
    config: &crate::config::RepoConfig,
    graph: &Graph<'_>,
    selection: &Selection,
    options: &VerifyOptions,
    snapshot: Snapshot<'_>,
) -> Result<StatusView, String> {
    let Snapshot {
        required,
        selected_eval_ids,
        included_eval_ids,
        fingerprints,
        all_keys,
        cached,
        previous,
        mut latest,
        waiting,
        unreadable,
    } = snapshot;
    let selected_ids: BTreeSet<_> = selected_eval_ids.iter().collect();
    let included_ids: BTreeSet<_> = included_eval_ids.iter().collect();
    let keys: BTreeMap<_, _> = all_keys
        .iter()
        .filter(|(id, _)| !(options.force && selected_ids.contains(*id)))
        .map(|(id, key)| (*id, key))
        .collect();
    let claim = |id: &crate::types::EvalId| cached.get(id);
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
    for eval in &config.evals {
        if let Some(Claim::Reuse(execution)) = claim(&eval.id) {
            evidence.insert(
                eval.id.clone(),
                Evidence::Current(execution.verdict().expect("completed cache entry")),
            );
        }
    }
    let evaluation = graph.evaluate(&evidence);
    let deferred: BTreeSet<_> = waiting
        .into_iter()
        .filter(|view| view.request.status() == crate::types::RequestStatus::Deferred)
        .filter(|view| {
            matches!(
                claim(&view.request.eval_id),
                Some(Claim::WaitHuman(id)) if Some(id) == view.request.execution_id.as_ref()
            )
        })
        .map(|view| view.request.eval_id)
        .collect();
    let mut shown_evidence = evidence.clone();
    for id in &deferred {
        shown_evidence.insert(id.clone(), Evidence::Deferred);
    }
    let shown = graph.evaluate(&shown_evidence);
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
        let status = ArtifactCondition::from(shown.artifacts[id.as_str()].status);
        let own = &graph.artifacts()[id.as_str()];
        let unmet = graph
            .dependency_closure(&[*id])
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
        let force = options.force && selected;
        let (action, reason) = match current.readiness {
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
                        // Status never treats an ERROR as a blocker, so every cause is RED.
                        Readiness::Blocked => format!(
                            "; its gates are blocked by RED: {}",
                            current
                                .causes
                                .iter()
                                .map(|cause| cause.eval().as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    }
                ),
            ),
            Readiness::Blocked => (
                VerifyAction::Blocked,
                current.blocked_reason().unwrap_or_default(),
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
                Profile::Human { .. } => (
                    VerifyAction::Execute,
                    "Record a request awaiting a Human claim and submission.".into(),
                ),
                Profile::Runtime { .. } | Profile::Agent { .. } => (
                    VerifyAction::Execute,
                    if force {
                        "An explicitly forced Eval requires a new execution.".into()
                    } else {
                        match cache::eval_key(config, eval, &fingerprints) {
                            Ok(_) => {
                                "The current eval and fingerprints have no completed cached result."
                                    .into()
                            }
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
            if deferred.contains(&eval.id) {
                EvalCondition::Deferred
            } else {
                EvalCondition::WaitingHuman
            }
        } else {
            EvalCondition::from(shown.evals[eval.id.as_str()].status)
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
            blocked_by: current.blocked_by.iter().copied().map(Into::into).collect(),
            obligations: obligations_by_artifact[eval.target.as_str()].clone(),
            eval_def_hash: cache::eval_definition_hash(&eval.declaration),
            fingerprint: fingerprints
                .get(&eval.target)
                .map(|fingerprint| fingerprint.value.clone()),
            fingerprints: cache::dependencies(config, eval)
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
            key: all_keys.get(&eval.id).map(|key| key.value.clone()),
            last: latest.remove(eval.id.as_str()),
            changes: keys.get(&eval.id).and_then(|key| {
                previous
                    .get(&(eval.id.clone(), key.eval_def_hash.clone()))
                    .map(|execution| {
                        cache::changes(
                            execution,
                            &eval.target,
                            key,
                            fingerprints[&eval.target].manifest.as_ref(),
                        )
                    })
            }),
        });
    }
    Ok(StatusView {
        unreadable,
        selection: selection.clone(),
        recursive: options.recursive,
        force: options.force,
        selected_eval_ids,
        included_eval_ids,
        satisfied: obligations.is_empty(),
        obligations,
        artifacts,
        evals,
        counts,
    })
}
