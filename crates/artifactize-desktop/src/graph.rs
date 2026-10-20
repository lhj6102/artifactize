pub(crate) mod current;

// The selected Run as a graph of Artifacts for the monitor's Graph view: nodes, dependency
// edges and their lights come from the library's [`monitor::graph`] over the shown tree.
use crate::{App, Failure, ids::ShapeKey};
use artifactize_core::monitor;
use serde::Serialize;
use specta::Type;

/// How a node or edge is lit, in precedence order; the frontend maps each to one colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Light {
    /// An eval's verdict is RED.
    Red,
    /// An eval failed operationally: ERROR.
    Error,
    /// Running, or waiting for its Human.
    Active,
    /// Cannot proceed: an upstream Artifact is RED, ERROR or itself blocked.
    Blocked,
    /// Runnable but still waiting: queued, or for upstream Artifacts not done yet.
    Waiting,
    /// Every eval is GREEN.
    Green,
}

impl From<monitor::Light> for Light {
    fn from(light: monitor::Light) -> Self {
        match light {
            monitor::Light::Red => Self::Red,
            monitor::Light::Error => Self::Error,
            monitor::Light::Active => Self::Active,
            monitor::Light::Blocked => Self::Blocked,
            monitor::Light::Waiting => Self::Waiting,
            monitor::Light::Green => Self::Green,
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GraphNode {
    /// The Artifact row's tree id, `a:<artifact>`.
    pub id: crate::ids::NodeKey,
    pub name: String,
    pub light: Light,
}

/// `target`'s evals depend on `source`; both are tree ids.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdge {
    pub source: crate::ids::NodeKey,
    pub target: crate::ids::NodeKey,
    pub light: Light,
}

/// The Artifacts and the edges between them, each with its light.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Shape {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

/// What changed in a Run graph since the shape the caller holds.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Update {
    /// A shape the caller does not hold: its Artifacts, upstream first in the tree's order, and
    /// its edges, all lit.
    Shape(Shape),
    /// The caller's shape, relit: one light per node and per edge, in its order.
    Lights {
        nodes: Vec<Light>,
        edges: Vec<Light>,
    },
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RunGraph {
    pub run: crate::ids::RunKey,
    /// Names the graph's Artifacts and edges, whatever their lights.
    pub shape: ShapeKey,
    pub update: Update,
}

fn id(name: &artifactize_core::types::ArtifactName) -> crate::ids::NodeKey {
    monitor::NodeId::artifact(name.clone()).into()
}

fn node(node: &monitor::GraphNode) -> GraphNode {
    GraphNode {
        id: node.id.clone().into(),
        name: node.name.to_string(),
        light: node.light.into(),
    }
}

fn edge(edge: &monitor::GraphEdge) -> GraphEdge {
    GraphEdge {
        source: id(&edge.from),
        target: id(&edge.to),
        light: edge.light.into(),
    }
}

/// The selected Run `run` as a graph of its Artifacts, from the shown snapshot: only its
/// lights while it keeps the shape `known` the caller holds.
#[tauri::command]
#[specta::specta]
pub async fn run_graph(
    app: tauri::State<'_, App>,
    run: crate::ids::RunKey,
    known: Option<ShapeKey>,
) -> Result<RunGraph, Failure> {
    let selected = app.browse.selected(&run.into_inner())?;
    let graph = &selected.graph;
    let update = if known == Some(selected.shape) {
        Update::Lights {
            nodes: graph.nodes.iter().map(|node| node.light.into()).collect(),
            edges: graph.edges.iter().map(|edge| edge.light.into()).collect(),
        }
    } else {
        Update::Shape(Shape {
            nodes: graph.nodes.iter().map(node).collect(),
            edges: graph.edges.iter().map(edge).collect(),
        })
    };
    Ok(RunGraph {
        run: selected.run.run.id.clone().into(),
        shape: selected.shape,
        update,
    })
}

/// An Artifact between its neighbours, at most [`NEAR`] on each side: those shown first are the
/// ones lit first in [`Light`]'s precedence (failing before passing), then in the tree's order.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Neighbours {
    /// The Artifact, the neighbours shown, and the edges between it and them.
    pub shape: Shape,
    /// Artifacts its evals use beyond those shown.
    pub more_uses: u32,
    /// Artifacts whose evals use it beyond those shown.
    pub more_used_by: u32,
}

/// Neighbours shown on each side: a shared input that thousands of Artifacts use would
/// otherwise draw thousands of rows in the Detail, too small to read.
pub const NEAR: usize = 16;

/// The Artifact `node` of the selected Run `run` between its neighbours: the Artifacts its
/// evals use, itself, then those whose evals use it, with the edges between it and them.
#[tauri::command]
#[specta::specta]
pub async fn run_neighbours(
    app: tauri::State<'_, App>,
    run: crate::ids::RunKey,
    node: crate::ids::NodeKey,
) -> Result<Neighbours, Failure> {
    let selected = app.browse.selected(&run.into_inner())?;
    Ok(neighbours(&selected.graph, &node.into_inner()))
}

fn neighbours(graph: &monitor::Graph, node: &monitor::NodeId) -> Neighbours {
    let Some(own) = graph.nodes.iter().find(|item| &item.id == node) else {
        return Neighbours {
            shape: Shape {
                nodes: Vec::new(),
                edges: Vec::new(),
            },
            more_uses: 0,
            more_used_by: 0,
        };
    };
    let lights: std::collections::BTreeMap<_, Light> = graph
        .nodes
        .iter()
        .map(|item| (&item.name, item.light.into()))
        .collect();
    // One side's edges, those shown first, cut to `NEAR`; and how many were cut.
    let side =
        |far: fn(&monitor::GraphEdge) -> &artifactize_core::types::ArtifactName,
         near: fn(&monitor::GraphEdge) -> &artifactize_core::types::ArtifactName| {
            let mut edges: Vec<_> = graph
                .edges
                .iter()
                .filter(|edge| near(edge) == &own.name)
                .collect();
            edges.sort_by_key(|edge| lights.get(far(edge)));
            let more = edges.len().saturating_sub(NEAR);
            edges.truncate(NEAR);
            (edges, u32::try_from(more).unwrap_or(u32::MAX))
        };
    let (uses, more_uses) = side(|edge| &edge.from, |edge| &edge.to);
    let (used_by, more_used_by) = side(|edge| &edge.to, |edge| &edge.from);
    let edges: Vec<_> = uses.into_iter().chain(used_by).collect();
    let near: std::collections::BTreeSet<_> = edges
        .iter()
        .flat_map(|edge| [&edge.from, &edge.to])
        .collect();
    Neighbours {
        shape: Shape {
            nodes: graph
                .nodes
                .iter()
                .filter(|item| near.contains(&item.name) || item.id == own.id)
                .map(self::node)
                .collect(),
            edges: edges.into_iter().map(edge).collect(),
        },
        more_uses,
        more_used_by,
    }
}

#[cfg(test)]
mod tests;
