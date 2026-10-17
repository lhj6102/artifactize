//! Eval-level dependency condensation, shared by live scheduling and saved Run views.
use std::collections::{BTreeMap, BTreeSet};

use petgraph::{algo::kosaraju_scc, graph::DiGraph};

use crate::types::{ArtifactName, EvalId};

#[derive(Debug)]
pub(crate) struct Schedule<'a> {
    pub components: Vec<Vec<&'a EvalId>>,
    pub gates: BTreeMap<&'a EvalId, Vec<&'a EvalId>>,
}

impl<'a> Schedule<'a> {
    /// A reference names material; its obligations are all Evals evaluating that material.
    /// Merely owning a sibling Eval or making an Artifact visible is not an Eval dependency.
    pub fn new(
        definitions: impl IntoIterator<Item = (&'a EvalId, &'a ArtifactName, Vec<&'a ArtifactName>)>,
    ) -> Self {
        let definitions: Vec<_> = definitions.into_iter().collect();
        let mut graph = DiGraph::<&EvalId, ()>::new();
        let nodes: BTreeMap<_, _> = definitions
            .iter()
            .map(|(id, _, _)| (*id, graph.add_node(*id)))
            .collect();
        let mut targets: BTreeMap<&ArtifactName, Vec<&EvalId>> = BTreeMap::new();
        for (id, target, _) in &definitions {
            targets.entry(target).or_default().push(id);
        }
        for (id, _, dependencies) in &definitions {
            for dependency in dependencies {
                for gate in targets.get(dependency).into_iter().flatten() {
                    graph.add_edge(nodes[id], nodes[gate], ());
                }
            }
        }
        let components: Vec<Vec<_>> = kosaraju_scc(&graph)
            .into_iter()
            .map(|members| {
                let mut ids: Vec<_> = members.into_iter().map(|node| graph[node]).collect();
                ids.sort_unstable();
                ids
            })
            .collect();
        let component_of: BTreeMap<_, _> = components
            .iter()
            .enumerate()
            .flat_map(|(index, ids)| ids.iter().map(move |id| (*id, index)))
            .collect();
        let mut gates = BTreeMap::new();
        for (index, ids) in components.iter().enumerate() {
            let external: BTreeSet<_> = ids
                .iter()
                .flat_map(|id| graph.neighbors(nodes[id]))
                .map(|node| component_of[graph[node]])
                .filter(|dependency| *dependency != index)
                .collect();
            let mut required: Vec<_> = external
                .into_iter()
                .flat_map(|dependency| components[dependency].iter().copied())
                .collect();
            required.sort_unstable();
            for id in ids {
                gates.insert(*id, required.clone());
            }
        }
        Self { components, gates }
    }
}
