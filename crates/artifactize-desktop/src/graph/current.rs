//! Current declaration structure, not a historical Run or a state database snapshot.
use super::{GraphEdge, GraphNode, Light, Shape};
use crate::{
    App, Failure,
    browse::{NodeKind, TreeNode},
    status::{Tone, Weight},
};
use artifactize_core::{
    graph_structure::{self, Structure},
    monitor::NodeId,
};
use serde::Serialize;
use specta::Type;
use std::collections::BTreeSet;

#[derive(Debug, Serialize, Type)]
pub struct CurrentGraph {
    pub fingerprint: crate::ids::FingerprintKey,
    pub graph: Shape,
    pub tree: Vec<TreeNode>,
    /// Complete, canonical structural data rendered as text: criteria and relation labels
    /// remain inspectable even when several relations share one graph edge.
    pub structure: String,
}

fn row(
    id: NodeId,
    kind: NodeKind,
    name: String,
    right: String,
    children: Vec<TreeNode>,
) -> TreeNode {
    TreeNode {
        id: id.into(),
        kind,
        name,
        right,
        children,
        glyph: "·".into(),
        tone: Tone::Muted,
        weight: Weight::Normal,
        marks: String::new(),
        text: Vec::new(),
        changed: false,
        done: false,
        attention: false,
        urgent: false,
        upstream: Vec::new(),
    }
}

pub(super) fn project(structure: &Structure) -> CurrentGraph {
    let id = |name: &artifactize_core::types::ArtifactName| -> crate::ids::NodeKey {
        NodeId::artifact(name.clone()).into()
    };
    let nodes = structure
        .artifacts
        .keys()
        .map(|name| GraphNode {
            id: id(name),
            name: name.to_string(),
            light: Light::Waiting,
        })
        .collect();
    let pairs: BTreeSet<_> = structure
        .relations
        .iter()
        .map(|edge| (&edge.source, &edge.target))
        .collect();
    let edges = pairs
        .into_iter()
        .map(|(source, target)| GraphEdge {
            source: id(source),
            target: id(target),
            light: Light::Waiting,
        })
        .collect();
    let tree = structure
        .artifacts
        .iter()
        .map(|(name, artifact)| {
            let evals = structure
                .evals
                .iter()
                .filter(|(_, eval)| &eval.target == name)
                .map(|(id, eval)| {
                    row(
                        NodeId::eval(id.clone()),
                        NodeKind::Eval,
                        eval.title.clone(),
                        id.to_string(),
                        Vec::new(),
                    )
                })
                .collect();
            row(
                NodeId::artifact(name.clone()),
                NodeKind::Artifact,
                name.to_string(),
                format!("{:?}", artifact.kind),
                evals,
            )
        })
        .collect();
    CurrentGraph {
        fingerprint: structure.fingerprint().into(),
        graph: Shape { nodes, edges },
        tree,
        structure: structure.tree(),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn current_graph(app: tauri::State<'_, App>) -> Result<CurrentGraph, Failure> {
    let repo = app
        .launch
        .repo
        .clone()
        .ok_or_else(|| Failure::Read("Current graph requires --repo PATH.".into()))?;
    let structure = crate::failure::joined(
        tokio::task::spawn_blocking(move || graph_structure::read(&repo)).await,
    )?
    .map_err(Failure::Read)?;
    Ok(project(&structure))
}
