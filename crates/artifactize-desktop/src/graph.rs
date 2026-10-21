pub(crate) mod current;

// A graph of Artifacts for the Main area: the shown Run's, lit by the library's
// [`monitor::graph`] over the shown tree, or a branch's Current, lit by what verify would do.
use crate::{App, Failure, browse::BranchKey};
use artifactize_core::monitor;
use serde::{Deserialize, Serialize};
use specta::Type;

/// What a graph or Detail shows: a saved Run, or a branch's worktree as it is now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Source {
    Run { run: crate::ids::RunKey },
    Current { branch: BranchKey },
}

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
    /// On a Current: at least one eval would run.
    Run,
    /// On a Current: every eval's result would be reused.
    Reused,
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

impl From<monitor::live::Plan> for Light {
    fn from(plan: monitor::live::Plan) -> Self {
        use monitor::live::Plan;
        match plan {
            Plan::Run => Self::Run,
            Plan::Blocked => Self::Blocked,
            Plan::Wait => Self::Waiting,
            Plan::Reuse => Self::Reused,
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
    /// Under the name: on a Current what verify would do, `would run (2 evals)`.
    pub note: Option<String>,
}

/// `target`'s evals depend on `source`; both are tree ids.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdge {
    pub source: crate::ids::NodeKey,
    pub target: crate::ids::NodeKey,
    pub light: Light,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Graph {
    pub source: Source,
    /// Upstream first, in the tree's order.
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

pub(crate) fn node_id(name: &artifactize_core::types::ArtifactName) -> crate::ids::NodeKey {
    monitor::NodeId::artifact(name.clone()).into()
}

/// The graph of `source`: the shown Run's, from the shown snapshot, or a branch's Current as
/// its last scan found it.
#[tauri::command]
#[specta::specta]
pub fn graph(app: tauri::State<'_, App>, source: Source) -> Result<Graph, Failure> {
    match &source {
        Source::Run { run } => {
            let graph = app
                .browse
                .with_tree(&run.clone().into_inner(), monitor::graph)?;
            Ok(Graph {
                nodes: graph
                    .nodes
                    .iter()
                    .map(|node| GraphNode {
                        id: node.id.clone().into(),
                        name: node.name.to_string(),
                        light: node.light.into(),
                        note: None,
                    })
                    .collect(),
                edges: graph
                    .edges
                    .iter()
                    .map(|edge| GraphEdge {
                        source: node_id(&edge.from),
                        target: node_id(&edge.to),
                        light: edge.light.into(),
                    })
                    .collect(),
                source,
            })
        }
        Source::Current { branch } => {
            let current = app.currents.shown(&branch.to_place())?;
            Ok(current::graph(&current.current, source))
        }
    }
}
