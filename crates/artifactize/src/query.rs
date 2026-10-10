//! Read-only state queries and result projections.

mod summary;
pub use summary::{
    Kinds, Source, SourceKind, profile_name, request_output, request_summary, reused, run_output,
    run_summary, source,
};

use std::{collections::BTreeMap, path::Path};

use serde::Serialize;

use crate::{
    config::{Artifact, Eval, RepoConfig},
    graph::Graph,
    project::selection::Selection,
    scope::Relation,
};

/// The format version of the printed graph: the shape of its Artifacts, evals, relations
/// and components. It changes when a field is removed or changes meaning, so that scripts
/// reading the output can refuse a shape they do not know.
const GRAPH_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphView<'a> {
    pub version: u32,
    #[serde(serialize_with = "crate::platform::path_serde::serialize")]
    pub repo_path: &'a Path,
    pub selection: &'a Selection,
    pub artifacts: BTreeMap<&'a crate::config::ArtifactName, &'a Artifact>,
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
    pub artifacts: Vec<&'a crate::config::ArtifactName>,
    pub dependencies: Vec<usize>,
    pub gates: Vec<&'a crate::config::EvalId>,
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
        .filter(|eval| artifacts.contains_key(&eval.target))
        .collect();
    let relations: Vec<_> = config
        .relations
        .iter()
        .filter(|relation| {
            artifacts.contains_key(&relation.source) && artifacts.contains_key(&relation.target)
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
            dependencies: component
                .dependencies
                .iter()
                .map(|dependency| dependency.index())
                .collect(),
            gates: component.gates.clone(),
            cyclic: component.artifacts.len() > 1
                || relations.iter().any(|edge| {
                    edge.cyclic && component.artifacts.contains(&&edge.relation.source)
                }),
        })
        .collect();
    Ok(GraphView {
        version: GRAPH_FORMAT_VERSION,
        repo_path: &config.root,
        selection,
        artifacts,
        evals,
        relations,
        components,
    })
}
