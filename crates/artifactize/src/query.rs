//! Read-only state queries and result projections.

mod summary;
pub use summary::{Source, SourceKind, profile_name, request_output, reused, run_output, source};

use std::{collections::BTreeMap, path::Path};

use serde::Serialize;

use crate::{
    config::{Artifact, Eval, RepoConfig},
    graph::Graph,
    project::selection::Selection,
    scope::Relation,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphView<'a> {
    pub version: u32,
    pub repo_path: &'a Path,
    pub selection: &'a Selection,
    pub artifacts: BTreeMap<&'a str, &'a Artifact>,
    pub evals: Vec<&'a Eval>,
    pub relations: Vec<GraphRelation<'a>>,
    pub components: Vec<GraphComponent<'a>>,
}

#[derive(Debug, Serialize)]
pub struct GraphRelation<'a> {
    #[serde(flatten)]
    pub relation: &'a Relation,
    pub cyclic: bool,
}

#[derive(Debug, Serialize)]
pub struct GraphComponent<'a> {
    pub id: usize,
    pub artifacts: Vec<&'a str>,
    pub dependencies: Vec<usize>,
    pub gates: Vec<&'a str>,
    pub cyclic: bool,
}

/// Full static definitions, restricted to the selected dependency closure.
pub fn graph<'a>(
    config: &'a RepoConfig,
    selection: &'a Selection,
) -> Result<GraphView<'a>, String> {
    let graph = Graph::new(config).map_err(|error| error.to_string())?;
    let selected = selection.resolve(config)?;
    let required = graph
        .dependency_closure(&selected.roots)
        .map_err(|error| error.to_string())?;
    let artifacts: BTreeMap<_, _> = required
        .iter()
        .map(|&id| (id, &config.artifacts[id]))
        .collect();
    let evals = config
        .evals
        .iter()
        .filter(|eval| artifacts.contains_key(eval.target.as_str()))
        .collect();
    let relations: Vec<_> = config
        .relations
        .iter()
        .filter(|relation| {
            artifacts.contains_key(relation.source.as_str())
                && artifacts.contains_key(relation.target.as_str())
        })
        .map(|relation| GraphRelation {
            relation,
            cyclic: graph.artifacts()[relation.source.as_str()].component
                == graph.artifacts()[relation.target.as_str()].component,
        })
        .collect();
    let components = graph
        .components()
        .iter()
        .enumerate()
        .filter(|(_, component)| artifacts.contains_key(component.artifacts[0]))
        .map(|(id, component)| GraphComponent {
            id,
            artifacts: component.artifacts.clone(),
            dependencies: component.dependencies.clone(),
            gates: component.gates.clone(),
            cyclic: component.artifacts.len() > 1
                || relations.iter().any(|edge| {
                    edge.cyclic && component.artifacts.contains(&edge.relation.source.as_str())
                }),
        })
        .collect();
    Ok(GraphView {
        version: 1,
        repo_path: &config.root,
        selection,
        artifacts,
        evals,
        relations,
        components,
    })
}
