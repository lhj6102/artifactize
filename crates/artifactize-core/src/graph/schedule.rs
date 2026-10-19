//! Eval gates over Artifact dependency cycles, shared by live scheduling and saved Run views.
//!
//! An eval is gated, never an Artifact. Its gates are the evals of the Artifacts it depends
//! on outside its own Artifact's cycle: a RED there means that content will change, so the
//! eval's result would not be fresh. Its own Artifact and the Artifacts in a dependency cycle
//! with it are evaluated together, so their evals never gate it.
//!
//! An eval depends on the Artifacts it references and on its target's children. A child
//! folder's files are part of its parent's content, so a child counts exactly as if every
//! eval of the parent referenced it. Cycles are the Artifact cycles these links form.
//!
//! A Human eval also waits for every non-Human eval of its own Artifact: a person signs off
//! only on what the machine evals already pass. Other Human evals never gate it.
use std::collections::{BTreeMap, BTreeSet};

use petgraph::{algo::kosaraju_scc, graph::DiGraph};

use crate::types::{ArtifactName, EvalId};

/// One eval as the Schedule sees it.
pub(crate) struct Input<'a> {
    pub id: &'a EvalId,
    pub target: &'a ArtifactName,
    /// The Artifacts it references.
    pub deps: Vec<&'a ArtifactName>,
    pub human: bool,
}

#[derive(Debug)]
pub(crate) struct Schedule<'a> {
    /// Every eval after all of its gates: Artifact cycles dependency-first, evals by ID.
    pub order: Vec<&'a EvalId>,
    pub gates: BTreeMap<&'a EvalId, Vec<&'a EvalId>>,
}

impl<'a> Schedule<'a> {
    /// `children` are `(parent, child)`. A reference names material, and its gates are all
    /// evals of that material's cycle. A reference into the eval's own cycle brings the
    /// gates the cycle's evals reference outside it, since that is what the referenced
    /// peer's content depends on.
    pub fn new(
        evals: impl IntoIterator<Item = Input<'a>>,
        children: impl IntoIterator<Item = (&'a ArtifactName, &'a ArtifactName)>,
    ) -> Self {
        let mut links: BTreeMap<&ArtifactName, BTreeSet<&ArtifactName>> = BTreeMap::new();
        for (parent, child) in children {
            links.entry(parent).or_default().insert(child);
        }
        // A child is a reference every eval of its parent makes.
        let evals: Vec<_> = evals
            .into_iter()
            .map(|mut eval| {
                eval.deps
                    .extend(links.get(eval.target).into_iter().flatten().copied());
                eval.deps.sort_unstable();
                eval.deps.dedup();
                eval
            })
            .collect();
        // Consumer -> input, so the SCC traversal yields dependencies first.
        // Children link their parent whether or not it owns evals, so the cycles are the
        // Artifact cycles of the content.
        let edges: BTreeSet<(&ArtifactName, &ArtifactName)> =
            evals
                .iter()
                .flat_map(|eval| eval.deps.iter().map(|dep| (eval.target, *dep)))
                .chain(links.iter().flat_map(|(&parent, children)| {
                    children.iter().map(move |&child| (parent, child))
                }))
                .collect();
        let mut graph = DiGraph::<&ArtifactName, ()>::new();
        let nodes: BTreeMap<_, _> = evals
            .iter()
            .map(|eval| eval.target)
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
        for eval in &evals {
            let own = cycle_of[eval.target];
            members[own].push(eval);
            outside[own].extend(
                eval.deps
                    .iter()
                    .map(|dep| cycle_of[dep])
                    .filter(|&cycle| cycle != own),
            );
        }
        // Within a cycle, machine evals come before the Human evals that wait for them.
        for evals in &mut members {
            evals.sort_unstable_by_key(|eval| (eval.human, eval.id));
        }
        let gates = evals
            .iter()
            .map(|eval| {
                let own = cycle_of[eval.target];
                let mut sources: BTreeSet<_> = eval.deps.iter().map(|dep| cycle_of[dep]).collect();
                if sources.remove(&own) {
                    sources.extend(&outside[own]);
                }
                let mut gates: Vec<_> = sources
                    .into_iter()
                    .flat_map(|cycle| members[cycle].iter().map(|eval| eval.id))
                    .collect();
                if eval.human {
                    gates.extend(
                        members[own]
                            .iter()
                            .filter(|sibling| sibling.target == eval.target && !sibling.human)
                            .map(|sibling| sibling.id),
                    );
                }
                gates.sort_unstable();
                (eval.id, gates)
            })
            .collect();
        Self {
            order: members.into_iter().flatten().map(|eval| eval.id).collect(),
            gates,
        }
    }
}
