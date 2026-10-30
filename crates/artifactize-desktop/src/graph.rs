pub(crate) mod current;
pub(crate) mod find;

// A graph of Artifacts for the Main area: the shown Run's, lit by the library's
// [`monitor::graph`] over the shown tree, or a branch's Current, lit by what verify would do.
use crate::{App, Failure, browse::BranchKey, ids::ShapeKey};
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
    /// On a Current: at least one eval would run.
    Run,
    /// On a Current: every eval's result would be reused.
    Reused,
    /// On a Current: what would happen is still being checked.
    Checking,
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
            Plan::Check => Self::Checking,
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
    /// The Artifacts of the graph it is made of directly, by tree id: drawn inside it.
    pub parts: Vec<crate::ids::NodeKey>,
}

/// `target`'s evals depend on `source`; both are tree ids.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdge {
    pub source: crate::ids::NodeKey,
    pub target: crate::ids::NodeKey,
    pub light: Light,
}

/// The Artifacts, each with its parts, and the edges between them, each with its light.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Shape {
    /// Upstream first, in the tree's order.
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

/// What changed in a graph since the shape the caller holds.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Update {
    /// A shape the caller does not hold, all lit.
    Shape(Shape),
    /// The caller's shape, relit: one light per node and per edge, in its order.
    Lights {
        nodes: Vec<Light>,
        edges: Vec<Light>,
    },
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Graph {
    pub source: Source,
    /// Names the graph's Artifacts and edges, whatever their lights.
    pub shape: ShapeKey,
    pub update: Update,
}

pub(crate) fn node_id(name: &artifactize_core::types::ArtifactName) -> crate::ids::NodeKey {
    monitor::NodeId::artifact(name.clone()).into()
}

/// The shown Run's graph with the API's ids and lights.
fn shape(graph: &monitor::Graph) -> Shape {
    Shape {
        nodes: graph
            .nodes
            .iter()
            .map(|node| GraphNode {
                id: node.id.clone().into(),
                name: node.name.to_string(),
                light: node.light.into(),
                note: None,
                parts: node.parts.iter().map(node_id).collect(),
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
    }
}

/// The graph of `source`: the shown Run's, from the shown snapshot, only its lights while it
/// keeps the shape `known` the caller holds; or a branch's Current as its last scan found it.
#[tauri::command]
#[specta::specta]
pub async fn graph(
    app: tauri::State<'_, App>,
    source: Source,
    known: Option<ShapeKey>,
) -> Result<Graph, Failure> {
    match &source {
        Source::Run { run } => {
            let selected = app.browse.selected(&run.clone().into_inner())?;
            let graph = &selected.graph;
            let update = if known == Some(selected.shape) {
                Update::Lights {
                    nodes: graph.nodes.iter().map(|node| node.light.into()).collect(),
                    edges: graph.edges.iter().map(|edge| edge.light.into()).collect(),
                }
            } else {
                Update::Shape(shape(graph))
            };
            Ok(Graph {
                source,
                shape: selected.shape,
                update,
            })
        }
        // A Current is read once a scan, and its notes change with its lights: it comes whole.
        Source::Current { branch } => {
            let shown = app.currents.shown(&branch.to_place())?;
            Ok(Graph {
                source,
                shape: shown.shape,
                update: Update::Shape(current::shape(&shown.current)),
            })
        }
    }
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
    /// The Artifacts it is a part of directly, by name in the graph's order.
    pub wholes: Vec<String>,
    /// How many times the graph draws it: once inside each drawing of each of its wholes.
    pub copies: u32,
}

/// Neighbours shown on each side: a shared input that thousands of Artifacts use would
/// otherwise draw thousands of rows in the Detail, too small to read.
pub const NEAR: usize = 16;

/// The Artifact `node` of `source` between its neighbours: the Artifacts its evals use,
/// itself, then those whose evals use it, with the edges between it and them.
#[tauri::command]
#[specta::specta]
pub async fn neighbours(
    app: tauri::State<'_, App>,
    source: Source,
    node: crate::ids::NodeKey,
) -> Result<Neighbours, Failure> {
    let graph = match source {
        Source::Run { run } => shape(&app.browse.selected(&run.into_inner())?.graph),
        Source::Current { branch } => {
            current::shape(&app.currents.shown(&branch.to_place())?.current)
        }
    };
    Ok(self::neighbours_in(&graph, &node.into_inner()))
}

fn neighbours_in(graph: &Shape, node: &monitor::NodeId) -> Neighbours {
    let Some(own) = graph.nodes.iter().find(|item| item.id.inner() == node) else {
        return Neighbours {
            shape: Shape {
                nodes: Vec::new(),
                edges: Vec::new(),
            },
            more_uses: 0,
            more_used_by: 0,
            wholes: Vec::new(),
            copies: 0,
        };
    };
    let lights: std::collections::BTreeMap<_, Light> = graph
        .nodes
        .iter()
        .map(|item| (item.id.inner(), item.light))
        .collect();
    // One side's edges, those shown first, cut to `NEAR`; and how many were cut.
    let side = |far: fn(&GraphEdge) -> &crate::ids::NodeKey,
                near: fn(&GraphEdge) -> &crate::ids::NodeKey| {
        let mut edges: Vec<_> = graph
            .edges
            .iter()
            .filter(|edge| near(edge) == &own.id)
            .collect();
        edges.sort_by_key(|edge| lights.get(far(edge).inner()));
        let more = edges.len().saturating_sub(NEAR);
        edges.truncate(NEAR);
        (edges, u32::try_from(more).unwrap_or(u32::MAX))
    };
    let (uses, more_uses) = side(|edge| &edge.source, |edge| &edge.target);
    let (used_by, more_used_by) = side(|edge| &edge.target, |edge| &edge.source);
    let edges: Vec<_> = uses.into_iter().chain(used_by).collect();
    let near: std::collections::BTreeSet<_> = edges
        .iter()
        .flat_map(|edge| [edge.source.inner(), edge.target.inner()])
        .collect();
    Neighbours {
        shape: Shape {
            nodes: graph
                .nodes
                .iter()
                .filter(|item| near.contains(item.id.inner()) || item.id == own.id)
                .cloned()
                .collect(),
            edges: edges.into_iter().cloned().collect(),
        },
        more_uses,
        more_used_by,
        wholes: graph
            .nodes
            .iter()
            .filter(|item| item.parts.contains(&own.id))
            .map(|item| item.name.clone())
            .collect(),
        copies: copies(graph, &own.id),
    }
}

/// How many times `graph` draws `node`: once at the top, unless it is a part, else once inside
/// each drawing of each Artifact it is a part of.
fn copies(graph: &Shape, node: &crate::ids::NodeKey) -> u32 {
    type Wholes<'a> = std::collections::BTreeMap<&'a monitor::NodeId, Vec<&'a monitor::NodeId>>;
    let mut wholes: Wholes<'_> = Default::default();
    for item in &graph.nodes {
        for part in &item.parts {
            wholes
                .entry(part.inner())
                .or_default()
                .push(item.id.inner());
        }
    }
    // Parts close no cycle; one in a saved Run would count its way round once.
    fn count<'a>(
        wholes: &Wholes<'a>,
        node: &'a monitor::NodeId,
        path: &mut Vec<&'a monitor::NodeId>,
    ) -> u32 {
        let above = wholes.get(node).map_or(&[][..], Vec::as_slice);
        if above.is_empty() || path.contains(&node) {
            return 1;
        }
        path.push(node);
        let sum = above.iter().fold(0u32, |sum, whole| {
            sum.saturating_add(count(wholes, whole, path))
        });
        path.pop();
        sum
    }
    count(&wholes, node.inner(), &mut Vec::new())
}

#[cfg(test)]
mod tests;
