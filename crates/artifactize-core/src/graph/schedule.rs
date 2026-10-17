//! Eval gates over Artifact dependency cycles, shared by live scheduling and saved Run views.
//!
//! An eval is gated, never an Artifact. Its gates are the evals of the Artifacts it depends
//! on outside its own Artifact's cycle: a RED there means that content will change, so the
//! eval's result would not be fresh. Its own Artifact and the Artifacts in a dependency cycle
//! with it are evaluated together, so their evals never gate it.
//!
//! Dependencies are eval references, so cycles are the Artifact cycles those references
//! form. Mounts and children make material visible but, like owning a sibling eval, never
//! gate; a cycle they close in the material graph does not make Artifacts peers here.
use std::collections::{BTreeMap, BTreeSet};

use petgraph::{algo::kosaraju_scc, graph::DiGraph};

use crate::types::{ArtifactName, EvalId};

#[derive(Debug)]
pub(crate) struct Schedule<'a> {
    /// Every eval after all of its gates: Artifact cycles dependency-first, evals by ID.
    pub order: Vec<&'a EvalId>,
    pub gates: BTreeMap<&'a EvalId, Vec<&'a EvalId>>,
}

impl<'a> Schedule<'a> {
    /// `evals` are `(eval, target, references)`. A reference names material, and its gates
    /// are all evals of that material's cycle. A reference into the eval's own cycle brings
    /// the gates the cycle's evals reference outside it, since that is what the referenced
    /// peer's content depends on.
    pub fn new(
        evals: impl IntoIterator<Item = (&'a EvalId, &'a ArtifactName, Vec<&'a ArtifactName>)>,
    ) -> Self {
        let evals: Vec<_> = evals.into_iter().collect();
        // Consumer -> input, so the SCC traversal yields dependencies first.
        let edges: BTreeSet<(&ArtifactName, &ArtifactName)> = evals
            .iter()
            .flat_map(|(_, target, deps)| deps.iter().map(|dep| (*target, *dep)))
            .collect();
        let mut graph = DiGraph::<&ArtifactName, ()>::new();
        let nodes: BTreeMap<_, _> = evals
            .iter()
            .map(|(_, target, _)| *target)
            .chain(
                edges
                    .iter()
                    .flat_map(|&(consumer, input)| [consumer, input]),
            )
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|artifact| (artifact, graph.add_node(artifact)))
            .collect();
        for (consumer, input) in &edges {
            graph.add_edge(nodes[consumer], nodes[input], ());
        }
        let cycles = kosaraju_scc(&graph);
        let graph = &graph;
        let cycle_of: BTreeMap<_, _> = cycles
            .iter()
            .enumerate()
            .flat_map(|(index, members)| members.iter().map(move |&node| (graph[node], index)))
            .collect();
        let mut members = vec![Vec::new(); cycles.len()];
        let mut outside = vec![BTreeSet::new(); cycles.len()];
        for (id, target, deps) in &evals {
            let own = cycle_of[target];
            members[own].push(*id);
            outside[own].extend(
                deps.iter()
                    .map(|dep| cycle_of[dep])
                    .filter(|&cycle| cycle != own),
            );
        }
        for ids in &mut members {
            ids.sort_unstable();
        }
        let gates = evals
            .iter()
            .map(|(id, target, deps)| {
                let own = cycle_of[target];
                let mut sources: BTreeSet<_> = deps.iter().map(|dep| cycle_of[dep]).collect();
                if sources.remove(&own) {
                    sources.extend(&outside[own]);
                }
                let mut gates: Vec<_> = sources
                    .into_iter()
                    .flat_map(|cycle| members[cycle].iter().copied())
                    .collect();
                gates.sort_unstable();
                (*id, gates)
            })
            .collect();
        Self {
            order: members.into_iter().flatten().collect(),
            gates,
        }
    }
}
