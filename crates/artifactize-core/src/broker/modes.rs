//! Scheduling policy for speculative work and deferred sign-offs.
use std::collections::{BTreeMap, BTreeSet};

use crate::{
    config::{EvalId, ProfileKind},
    graph::{Evidence, Graph, Readiness},
    store::{Request, Run},
};

/// Every Human sign-off is deferred: it never holds up other evals. A Run that only reuses
/// Human results requests none.
pub(super) fn deferred(run: &Run, requests: &[Request]) -> BTreeSet<EvalId> {
    if run.reuse_only.contains(&ProfileKind::Human) {
        return BTreeSet::new();
    }
    requests
        .iter()
        .filter(|request| request.profile.kind() == ProfileKind::Human)
        .map(|request| request.eval_id.clone())
        .collect()
}

/// Longest remaining dependency path is counted in eval obligations, not wall-clock
/// guesses. Artifact cycle peers do not gate one another; each Eval uses its own references.
pub(super) fn order(
    graph: &Graph<'_>,
    requests: &[Request],
    evidence: &BTreeMap<EvalId, Evidence>,
) -> Vec<usize> {
    let evaluation = graph.evaluate(evidence);
    let predecessors: BTreeMap<_, BTreeSet<_>> = requests
        .iter()
        .map(|request| {
            (
                request.eval_id.clone(),
                graph.gates(&request.eval_id).into_iter().cloned().collect(),
            )
        })
        .collect();
    let mut successors: BTreeMap<EvalId, BTreeSet<EvalId>> = predecessors
        .keys()
        .map(|id| (id.clone(), BTreeSet::new()))
        .collect();
    for (id, gates) in &predecessors {
        for gate in gates {
            successors
                .entry(gate.clone())
                .or_default()
                .insert(id.clone());
        }
    }
    let weight = |id: &EvalId| {
        let eval = &evaluation.evals[id.as_str()];
        usize::from(
            eval.status != crate::graph::EvalStatus::Blocked
                && !matches!(
                    evidence.get(id),
                    Some(Evidence::Current(_)) | Some(Evidence::OperationalError)
                ),
        )
    };
    let mut pending: BTreeMap<_, _> = successors
        .iter()
        .map(|(id, next)| (id.clone(), next.len()))
        .collect();
    let mut ready: Vec<_> = pending
        .iter()
        .filter_map(|(id, count)| (*count == 0).then_some(id.clone()))
        .collect();
    let mut lengths: BTreeMap<_, _> = pending.keys().map(|id| (id.clone(), weight(id))).collect();
    // Reverse topological reduction avoids recursion even for long Artifact chains.
    while let Some(id) = ready.pop() {
        let tail = lengths[&id];
        for gate in predecessors.get(&id).into_iter().flatten() {
            let length = lengths.get_mut(gate).expect("indexed gate");
            *length = (*length).max(weight(gate) + tail);
            let count = pending.get_mut(gate).expect("indexed gate");
            *count -= 1;
            if *count == 0 {
                ready.push(gate.clone());
            }
        }
    }
    let mut order: Vec<_> = (0..requests.len()).collect();
    order.sort_by_key(|&index| {
        let id = &requests[index].eval_id;
        (
            evaluation.evals[id.as_str()].readiness != Readiness::Ready,
            std::cmp::Reverse(lengths[id]),
            index,
        )
    });
    order
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stop {
    Blocked,
    Yield,
}
