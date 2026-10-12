//! The selected Run as a graph of Artifacts for the monitor's Graph view: nodes, dependency
//! edges and their lights come from the library's [`monitor::graph`] over the shown tree.
use crate::{App, Failure};
use artifactize_core::monitor;
use serde::Serialize;
use specta::Type;

/// How a node or edge is lit, in precedence order; the frontend maps each to one colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
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
    pub id: String,
    pub name: String,
    pub light: Light,
}

/// `target`'s evals depend on `source`; both are tree ids.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub light: Light,
    /// A dependency eval of the target names the source and lights the edge.
    pub dependency: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RunGraph {
    pub run: String,
    /// Upstream first, in the tree's order.
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

/// The selected Run `run` as a graph of its Artifacts, from the shown snapshot.
#[tauri::command]
#[specta::specta]
pub fn run_graph(app: tauri::State<'_, App>, run: String) -> Result<RunGraph, Failure> {
    let run: artifactize_core::types::RunId = run.parse().map_err(Failure::Missing)?;
    let graph = app.browse.with_tree(&run, monitor::graph)?;
    let id = |name: &artifactize_core::types::ArtifactName| {
        monitor::NodeId::artifact(name.clone()).to_string()
    };
    Ok(RunGraph {
        nodes: graph
            .nodes
            .iter()
            .map(|node| GraphNode {
                id: node.id.to_string(),
                name: node.name.to_string(),
                light: node.light.into(),
            })
            .collect(),
        edges: graph
            .edges
            .iter()
            .map(|edge| GraphEdge {
                source: id(&edge.from),
                target: id(&edge.to),
                light: edge.light.into(),
                dependency: edge.dependency,
            })
            .collect(),
        run: run.to_string(),
    })
}
